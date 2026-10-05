//! Bounded native image bridge for the CPU GLB renderer. No GPU backend hooks.
use super::*;
use crate::model_preview::{self, OrbitCamera, OrbitCameraState, PreviewMesh};
use std::{
    fs::File,
    io::Read,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_CACHED_VIEWS: usize = 12;
// Bound all parsing/raster work to one background task at a time. Each view
// coalesces camera changes; stale frames never overwrite newer requested views.
static RASTER_GATE: Mutex<()> = Mutex::new(());

#[derive(Default)]
pub(super) struct ModelViews(Mutex<BTreeMap<NodeId, Entity<ModelView>>>);
impl ModelViews {
    pub(super) fn retain(&self, graph: &gpui_flow_core::Graph) {
        self.0.lock().unwrap().retain(|id, _| {
            graph
                .node(id)
                .is_some_and(|node| matches!(node.kind.as_str(), "model" | "generate_model"))
        });
    }
    pub(super) fn render(&self, node: &Node, path: &std::path::Path, cx: &mut App) -> AnyElement {
        let camera = match node.data.get("camera") {
            Some(value) => match serde_json::from_value(value.clone()) {
                Ok(state) => Some(state),
                Err(_) => return message("模型相机数据无效，请重置视角"),
            },
            None => None,
        };
        let request = Request {
            path: path.to_owned(),
            camera,
        };
        let mut views = self.0.lock().unwrap();
        if !views.contains_key(&node.id) {
            if views.len() >= MAX_CACHED_VIEWS {
                return message("最多同时预览 12 个模型；删除其他模型可释放预览");
            }
            views.insert(node.id.clone(), cx.new(ModelView::new));
        }
        let view = views.get(&node.id).unwrap().clone();
        drop(views);
        view.update(cx, |view, cx| view.request(request, cx));
        view.into_any_element()
    }
}
fn message(text: impl Into<SharedString>) -> AnyElement {
    div()
        .size_full()
        .bg(rgb(0x171b23))
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .text_xs()
        .text_color(rgb(0xb4bdca))
        .child(text.into())
        .into_any_element()
}

#[derive(Clone, PartialEq)]
struct Request {
    path: PathBuf,
    camera: Option<OrbitCameraState>,
}
struct ModelView {
    cancelled: Arc<AtomicBool>,
    desired: Option<Request>,
    completed: Option<Request>,
    running: bool,
    mesh: Option<(PathBuf, Arc<PreviewMesh>)>,
    image: Option<Arc<RenderImage>>,
    status: String,
    error: Option<String>,
}
impl ModelView {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| {
            this.cancelled.store(true, Ordering::Release);
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            desired: None,
            completed: None,
            running: false,
            mesh: None,
            image: None,
            status: "正在读取本地 GLB…".into(),
            error: None,
        }
    }
    fn request(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.desired.as_ref() == Some(&request) {
            return;
        }
        self.desired = Some(request);
        self.start(cx);
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running || self.desired == self.completed {
            return;
        }
        let Some(request) = self.desired.clone() else {
            return;
        };
        let mesh = self
            .mesh
            .as_ref()
            .filter(|(path, _)| path == &request.path)
            .map(|(_, mesh)| mesh.clone());
        self.running = true;
        self.error = None;
        let work = request.clone();
        let cancelled = self.cancelled.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if cancelled.load(Ordering::Acquire) {
                        return Err("预览已取消".into());
                    }
                    let _gate = RASTER_GATE.lock().map_err(|_| "预览任务异常".to_string())?;
                    if cancelled.load(Ordering::Acquire) {
                        return Err("预览已取消".into());
                    }
                    let mesh = match mesh {
                        Some(mesh) => mesh,
                        None => {
                            let metadata =
                                std::fs::symlink_metadata(&work.path).map_err(|e| e.to_string())?;
                            if !metadata.is_file()
                                || metadata.len() > model_preview::MAX_GLB_BYTES as u64
                            {
                                return Err("模型不是普通文件或超过 32 MiB".into());
                            }
                            let mut bytes = Vec::new();
                            File::open(&work.path)
                                .map_err(|e| e.to_string())?
                                .take(model_preview::MAX_GLB_BYTES as u64 + 1)
                                .read_to_end(&mut bytes)
                                .map_err(|e| e.to_string())?;
                            Arc::new(model_preview::parse_glb(&bytes).map_err(|e| e.to_string())?)
                        }
                    };
                    if cancelled.load(Ordering::Acquire) {
                        return Err("预览已取消".into());
                    }
                    let camera = match work.camera {
                        Some(state) => {
                            OrbitCamera::from_state(&mesh, state).map_err(|e| e.to_string())?
                        }
                        None => OrbitCamera::fit(&mesh),
                    };
                    let pixels = model_preview::render(&mesh, &camera, 512, 384)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((mesh, pixels.into_bgra8()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.running = false;
                // Keep validated geometry even when an intervening orbit made
                // this frame stale, then raster only the latest requested view.
                if let Ok((mesh, _)) = &result {
                    this.mesh = Some((request.path.clone(), mesh.clone()));
                }
                if this.desired.as_ref() == Some(&request) {
                    this.completed = Some(request);
                    if let Some(old) = this.image.take() {
                        cx.drop_image(old, None);
                    }
                    match result {
                        Ok((mesh, pixels)) => {
                            let buffer = image::RgbaImage::from_raw(512, 384, pixels)
                                .expect("bounded raster dimensions");
                            this.image =
                                Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])));
                            this.status = format!(
                                "CPU · {} 顶点 · {} 三角面",
                                mesh.vertex_count(),
                                mesh.triangle_count()
                            );
                            this.error = None;
                        }
                        Err(error) => this.error = Some(error),
                    }
                }
                this.start(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for ModelView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().size_full().relative().bg(rgb(0x171b23));
        if let Some(error) = &self.error {
            body = body.child(message(format!("模型预览失败：{error}")));
        } else if let Some(image) = &self.image {
            body = body.child(
                img(image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            );
        } else {
            body = body.child(message("正在读取本地 GLB…"));
        }
        body.child(
            div()
                .absolute()
                .left_2()
                .bottom_2()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgba(0x171b23cc))
                .text_size(px(10.))
                .text_color(rgb(0xb4bdca))
                .child(if self.running {
                    "正在更新视角…".into()
                } else {
                    self.status.clone()
                }),
        )
    }
}

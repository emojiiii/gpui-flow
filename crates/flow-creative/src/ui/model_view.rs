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
pub(super) struct ModelViews(Mutex<ModelCache>);

#[derive(Default)]
struct ModelCache {
    views: BTreeMap<NodeId, Entity<ModelView>>,
    camera_override: Option<CameraOverride>,
}

struct CameraOverride {
    node: NodeId,
    asset_key: String,
    state: OrbitCameraState,
}

impl CameraOverride {
    fn matches(&self, node: &Node) -> bool {
        self.node == node.id && model_asset(node) == Some(self.asset_key.as_str())
    }
}

fn model_asset(node: &Node) -> Option<&str> {
    matches!(node.kind.as_str(), "model" | "generate_model")
        .then(|| node.data.get("asset").and_then(Value::as_str))
        .flatten()
        .filter(|asset| !asset.is_empty())
}

fn requested_camera(
    node: &Node,
    camera_override: Option<&CameraOverride>,
) -> Result<Option<OrbitCameraState>, serde_json::Error> {
    if let Some(draft) = camera_override.filter(|draft| draft.matches(node)) {
        return Ok(Some(draft.state));
    }
    node.data
        .get("camera")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
}

impl ModelViews {
    /// Draft controls belong only to this presentation cache, never the graph.
    /// The caller notifies FlowCanvas after setting or clearing the draft.
    pub(super) fn set_camera_override(
        &self,
        node: NodeId,
        asset_key: String,
        state: OrbitCameraState,
    ) {
        self.0.lock().unwrap().camera_override = Some(CameraOverride {
            node,
            asset_key,
            state,
        });
    }

    pub(super) fn clear_camera_override(&self) {
        self.0.lock().unwrap().camera_override = None;
    }

    pub(super) fn retain(&self, graph: &gpui_flow_core::Graph, cx: &mut App) {
        let hidden = gpui_flow_core::hidden_node_ids(graph);
        let mut cache = self.0.lock().unwrap();
        if cache.camera_override.as_ref().is_some_and(|draft| {
            hidden.contains(&draft.node)
                || !graph
                    .node(&draft.node)
                    .is_some_and(|node| draft.matches(node))
        }) {
            cache.camera_override = None;
        }
        let mut retained = Vec::new();
        let mut removed = Vec::new();
        cache.views.retain(|id, view| {
            if !hidden.contains(id)
                && let Some(asset_key) = graph.node(id).and_then(model_asset)
            {
                retained.push((view.clone(), asset_key.to_owned()));
                true
            } else {
                removed.push(view.clone());
                false
            }
        });
        drop(cache);
        for view in removed {
            view.update(cx, |view, cx| {
                view.clear(cx);
                cx.notify();
            });
        }
        for (view, asset_key) in retained {
            view.update(cx, |view, cx| {
                if view
                    .desired
                    .as_ref()
                    .is_some_and(|request| request.source.asset_key != asset_key)
                {
                    view.clear(cx);
                    cx.notify();
                }
            });
        }
    }

    pub(super) fn render(&self, node: &Node, path: &std::path::Path, cx: &mut App) -> AnyElement {
        let mut cache = self.0.lock().unwrap();
        if cache
            .camera_override
            .as_ref()
            .is_some_and(|draft| draft.node == node.id && !draft.matches(node))
        {
            cache.camera_override = None;
        }
        let camera = requested_camera(node, cache.camera_override.as_ref());
        let Some(asset_key) = model_asset(node) else {
            let view = cache.views.remove(&node.id);
            drop(cache);
            if let Some(view) = view {
                view.update(cx, |view, cx| view.clear(cx));
            }
            return message("模型没有本地素材");
        };
        let camera = match camera {
            Ok(camera) => camera,
            Err(_) => {
                let view = cache.views.get(&node.id).cloned();
                drop(cache);
                if let Some(view) = view {
                    view.update(cx, |view, cx| view.clear(cx));
                }
                return message("模型相机数据无效，请重置视角");
            }
        };
        let request = Request {
            source: Source {
                asset_key: asset_key.to_owned(),
                path: path.to_owned(),
            },
            camera,
        };
        if !cache.views.contains_key(&node.id) {
            if cache.views.len() >= MAX_CACHED_VIEWS {
                return message("最多同时预览 12 个模型；删除其他模型可释放预览");
            }
            cache.views.insert(node.id.clone(), cx.new(ModelView::new));
        }
        let view = cache.views.get(&node.id).unwrap().clone();
        drop(cache);
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
struct Source {
    asset_key: String,
    path: PathBuf,
}

#[derive(Clone, PartialEq)]
struct Request {
    source: Source,
    camera: Option<OrbitCameraState>,
}
struct ModelView {
    desired: Option<Request>,
    completed: Option<Request>,
    running: Option<Arc<AtomicBool>>,
    mesh: Option<(Source, Arc<PreviewMesh>)>,
    image: Option<Arc<RenderImage>>,
    status: String,
    error: Option<String>,
}
impl ModelView {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.clear(cx)).detach();
        Self {
            desired: None,
            completed: None,
            running: None,
            mesh: None,
            image: None,
            status: "正在读取本地 GLB…".into(),
            error: None,
        }
    }

    fn clear(&mut self, cx: &mut App) {
        if let Some(cancelled) = &self.running {
            cancelled.store(true, Ordering::Release);
        }
        // Retain the running slot until its completion callback. A replacement
        // source cannot enqueue another job while the old job is still exiting.
        self.desired = None;
        self.completed = None;
        self.mesh = None;
        if let Some(image) = self.image.take() {
            cx.drop_image(image, None);
        }
        self.error = None;
        self.status = "正在读取本地 GLB…".into();
    }

    fn request(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.desired.as_ref() == Some(&request) {
            return;
        }
        if self
            .desired
            .as_ref()
            .is_some_and(|previous| previous.source != request.source)
        {
            self.clear(cx);
        }
        self.desired = Some(request);
        self.start(cx);
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() || self.desired == self.completed {
            return;
        }
        let Some(request) = self.desired.clone() else {
            return;
        };
        let mesh = self
            .mesh
            .as_ref()
            .filter(|(source, _)| source == &request.source)
            .map(|(_, mesh)| mesh.clone());
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running = Some(cancelled.clone());
        self.error = None;
        let work = request.clone();
        let worker_cancelled = cancelled.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let cancelled = worker_cancelled;
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
                            let metadata = std::fs::symlink_metadata(&work.source.path)
                                .map_err(|e| e.to_string())?;
                            if !metadata.is_file()
                                || metadata.len() > model_preview::MAX_GLB_BYTES as u64
                            {
                                return Err("模型不是普通文件或超过 32 MiB".into());
                            }
                            let mut bytes = Vec::new();
                            File::open(&work.source.path)
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
                    let pixels =
                        model_preview::render_cancellable(&mesh, &camera, 512, 384, &cancelled)
                            .map_err(|e| e.to_string())?;
                    Ok::<_, String>((mesh, pixels.into_bgra8()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                // Keep validated geometry even when an intervening orbit made
                // this frame stale. A source change or released view must never
                // reacquire that old geometry, even if its key changes back.
                if !cancelled.load(Ordering::Acquire)
                    && this
                        .desired
                        .as_ref()
                        .is_some_and(|desired| desired.source == request.source)
                    && let Ok((mesh, _)) = &result
                {
                    this.mesh = Some((request.source.clone(), mesh.clone()));
                }
                if !cancelled.load(Ordering::Acquire) && this.desired.as_ref() == Some(&request) {
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
                                "CPU · {} 顶点 · {} 三角面 · {} 贴图{}",
                                mesh.vertex_count(),
                                mesh.triangle_count(),
                                mesh.texture_count(),
                                if mesh.uses_mip_filter_fallback() {
                                    " · 基础级采样"
                                } else {
                                    ""
                                }
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
                    .absolute()
                    .inset_0()
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
                .child(if self.running.is_some() {
                    "正在更新视角…".into()
                } else {
                    self.status.clone()
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use serde_json::json;

    fn camera(yaw: f32) -> OrbitCameraState {
        OrbitCameraState {
            yaw_radians: yaw,
            pitch_radians: 0.25,
            distance_factor: 3.2,
        }
    }

    fn resolve(views: &ModelViews, node: &Node) -> Option<OrbitCameraState> {
        requested_camera(node, views.0.lock().unwrap().camera_override.as_ref()).unwrap()
    }

    #[test]
    fn draft_camera_is_scoped_to_both_model_node_and_current_asset() {
        let views = ModelViews::default();
        let saved = camera(0.5);
        let draft = camera(1.5);
        let node = Node::new("model-a", "Model", WorldPoint::ZERO)
            .with_kind("model")
            .with_data(json!({"asset": "assets/a.glb", "camera": saved}));
        let original = node.clone();
        views.set_camera_override(node.id.clone(), "assets/a.glb".into(), draft);
        assert_eq!(resolve(&views, &node), Some(draft));

        let mut other = node.clone();
        other.id = "model-b".into();
        assert_eq!(resolve(&views, &other), Some(saved));

        let mut replaced = node.clone();
        replaced.data["asset"] = json!("assets/b.glb");
        assert_eq!(resolve(&views, &replaced), Some(saved));
        replaced.data["asset"] = json!("");
        assert_eq!(resolve(&views, &replaced), Some(saved));

        let mut changed_kind = node.clone();
        changed_kind.kind = "image".into();
        assert_eq!(resolve(&views, &changed_kind), Some(saved));
        assert_eq!(node, original);
    }

    #[test]
    fn replacing_and_clearing_a_draft_restores_saved_or_fit_camera() {
        for saved in [None, Some(camera(0.5))] {
            let views = ModelViews::default();
            let mut node = Node::new("model-a", "Model", WorldPoint::ZERO)
                .with_kind("generate_model")
                .with_data(json!({"asset": "assets/a.glb"}));
            if let Some(saved) = saved {
                node.data["camera"] = serde_json::to_value(saved).unwrap();
            }
            let original = node.clone();
            views.set_camera_override(node.id.clone(), "assets/a.glb".into(), camera(1.));
            views.set_camera_override(node.id.clone(), "assets/a.glb".into(), camera(2.));
            assert_eq!(resolve(&views, &node), Some(camera(2.)));
            views.clear_camera_override();
            assert_eq!(resolve(&views, &node), saved);
            assert_eq!(node, original);
        }
    }
}

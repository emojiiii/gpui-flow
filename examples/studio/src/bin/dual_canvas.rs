//! Two independently owned canvas components in one native window.
//! Run with: cargo run --locked -p flow-studio --bin dual_canvas
//! Use --scene for retained scene canvases and --verify for a one-shot native check.

use gpui::{prelude::*, *};
use gpui_flow::{
    FlowTheme, builtin_node,
    components::{ControlButton, FlowCanvas, LeaferCompatibilityProfile, SceneView},
    core::{
        AGENT_EDIT_SCHEMA_VERSION, AgentEditErrorCode, AgentEditRequest, AgentOperation,
        FlowDocument, NodeId, Point as FlowPoint,
    },
    interaction::{CanvasCommand, scene::SceneCommand},
    scene::{Affine, Color, DisplayId, DisplayObject, Scene, Style, display},
};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    sync::Arc,
};

const NODE_ID: &str = "shared-node";
const GROUP_ID: &str = "group";
const FRAME_ID: &str = "frame";
const PANE_TITLES: [&str; 2] = ["左画布", "右画布"];

#[derive(Clone)]
enum Canvas {
    Flow(Entity<FlowCanvas>),
    Scene(Entity<SceneView>),
}

#[derive(Clone, Copy)]
enum Edit {
    Select,
    Delete,
    Undo,
}

type Verification = Rc<RefCell<Option<std::thread::Result<()>>>>;

impl Canvas {
    fn new(scene: bool, index: usize, cx: &mut App) -> Self {
        if scene {
            let accent = [0x53d2bf, 0x8f9ff5][index];
            let mut document = Scene::new();
            document
                .insert(
                    DisplayObject::new(GROUP_ID, display::Group::new())
                        .with_transform(Affine::translation(36., 65.)),
                    None,
                    None,
                )
                .unwrap();
            document
                .insert(
                    DisplayObject::new(FRAME_ID, display::Frame::new(340., 210.).rounded(16.))
                        .with_style(Style::fill(Color::hex(0x202c3d))),
                    Some(GROUP_ID.into()),
                    None,
                )
                .unwrap();
            document
                .insert(
                    DisplayObject::new(NODE_ID, display::Rect::new(180., 104.).rounded(12.))
                        .with_transform(Affine::translation(30., 30.))
                        .with_style(Style::fill(Color::hex(accent))),
                    Some(FRAME_ID.into()),
                    None,
                )
                .unwrap();
            Self::Scene(
                SceneView::mount_with_profile(document, LeaferCompatibilityProfile, cx)
                    .expect("valid scene document"),
            )
        } else {
            let mut document = FlowDocument::default();
            let mut node = builtin_node("text", NODE_ID, FlowPoint::new(36., 65.));
            node.title = PANE_TITLES[index].into();
            node.data["text"] = NODE_ID.into();
            document.graph.add_node(node).unwrap();
            Self::Flow(FlowCanvas::mount(document, cx).expect("valid flow document"))
        }
    }

    fn edit(&self, edit: Edit, cx: &mut App) -> Result<(), String> {
        match self {
            Self::Flow(canvas) => canvas
                .update(cx, |view, cx| match edit {
                    Edit::Select => match view.find_id(&NodeId::from(NODE_ID)) {
                        Some(handle) => view.select(&handle, cx),
                        None => Ok(()),
                    },
                    Edit::Delete => view.execute_command(CanvasCommand::DeleteSelection, cx),
                    Edit::Undo => view.execute_command(CanvasCommand::Undo, cx),
                })
                .map_err(|error| error.to_string()),
            Self::Scene(canvas) => canvas
                .update(cx, |view, cx| match edit {
                    Edit::Select => match view.find_id(&DisplayId::from(NODE_ID)) {
                        Some(handle) => view.select(handle, cx),
                        None => Ok(()),
                    },
                    Edit::Delete => view.execute_command(SceneCommand::DeleteSelection, cx),
                    Edit::Undo => view.execute_command(SceneCommand::Undo, cx),
                })
                .map_err(|error| error.to_string()),
        }
    }

    fn status(&self, cx: &App) -> (usize, bool, bool) {
        match self {
            Self::Flow(canvas) => {
                let view = canvas.read(cx);
                (
                    view.selection().len(),
                    view.find_id(&NODE_ID.into()).is_some(),
                    view.can_undo(),
                )
            }
            Self::Scene(canvas) => {
                let view = canvas.read(cx);
                (
                    view.selected_elements().count(),
                    view.find_id(&NODE_ID.into()).is_some(),
                    view.can_undo(),
                )
            }
        }
    }

    fn element(&self) -> AnyElement {
        match self {
            Self::Flow(canvas) => canvas.clone().into_any_element(),
            Self::Scene(canvas) => canvas.clone().into_any_element(),
        }
    }
}

struct DualCanvas {
    panes: [Canvas; 2],
    errors: [Option<String>; 2],
    verification: Option<Verification>,
    _subscriptions: [Subscription; 2],
}

impl DualCanvas {
    fn new(panes: [Canvas; 2], verification: Option<Verification>, cx: &mut Context<Self>) -> Self {
        let subscriptions = panes.each_ref().map(|pane| match pane {
            Canvas::Flow(canvas) => cx.subscribe(canvas, |_, _, _, cx| cx.notify()),
            Canvas::Scene(canvas) => cx.subscribe(canvas, |_, _, _, cx| cx.notify()),
        });
        Self {
            panes,
            errors: [None, None],
            verification,
            _subscriptions: subscriptions,
        }
    }

    fn pane(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let (selected, has_node, can_undo) = self.panes[index].status(cx);
        let buttons = [
            ("select", "选中节点", Edit::Select, !has_node),
            ("delete", "删除选中", Edit::Delete, selected == 0),
            ("undo", "撤销", Edit::Undo, !can_undo),
        ];
        div()
            .id(("canvas-pane", index))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme.border))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .bg(rgb(theme.panel))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(PANE_TITLES[index]),
                            )
                            .child(div().text_sm().text_color(rgb(theme.muted)).child(format!(
                                "{} · 选区 {selected}",
                                if has_node { NODE_ID } else { "节点已删除" },
                            ))),
                    )
                    .child(div().flex().gap_2().children(buttons.map(
                        |(id, label, edit, disabled)| {
                            ControlButton::new(id, label)
                                .disabled(disabled)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.errors[index] = this.panes[index].edit(edit, cx).err();
                                    cx.notify();
                                }))
                        },
                    )))
                    .when_some(self.errors[index].clone(), |panel, message| {
                        panel.child(div().text_sm().text_color(rgb(0xfda4af)).child(message))
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.panes[index].element()),
            )
    }
}

impl Render for DualCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(outcome) = self.verification.take() {
            let panes = self.panes.clone();
            window.on_next_frame(move |window, cx| {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let kind = match &panes {
                        [Canvas::Flow(left), Canvas::Flow(right)] => {
                            verify_flow(left, right, window, cx);
                            verify_agent_edit(left, right, cx);
                            println!("FlowCanvas: agent batch policy, isolation and undo verified");
                            "FlowCanvas"
                        }
                        [Canvas::Scene(left), Canvas::Scene(right)] => {
                            verify_scene(left, right, window, cx);
                            "SceneView"
                        }
                        _ => unreachable!("both panes use the selected canvas kind"),
                    };
                    println!(
                        "{kind}: same-window selection, focused undo and ID isolation verified"
                    );
                }));
                *outcome.borrow_mut() = Some(result);
                cx.quit();
            });
        }
        let theme = FlowTheme::default();
        div()
            .size_full()
            .flex()
            .gap_3()
            .p_3()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .child(self.pane(0, cx))
            .child(self.pane(1, cx))
    }
}

fn verify_flow(
    left: &Entity<FlowCanvas>,
    right: &Entity<FlowCanvas>,
    window: &mut Window,
    cx: &mut App,
) {
    let id = NodeId::from(NODE_ID);
    let left_handle = left.read(cx).find_id(&id).unwrap();
    let right_handle = right.read(cx).find_id(&id).unwrap();
    assert_eq!(left_handle.id(), right_handle.id());
    assert_eq!(
        left.read(cx).node(&left_handle).unwrap().title,
        PANE_TITLES[0]
    );
    assert_eq!(
        right.read(cx).node(&right_handle).unwrap().title,
        PANE_TITLES[1]
    );
    assert!(right.read(cx).node(&left_handle).is_err());
    right.update(cx, |view, cx| {
        assert!(view.select(&left_handle, cx).is_err());
        assert!(view.remove(&left_handle, cx).is_err());
    });
    left.update(cx, |view, cx| {
        let duplicate = builtin_node("text", NODE_ID, FlowPoint::ZERO);
        assert!(view.add(duplicate, cx).is_err());
        let added = builtin_node("text", "new-node", FlowPoint::ZERO);
        let added = view.add(added, cx).unwrap();
        assert_eq!(view.node(&added).unwrap().id, NodeId::from("new-node"));
        assert_eq!(view.children().count(), 2);
        view.execute_command(CanvasCommand::Undo, cx).unwrap();
        assert!(view.find_id(&"new-node".into()).is_none());
    });
    assert!(right.read(cx).find_id(&"new-node".into()).is_none());
    assert!(!right.read(cx).can_undo());
    left.update(cx, |view, cx| view.select(&left_handle, cx).unwrap());
    assert!(left.read(cx).selection().nodes.contains(&id));
    assert!(right.read(cx).selection().is_empty());
    right.update(cx, |view, cx| view.select(&right_handle, cx).unwrap());
    left.update(cx, |view, cx| view.remove(&left_handle, cx).unwrap());
    assert!(left.read(cx).find_id(&id).is_none());
    assert!(left.read(cx).can_undo());
    assert!(right.read(cx).selection().nodes.contains(&id));
    assert!(right.read(cx).find_id(&id).is_some());
    assert!(!right.read(cx).can_undo());

    left.focus_handle(cx).focus(window, cx);
    window.dispatch_keystroke(Keystroke::parse("ctrl-z").unwrap(), cx);
    assert!(left.read(cx).find_id(&id).is_some());
    assert!(left.read(cx).can_redo());
    assert!(!right.read(cx).can_redo());
    assert!(right.read(cx).selection().nodes.contains(&id));
    assert_eq!(right.read(cx).children().count(), 1);
}

fn verify_agent_edit(left: &Entity<FlowCanvas>, right: &Entity<FlowCanvas>, cx: &mut App) {
    let ids = [NodeId::from("agent-first"), NodeId::from("agent-second")];
    let revision = left.read(cx).agent_snapshot().revision;
    let request = AgentEditRequest {
        schema_version: AGENT_EDIT_SCHEMA_VERSION,
        expected_revision: revision.clone(),
        operations: ids
            .iter()
            .map(|id| AgentOperation::AddNode {
                id: id.clone(),
                title: "Agent node".into(),
                position: FlowPoint::ZERO,
                kind: None,
                size: None,
                ports: vec![],
                data: serde_json::Value::Null,
            })
            .collect(),
    };
    right.update(cx, |view, cx| {
        let error = view.apply_agent_edit(request.clone(), cx).unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::StaleRevision);
        assert!(ids.iter().all(|id| view.find_id(id).is_none()));
        assert!(!view.can_undo());
    });
    left.update(cx, |view, cx| {
        view.set_editable(false, cx);
        let error = view.apply_agent_edit(request.clone(), cx).unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
        view.set_editable(true, cx);

        view.execute_command(CanvasCommand::CapturePointer { pointer_id: 1 }, cx)
            .unwrap();
        let error = view.apply_agent_edit(request.clone(), cx).unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::Busy);
        view.execute_command(CanvasCommand::ReleasePointer, cx)
            .unwrap();

        let blocked_id = ids[1].clone();
        view.set_document_validator(Some(Arc::new(move |document| {
            if document.graph.node(&blocked_id).is_some() {
                return Err("blocked by host document policy".into());
            }
            Ok(())
        })))
        .unwrap();
        let error = view.apply_agent_edit(request.clone(), cx).unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
        assert_eq!(view.agent_snapshot().revision, revision);
        assert!(ids.iter().all(|id| view.find_id(id).is_none()));
        assert!(!view.can_undo());
        view.set_document_validator(None).unwrap();

        let receipt = view.apply_agent_edit(request, cx).unwrap();
        assert_ne!(receipt.revision, revision);
        assert_eq!(receipt.changes.nodes.inserted, ids);
        assert_eq!(view.agent_snapshot().revision, receipt.revision);
        assert!(ids.iter().all(|id| view.find_id(id).is_some()));
        view.execute_command(CanvasCommand::Undo, cx).unwrap();
        assert!(ids.iter().all(|id| view.find_id(id).is_none()));
        assert!(!view.can_undo());

        let image_id = NodeId::new("agent-asset");
        let asset = |source: &str| AgentEditRequest {
            schema_version: AGENT_EDIT_SCHEMA_VERSION,
            expected_revision: view.agent_snapshot().revision,
            operations: vec![AgentOperation::AddNode {
                id: image_id.clone(),
                title: "Agent image".into(),
                position: FlowPoint::ZERO,
                kind: Some("image".into()),
                size: None,
                ports: vec![],
                data: serde_json::json!({"source": source}),
            }],
        };
        let direct_path = asset("private/cover.png");
        let asset_id = asset("cover");
        view.set_agent_assets([("cover".into(), "private/cover.png".into())].into());
        let error = view.apply_agent_edit(direct_path, cx).unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
        assert!(!view.can_undo());
        view.apply_agent_edit(asset_id, cx).unwrap();
        assert_eq!(
            view.editor.graph().nodes[&image_id].data["source"],
            "private/cover.png",
        );
        assert_eq!(
            view.agent_snapshot().graph.nodes[&image_id].data["source"],
            "cover"
        );
        view.execute_command(CanvasCommand::Undo, cx).unwrap();
        assert!(view.find_id(&image_id).is_none());
    });
}

fn verify_scene(
    left: &Entity<SceneView>,
    right: &Entity<SceneView>,
    window: &mut Window,
    cx: &mut App,
) {
    let id = DisplayId::from(NODE_ID);
    let left_handle = left.read(cx).find_id(&id).unwrap();
    let right_handle = right.read(cx).find_id(&id).unwrap();
    assert_ne!(left_handle, right_handle);
    assert_eq!(left.read(cx).object(left_handle).unwrap().id, id);
    assert_eq!(right.read(cx).object(right_handle).unwrap().id, id);
    let left_group = left.read(cx).find_id(&GROUP_ID.into()).unwrap();
    let left_frame = left.read(cx).find_id(&FRAME_ID.into()).unwrap();
    assert_eq!(left.read(cx).children(None).unwrap(), [left_group]);
    assert_eq!(
        left.read(cx).children(Some(left_group)).unwrap(),
        [left_frame]
    );
    assert_eq!(
        left.read(cx).children(Some(left_frame)).unwrap(),
        [left_handle]
    );
    assert_eq!(left.read(cx).parent(left_handle).unwrap(), Some(left_frame));

    right.update(cx, |view, cx| {
        assert!(view.object(left_handle).is_err());
        assert!(view.children(Some(left_frame)).is_err());
        assert!(view.select(left_handle, cx).is_err());
        assert!(view.remove(left_handle, cx).is_err());
        let child = DisplayObject::new("new-child", display::Rect::new(10., 10.));
        assert!(view.add_child(left_frame, child, cx).is_err());
        assert!(view.find_id(&"new-child".into()).is_none());
    });
    left.update(cx, |view, cx| {
        let child = DisplayObject::new("new-child", display::Rect::new(10., 10.));
        assert!(view.add_child(left_handle, child, cx).is_err());
        let child = DisplayObject::new("new-child", display::Rect::new(10., 10.));
        let child = view.add_child(left_frame, child, cx).unwrap();
        assert_eq!(view.parent(child).unwrap(), Some(left_frame));
        view.execute_command(SceneCommand::Undo, cx).unwrap();
        assert!(view.find_id(&"new-child".into()).is_none());
    });

    left.update(cx, |view, cx| view.select(left_handle, cx).unwrap());
    assert_eq!(
        left.read(cx).selected_elements().collect::<Vec<_>>(),
        [left_handle]
    );
    assert_eq!(right.read(cx).selected_elements().count(), 0);
    right.update(cx, |view, cx| view.select(right_handle, cx).unwrap());
    left.update(cx, |view, cx| view.remove(left_handle, cx).unwrap());
    assert!(left.read(cx).find_id(&id).is_none());
    assert!(left.read(cx).can_undo());
    assert_eq!(
        right.read(cx).selected_elements().collect::<Vec<_>>(),
        [right_handle]
    );
    assert_eq!(right.read(cx).find_id(&id), Some(right_handle));
    assert!(!right.read(cx).can_undo());

    left.focus_handle(cx).focus(window, cx);
    window.dispatch_keystroke(Keystroke::parse("ctrl-z").unwrap(), cx);
    let restored = left.read(cx).find_id(&id).unwrap();
    assert_eq!(left.read(cx).parent(restored).unwrap(), Some(left_frame));
    assert!(left.read(cx).can_redo());
    assert!(!right.read(cx).can_redo());
    assert_eq!(
        right.read(cx).selected_elements().collect::<Vec<_>>(),
        [right_handle]
    );
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let scene = args.iter().any(|arg| arg == "--scene");
    let verification = args
        .iter()
        .any(|arg| arg == "--verify")
        .then(|| Rc::new(RefCell::new(None)));
    let app_outcome = verification.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1100.), px(630.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    window.set_window_title(if scene {
                        "双图形画布"
                    } else {
                        "双流程画布"
                    });
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    let panes = std::array::from_fn(|index| Canvas::new(scene, index, cx));
                    cx.new(|cx| DualCanvas::new(panes, app_outcome, cx))
                },
            )
            .expect("open dual-canvas window");
            cx.activate(true);
        });
    if let Some(outcome) = verification {
        let result = outcome
            .borrow_mut()
            .take()
            .expect("native window did not reach the verification frame");
        if let Err(error) = result {
            resume_unwind(error);
        }
    }
}

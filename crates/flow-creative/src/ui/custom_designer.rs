//! A declarative editor for exact, typed custom-node contracts. Executor IDs
//! name registered host code; editing or creating a definition never runs it.
use super::*;
use crate::custom_nodes::{
    CustomNodeDefinition, CustomPortDefinition, CustomPortType, MAX_CUSTOM_PORTS,
};

#[derive(Clone)]
pub(super) enum CustomDesignerAction {
    Close,
    AddInput,
    AddOutput,
    RemovePort(String),
    ToggleType(String),
    SetType(String, CustomPortType),
    SetExecutor(String),
    Create,
}

struct PortDraft {
    id: String,
    label: Entity<TextareaState>,
    data_type: CustomPortType,
    _subscription: Subscription,
}

pub(super) struct CustomDesigner {
    kind: String,
    name: Entity<TextareaState>,
    executor: Entity<TextareaState>,
    inputs: Vec<PortDraft>,
    outputs: Vec<PortDraft>,
    next_port: u64,
    type_menu: Option<String>,
    focus: FocusHandle,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

fn draft_field(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<CreativeCanvas>,
) -> (Entity<TextareaState>, Subscription) {
    let field = cx.new(|cx| {
        let mut field = TextareaState::new(window, cx)
            .placeholder(placeholder)
            .context_menu(false);
        field.set_value(value.to_string(), window, cx);
        field
    });
    let subscription = cx.subscribe(&field, |this, _, event, cx| {
        if matches!(event, InputEvent::Change) {
            if let Some(designer) = &mut this.custom_designer {
                designer.error = None;
            }
            cx.notify();
        }
    });
    (field, subscription)
}

impl CustomDesigner {
    fn new(kind: String, window: &mut Window, cx: &mut Context<CreativeCanvas>) -> Self {
        let (name, name_subscription) = draft_field("自定义节点", "节点名称", window, cx);
        let (executor, executor_subscription) =
            draft_field("local.pass_through", "已注册的执行接口 ID", window, cx);
        let mut this = Self {
            kind,
            name,
            executor,
            inputs: Vec::new(),
            outputs: Vec::new(),
            next_port: 1,
            type_menu: None,
            focus: cx.focus_handle(),
            error: None,
            _subscriptions: vec![name_subscription, executor_subscription],
        };
        this.add_port(true, window, cx);
        this.add_port(false, window, cx);
        this
    }

    fn add_port(&mut self, input: bool, window: &mut Window, cx: &mut Context<CreativeCanvas>) {
        if self.inputs.len() + self.outputs.len() >= MAX_CUSTOM_PORTS {
            return;
        }
        let serial = self.next_port;
        self.next_port += 1;
        let ports = if input {
            &mut self.inputs
        } else {
            &mut self.outputs
        };
        let direction = if input { "输入" } else { "输出" };
        let (label, subscription) = draft_field(
            &format!("{direction} {}", ports.len() + 1),
            "端口名称",
            window,
            cx,
        );
        ports.push(PortDraft {
            // IDs do not depend on labels or positions and are never reused.
            id: format!("{}_{serial}", if input { "in" } else { "out" }),
            label,
            data_type: CustomPortType::Named("image".into()),
            _subscription: subscription,
        });
        self.error = None;
    }

    fn definition(&self, cx: &App) -> Result<CustomNodeDefinition, String> {
        let title = self.name.read(cx).value().trim().to_string();
        if title.is_empty() || title.len() > 512 || title.contains('\0') {
            return Err("请填写节点名称，长度不超过 512 字节".into());
        }
        let executor = self.executor.read(cx).value().trim().to_string();
        if !valid_executor_id(&executor) {
            return Err("接口 ID 须以英文字母开头，仅可包含字母、数字、点、下划线和短横线".into());
        }
        let read_ports = |ports: &[PortDraft]| {
            ports
                .iter()
                .map(|port| {
                    let label = port.label.read(cx).value().trim().to_string();
                    if label.is_empty() || label.len() > 256 || label.contains('\0') {
                        return Err("请填写每个端口的名称，长度不超过 256 字节".to_string());
                    }
                    Ok(CustomPortDefinition {
                        id: port.id.clone(),
                        label,
                        data_type: port.data_type.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        };
        let definition = CustomNodeDefinition {
            schema_version: 1,
            kind: self.kind.clone(),
            title,
            size: WorldSize::new(360., 240.),
            inputs: read_ports(&self.inputs)?,
            outputs: read_ports(&self.outputs)?,
            executor,
            default_settings: serde_json::json!({}),
        };
        definition.validate().map_err(|error| error.to_string())?;
        Ok(definition)
    }

    fn execution_status(&self, registered: &[String], cx: &App) -> (String, bool) {
        let executor = self.executor.read(cx).value();
        let executor = executor.trim();
        if !valid_executor_id(executor) {
            return ("仅填写稳定的接口 ID，不接受 URL 或脚本".into(), false);
        }
        if !registered.iter().any(|id| id == executor) {
            return (
                "尚未接入此接口，节点不可执行；可先保存端口设计".into(),
                false,
            );
        }
        if executor == "local.pass_through" {
            let matches = self.inputs.len() == self.outputs.len()
                && self
                    .inputs
                    .iter()
                    .zip(&self.outputs)
                    .all(|(input, output)| input.data_type == output.data_type);
            if !matches {
                return (
                    "本地透传要求输入与输出数量相等，按顺序每对类型相同；当前配置不可执行".into(),
                    false,
                );
            }
            return (
                "本地透传已接入：按顺序传递同类型数据，连接全部输入后可运行".into(),
                true,
            );
        }
        (
            "接口已在本机注册；实际运行仍需满足该接口的输入与设置要求".into(),
            true,
        )
    }
}

fn valid_executor_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphabetic()
        && !id.split('.').any(str::is_empty)
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

fn port_types() -> [(&'static str, CustomPortType); 9] {
    [
        ("Text", CustomPortType::Text),
        ("Number", CustomPortType::Number),
        ("Bool", CustomPortType::Bool),
        ("Object", CustomPortType::Object),
        ("Image", CustomPortType::Named("image".into())),
        ("Video", CustomPortType::Named("video".into())),
        ("Audio", CustomPortType::Named("audio".into())),
        ("Model", CustomPortType::Named("model".into())),
        ("Mask", CustomPortType::Named("mask".into())),
    ]
}

fn port_type_label(data_type: &CustomPortType) -> &'static str {
    port_types()
        .into_iter()
        .find_map(|(label, kind)| (&kind == data_type).then_some(label))
        .unwrap_or("Object")
}

impl CreativeCanvas {
    /// The host supplies a fresh `custom.*` kind and owns atomic installation.
    pub(super) fn open_custom_designer(
        &mut self,
        kind: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.canvas.read(cx).is_editable()
            || self.canvas.read(cx).interactions.has_capture()
            || self.importing
            || self.saving
        {
            return;
        }
        if self.custom_designer.is_none() {
            self.apply_form_edit(cx);
            self.clear_text_edit();
            self.clear_mask_edit();
            self.asset_picker = None;
            self.show_processing_menu = false;
            self.custom_designer = Some(CustomDesigner::new(kind, window, cx));
        }
        if let Some(designer) = &self.custom_designer {
            designer.name.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn handle_custom_designer(
        &mut self,
        action: CustomDesignerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.custom_designer.is_none() {
            return;
        }
        match action {
            CustomDesignerAction::Close => {
                self.custom_designer = None;
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            CustomDesignerAction::Create => {
                if !self.canvas.read(cx).is_editable() || self.importing || self.saving {
                    return;
                }
                let result = self
                    .custom_designer
                    .as_ref()
                    .expect("designer exists")
                    .definition(cx)
                    .and_then(|definition| self.install_custom_design(definition, cx));
                match result {
                    Ok(()) => {
                        self.custom_designer = None;
                        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    Err(error) => {
                        if let Some(designer) = &mut self.custom_designer {
                            designer.error = Some(error);
                        }
                    }
                }
            }
            CustomDesignerAction::AddInput | CustomDesignerAction::AddOutput => {
                let input = matches!(action, CustomDesignerAction::AddInput);
                let designer = self.custom_designer.as_mut().expect("designer exists");
                designer.add_port(input, window, cx);
            }
            CustomDesignerAction::RemovePort(id) => {
                let designer = self.custom_designer.as_mut().expect("designer exists");
                designer.inputs.retain(|port| port.id != id);
                if designer.outputs.len() > 1 {
                    designer.outputs.retain(|port| port.id != id);
                }
                if designer.type_menu.as_ref() == Some(&id) {
                    designer.type_menu = None;
                }
                designer.error = None;
                // A removed textarea must not leave focus pointing at a dead field.
                designer.focus.focus(window, cx);
            }
            CustomDesignerAction::ToggleType(id) => {
                let designer = self.custom_designer.as_mut().expect("designer exists");
                designer.type_menu = (designer.type_menu.as_ref() != Some(&id)).then_some(id);
            }
            CustomDesignerAction::SetType(id, data_type) => {
                let designer = self.custom_designer.as_mut().expect("designer exists");
                if let Some(port) = designer
                    .inputs
                    .iter_mut()
                    .chain(&mut designer.outputs)
                    .find(|port| port.id == id)
                {
                    port.data_type = data_type;
                }
                designer.type_menu = None;
                designer.error = None;
            }
            CustomDesignerAction::SetExecutor(id) => {
                let designer = self.custom_designer.as_mut().expect("designer exists");
                designer
                    .executor
                    .update(cx, |state, cx| state.set_value(id, window, cx));
                designer.error = None;
            }
        }
        cx.notify();
    }

    /// Full-surface modal scrim with a bounded, independently scrolling panel.
    pub(super) fn custom_designer_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(designer) = &self.custom_designer else {
            return div();
        };
        let mut registered = self.custom_executor_ids();
        registered.sort();
        registered.dedup();
        let total = designer.inputs.len() + designer.outputs.len();
        let definition = designer.definition(cx);
        let problem = designer.error.as_ref().or(definition.as_ref().err());
        let (execution_status, available) = designer.execution_status(&registered, cx);
        let disabled = definition.is_err()
            || !self.canvas.read(cx).is_editable()
            || self.saving
            || self.importing;
        let mut interfaces = div().flex().flex_wrap().gap_1();
        for id in &registered {
            interfaces = interfaces.child(
                self.button(
                    SharedString::from(format!("custom-executor-{id}")),
                    id.clone(),
                    Action::CustomDesigner(CustomDesignerAction::SetExecutor(id.clone())),
                    false,
                    cx,
                )
                .h(px(23.))
                .text_size(px(10.))
                .bg(rgb(0xf1f4f8)),
            );
        }
        if registered.is_empty() {
            interfaces = interfaces.child(label("本机尚未注册任何执行接口"));
        }
        let mut ports = div().flex().flex_col().gap_2();
        for (input, title, drafts) in [
            (true, "输入端口", &designer.inputs),
            (false, "输出端口", &designer.outputs),
        ] {
            ports = ports.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(format!("{title} · {}", drafts.len())),
                    )
                    .child(
                        self.button(
                            if input {
                                "custom-add-input"
                            } else {
                                "custom-add-output"
                            },
                            if input { "+ 输入" } else { "+ 输出" },
                            Action::CustomDesigner(if input {
                                CustomDesignerAction::AddInput
                            } else {
                                CustomDesignerAction::AddOutput
                            }),
                            total >= MAX_CUSTOM_PORTS,
                            cx,
                        )
                        .h(px(25.)),
                    ),
            );
            if drafts.is_empty() {
                ports = ports.child(label("无需输入时可留空"));
            }
            for port in drafts {
                ports = ports.child(self.custom_port_row(
                    port,
                    !input && drafts.len() == 1,
                    designer.type_menu.as_ref() == Some(&port.id),
                    cx,
                ));
            }
        }
        let panel = div()
            .w(px(460.))
            .max_w(relative(1.))
            .h_full()
            .max_h(px(600.))
            .p_4()
            .rounded(px(14.))
            .bg(rgb(0xffffff))
            .shadow_lg()
            .border_1()
            .border_color(rgb(0xe3e6eb))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("设计自定义节点"),
                    )
                    .child(self.button(
                        "custom-close",
                        "关闭",
                        Action::CustomDesigner(CustomDesignerAction::Close),
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(label("节点名称"))
                    .child(
                        Textarea::new(&designer.name)
                            .h(px(34.))
                            .text_size(px(12.))
                            .aria_label("自定义节点名称"),
                    )
                    .child(label("执行接口 ID"))
                    .child(
                        Textarea::new(&designer.executor)
                            .h(px(34.))
                            .text_size(px(12.))
                            .aria_label("执行接口 ID，仅限本机稳定注册 ID"),
                    )
                    .child(label("本机已注册接口 · 点击选择"))
                    .child(
                        div()
                            .id("custom-registered-executors")
                            .max_h(px(52.))
                            .overflow_y_scroll()
                            .child(interfaces),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .text_size(px(11.))
                            .text_color(rgb(if available { 0x54745f } else { 0x9a713d }))
                            .bg(rgb(if available { 0xf0f6f1 } else { 0xfaf5eb }))
                            .child(execution_status),
                    ),
            )
            .child(
                div()
                    .id("custom-port-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pr_1()
                    .child(ports),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(problem, |body, problem| {
                        body.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0xb05e4e))
                                .child(problem.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label(format!(
                                "{total}/{MAX_CUSTOM_PORTS} 个端口 · 至少保留一个输出"
                            )))
                            .child(
                                self.button(
                                    "custom-create",
                                    "创建节点",
                                    Action::CustomDesigner(CustomDesignerAction::Create),
                                    disabled,
                                    cx,
                                )
                                .bg(rgb(if disabled { 0xeff0f3 } else { 0x24262b }))
                                .text_color(rgb(if disabled { 0xaeb1b7 } else { 0xffffff })),
                            ),
                    ),
            );
        div().absolute().top_0().left_0().size_full().child(
            div()
                .id("custom-designer-modal")
                .occlude()
                .track_focus(&designer.focus)
                .size_full()
                .p_4()
                .bg(rgba(0x18202a33))
                .flex()
                .items_center()
                .justify_center()
                // Focus belongs to this modal or its text fields, outside the
                // sibling canvas key context. Do not stop normal key bubbling:
                // GPUI inserts text through its default platform input path.
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_drop(|_: &ExternalPaths, _, cx| cx.stop_propagation())
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.handle_custom_designer(CustomDesignerAction::Close, window, cx);
                    }
                }))
                .child(panel),
        )
    }

    fn custom_port_row(
        &self,
        port: &PortDraft,
        last_output: bool,
        show_types: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = div()
            .p_2()
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(0xe7eaf0))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div().flex_1().min_w_0().child(
                            Textarea::new(&port.label)
                                .h(px(32.))
                                .text_size(px(12.))
                                .aria_label(format!("端口 {} 的名称", port.id)),
                        ),
                    )
                    .child(
                        self.button(
                            SharedString::from(format!("custom-type-{}", port.id)),
                            format!("{} ▾", port_type_label(&port.data_type)),
                            Action::CustomDesigner(CustomDesignerAction::ToggleType(
                                port.id.clone(),
                            )),
                            false,
                            cx,
                        )
                        .min_w(px(79.))
                        .bg(rgb(0xf1f4f8)),
                    )
                    .child(self.button(
                        SharedString::from(format!("custom-remove-{}", port.id)),
                        "删除",
                        Action::CustomDesigner(CustomDesignerAction::RemovePort(port.id.clone())),
                        last_output,
                        cx,
                    )),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(0x9299a4))
                    .child(format!("ID · {}", port.id)),
            );
        if show_types {
            let mut types = div().flex().flex_wrap().gap_1();
            for (label, data_type) in port_types() {
                let selected = port.data_type == data_type;
                types = types.child(
                    self.button(
                        SharedString::from(format!("custom-type-{}-{label}", port.id)),
                        label,
                        Action::CustomDesigner(CustomDesignerAction::SetType(
                            port.id.clone(),
                            data_type,
                        )),
                        false,
                        cx,
                    )
                    .h(px(25.))
                    .text_size(px(11.))
                    .bg(rgb(if selected { 0xe7effd } else { 0xf5f6f8 }))
                    .text_color(rgb(if selected {
                        0x467bc6
                    } else {
                        0x667180
                    })),
                );
            }
            body = body.child(types);
        }
        body
    }
}

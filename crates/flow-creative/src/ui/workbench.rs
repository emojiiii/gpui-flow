use super::*;

/// The native local-first application. Projects are created on disk before a canvas opens.
pub struct CreativeWorkbench {
    store: Option<ProjectStore>,
    projects: Vec<ProjectSummary>,
    thumbnails: BTreeMap<String, PathBuf>,
    name: Entity<TextareaState>,
    current: Option<Entity<CreativeCanvas>>,
    current_subscription: Option<Subscription>,
    notice: Option<String>,
    opening: bool,
}
impl CreativeWorkbench {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (store, notice) = match ProjectStore::new(root) {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(format!("无法打开本地项目目录：{error}"))),
        };
        let mut this = Self {
            store,
            projects: vec![],
            thumbnails: BTreeMap::new(),
            name: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("项目名称，例如：秋日品牌提案")
                    .context_menu(false)
            }),
            current: None,
            current_subscription: None,
            notice,
            opening: false,
        };
        this.refresh(cx);
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            owner
                .update(cx, |this, cx| {
                    this.current
                        .clone()
                        .is_none_or(|view| view.update(cx, |view, cx| view.close_ready(window, cx)))
                })
                .unwrap_or(true)
        });
        this
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let listing = store.list_projects()?;
                    let thumbnails = listing
                        .projects
                        .iter()
                        .take(48)
                        .filter_map(|project| {
                            store
                                .thumbnail_path(&project.metadata.id)
                                .ok()
                                .flatten()
                                .map(|path| (project.metadata.id.clone(), path))
                        })
                        .collect::<BTreeMap<_, _>>();
                    Ok::<_, ProjectError>((listing, thumbnails))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((listing, thumbnails)) => {
                        this.projects = listing.projects;
                        this.thumbnails = thumbnails;
                        if !listing.skipped.is_empty() {
                            this.notice = Some(format!(
                                "有 {} 个项目无法读取，已保留原文件",
                                listing.skipped.len()
                            ));
                        }
                    }
                    Err(error) => this.notice = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn activate(&mut self, project: LocalProject, window: &mut Window, cx: &mut Context<Self>) {
        let registry = Arc::new(standard_registry());
        let document = project.document().clone();
        let canvas = cx.new(|cx| {
            let mut canvas = CreativeCanvas::new(
                registry,
                document,
                demo_renderers(CreativeAssets::demo()),
                window,
                cx,
            )
            .expect("opened project was validated");
            canvas.bind_project(project, cx);
            canvas
        });
        self.current_subscription =
            Some(cx.subscribe_in(&canvas, window, |this, _, event, _, cx| {
                if matches!(event, CreativeEvent::BackToProjects) {
                    this.current = None;
                    this.current_subscription = None;
                    this.refresh(cx);
                    cx.notify();
                }
            }));
        self.current = Some(canvas);
        self.notice = None;
        cx.notify();
    }
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = &self.store else {
            return;
        };
        let value = self.name.read(cx).value();
        let name = if value.trim().is_empty() {
            "未命名项目"
        } else {
            value.trim()
        };
        let result = store.create_project(name, &FlowDocument::default(), &standard_registry());
        match result {
            Ok(project) => {
                self.name
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.activate(project, window, cx);
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                cx.notify();
            }
        }
    }
    fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.opening = true;
        self.notice = Some("正在打开本地项目…".into());
        let id = id.to_string();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.open_project(&id, &standard_registry()) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.opening = false;
                match result {
                    Ok(project) => this.activate(project, window, cx),
                    Err(error) => {
                        this.notice = Some(format!("打开失败，项目文件未修改：{error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}
impl Render for CreativeWorkbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(current) = &self.current {
            return div().size_full().child(current.clone()).into_any_element();
        }
        let t = theme();
        let mut projects = div().flex().flex_wrap().gap_5();
        for project in &self.projects {
            let id = project.metadata.id.clone();
            let name = project.metadata.name.clone();
            let thumbnail = self.thumbnails.get(&id).cloned();
            projects = projects.child(
                div()
                    .id(SharedString::from(format!("project-{id}")))
                    .w(px(268.))
                    .h(px(220.))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.panel))
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|style| style.border_color(rgb(0x9bbbe9)))
                    .on_click(cx.listener(move |this, _, window, cx| this.open(&id, window, cx)))
                    .child(
                        div()
                            .h(px(156.))
                            .bg(rgb(0xebedf3))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(thumbnail.clone(), |cover, path| {
                                cover.child(
                                    img(path)
                                        .size_full()
                                        .object_fit(ObjectFit::Cover)
                                        .with_fallback(|| {
                                            div().size_full().bg(rgb(0xebedf3)).into_any_element()
                                        }),
                                )
                            })
                            .when(thumbnail.is_none(), |cover| {
                                cover.child(
                                    div()
                                        .text_size(px(24.))
                                        .text_color(rgb(0xb0b6c5))
                                        .child(chrome::icon("layers", 24.)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().child(name))
                            .child(label(format!(
                                "{} 个对象 · {} 个素材",
                                project.card_count, project.asset_count
                            ))),
                    ),
            );
        }
        if self.projects.is_empty() {
            projects = projects.child(div().py_6().child(label(
                "还没有项目。新建后，画布与导入的素材会自动保存在本机。",
            )));
        }
        let mut root = div()
            .size_full()
            .bg(rgb(t.background))
            .text_color(rgb(t.foreground))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(64.))
                    .px_8()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(div().text_size(px(20.)).child("Flow Creative"))
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0xeaf0eb))
                            .text_size(px(11.))
                            .text_color(rgb(0x538668))
                            .child("● 本地工作空间"),
                    ),
            )
            .child(
                div()
                    .id("project-home-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_8()
                    .flex()
                    .flex_col()
                    .gap_8()
                    .child(
                        div()
                            .pt_8()
                            .pb_4()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_4()
                            .child(div().text_size(px(32.)).child("今天想创作什么？"))
                            .child(label("把灵感带到画布上"))
                            .child(
                                div()
                                    .w(px(480.))
                                    .mt_4()
                                    .p_4()
                                    .rounded_xl()
                                    .bg(rgb(t.panel))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        Textarea::new(&self.name)
                                            .bordered(false)
                                            .appearance(false)
                                            .h(px(40.))
                                            .aria_label("新项目名称"),
                                    )
                                    .child(
                                        ControlButton::new("new-project", "＋ 新建项目")
                                            .theme(FlowTheme {
                                                card: 0x24252a,
                                                foreground: 0xffffff,
                                                border: 0x24252a,
                                                ..t
                                            })
                                            .disabled(self.store.is_none() || self.opening)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create(window, cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_lg().child("最近项目"))
                            .child(label(format!("{} 个项目", self.projects.len()))),
                    )
                    .child(projects)
                    .child(label("项目自动保存在本机，随时回来继续。")),
            );
        if let Some(notice) = &self.notice {
            root = root.child(
                div()
                    .px_8()
                    .py_3()
                    .text_sm()
                    .text_color(rgb(0xb06c40))
                    .child(notice.clone()),
            );
        }
        root.into_any_element()
    }
}

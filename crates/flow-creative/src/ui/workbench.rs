use super::*;
use gpui_kit::component::input::{Input, InputState};

mod browser;

/// The native local-first application. Projects are created on disk before a canvas opens.
pub struct CreativeWorkbench {
    store: Option<ProjectStore>,
    projects: Vec<ProjectSummary>,
    previews: Entity<project_previews::ProjectPreviews>,
    _preview_observer: Subscription,
    _theme_observer: Subscription,
    home_generation: u64,
    query: Entity<InputState>,
    _query_subscription: Option<Subscription>,
    page: usize,
    listing: bool,
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
        let previews = cx.new(project_previews::ProjectPreviews::new);
        let preview_observer = cx.observe(&previews, |_, _, cx| cx.notify());
        let theme_observer = cx.observe_global::<gpui_kit::component::Theme>(|this, cx| {
            if this.current.is_none() {
                this.refresh(cx);
            }
        });
        let listing = store.is_some();
        let mut this = Self {
            store,
            projects: vec![],
            previews,
            _preview_observer: preview_observer,
            _theme_observer: theme_observer,
            home_generation: 0,
            query: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("搜索本地项目名称")
                    .context_menu(false)
            }),
            _query_subscription: None,
            page: 0,
            listing,
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
        this._query_subscription = Some(cx.subscribe(&this.query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.current.is_none() {
                this.page = 0;
                this.request_covers(cx);
                cx.notify();
            }
        }));
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
        self.home_generation = self.home_generation.wrapping_add(1);
        self.listing = true;
        let generation = self.home_generation;
        self.previews.update(cx, |previews, cx| previews.cancel(cx));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.list_projects() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.home_generation != generation || this.current.is_some() {
                    return;
                }
                this.listing = false;
                match result {
                    Ok(listing) => {
                        this.projects = listing.projects;
                        this.request_covers(cx);
                        if !listing.skipped.is_empty() {
                            this.notice = Some(format!(
                                "有 {} 个项目无法读取，已保留原文件",
                                listing.skipped.len()
                            ));
                        }
                    }
                    Err(error) => {
                        this.notice = Some(error.to_string());
                        this.request_covers(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn request_covers(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let query = self.query.read(cx).value();
        let page = browser::project_page(&self.projects, query.as_ref(), self.page);
        self.page = page.index;
        let ids: Vec<_> = page
            .items
            .iter()
            .filter(|p| p.card_count > 0)
            .map(|p| p.metadata.id.clone())
            .collect();
        let backend =
            text_export::frame_text_backend_for(Arc::new(cx.text_system().all_font_names()), cx)
                .ok();
        self.previews
            .update(cx, |previews, cx| previews.request(store, ids, backend, cx));
    }

    fn change_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page;
        self.request_covers(cx);
        cx.notify();
    }
    fn activate(&mut self, project: LocalProject, window: &mut Window, cx: &mut Context<Self>) {
        self.home_generation = self.home_generation.wrapping_add(1);
        self.previews.update(cx, |previews, cx| previews.cancel(cx));
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
        let query = self.query.read(cx).value();
        let page = browser::project_page(&self.projects, query.as_ref(), self.page);
        let page_index = page.index;
        let page_count = page.pages;
        let match_count = page.matches;
        let mut projects = div().flex().flex_wrap().gap_5();
        for project in page.items {
            let id = project.metadata.id.clone();
            let name = project.metadata.name.clone();
            let thumbnail = self
                .previews
                .read(cx)
                .result(&id)
                .and_then(|result| result.thumbnail.clone());
            let (preview, preview_label) = self
                .previews
                .read(cx)
                .result(&id)
                .map(|result| (result.image.clone(), result.label))
                .unwrap_or((
                    None,
                    if project.card_count == 0 {
                        "空白画布"
                    } else {
                        "正在绘制作品预览…"
                    },
                ));
            let cover_label = if preview.is_none() && thumbnail.is_some() {
                "素材预览"
            } else {
                preview_label
            };
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
                            .relative()
                            .h(px(156.))
                            .overflow_hidden()
                            .bg(rgb(0xebedf3))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(preview.clone(), |cover, image| {
                                cover.child(
                                    img(image)
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                )
                            })
                            .when_some(
                                thumbnail.clone().filter(|_| preview.is_none()),
                                |cover, path| {
                                    cover.child(
                                        img(path)
                                            .size_full()
                                            .object_fit(ObjectFit::Cover)
                                            .with_fallback(|| {
                                                div()
                                                    .size_full()
                                                    .bg(rgb(0xebedf3))
                                                    .into_any_element()
                                            }),
                                    )
                                },
                            )
                            .when(thumbnail.is_none() && preview.is_none(), |cover| {
                                cover.child(
                                    div()
                                        .text_size(px(24.))
                                        .text_color(rgb(0xb0b6c5))
                                        .child(chrome::icon("layers", 24.)),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .left(px(6.))
                                    .bottom(px(4.))
                                    .px_1()
                                    .rounded_sm()
                                    .bg(rgba(0xffffffdd))
                                    .text_size(px(10.))
                                    .text_color(rgb(0x7b818b))
                                    .child(cover_label),
                            ),
                    )
                    .child(
                        div()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().truncate().child(name))
                            .child(label(format!(
                                "{} 个对象 · {} 个素材",
                                project.card_count, project.asset_count
                            ))),
                    ),
            );
        }
        if match_count == 0 {
            projects = projects.child(div().py_6().child(label(if self.listing {
                "正在读取本地项目…"
            } else if self.projects.is_empty() {
                "还没有项目。新建后，画布与导入的素材会自动保存在本机。"
            } else {
                "没有匹配的项目，试试其他名称或清除搜索。"
            })));
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
                            .child(label(format!(
                                "{} / {} 个项目",
                                match_count,
                                self.projects.len()
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(
                                div().w(px(300.)).child(
                                    Input::new(&self.query)
                                        .h(px(36.))
                                        .aria_label("搜索本地项目名称"),
                                ),
                            )
                            .child(
                                ControlButton::new("clear-project-query", "清除")
                                    .theme(t)
                                    .disabled(query.is_empty())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.query.update(cx, |input, cx| {
                                            input.set_value("", window, cx)
                                        });
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                ControlButton::new("previous-project-page", "上一页")
                                    .theme(t)
                                    .disabled(page_index == 0 || self.listing)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.change_page(page_index.saturating_sub(1), cx);
                                    })),
                            )
                            .child(label(format!("{} / {} 页", page_index + 1, page_count)))
                            .child(
                                ControlButton::new("next-project-page", "下一页")
                                    .theme(t)
                                    .disabled(page_index + 1 >= page_count || self.listing)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.change_page(page_index.saturating_add(1), cx);
                                    })),
                            ),
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

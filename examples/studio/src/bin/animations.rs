//! Host-clock adapter for deterministic engine animations; no provider calls.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView,
    animation::*,
    engine::*,
    scene::{self, Affine, Color, DisplayObject, Scene, Style, display},
};
use std::time::{Duration, Instant};
fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("card", display::Rect::new(180., 110.).rounded(18.))
                .with_transform(Affine::translation(50., 70.))
                .with_style(Style::fill(Color::hex(0x4bbfaa))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "disc",
                display::Custom::new(
                    "demo.disc",
                    scene::Bounds::new(scene::Point::ZERO, scene::Size::new(70., 70.)),
                    vec![],
                )
                .with_data(serde_json::json!({"radius":35.})),
            )
            .with_transform(Affine::translation(70., 280.))
            .with_style(Style::fill(Color::hex(0xa08bfa))),
            None,
            None,
        )
        .unwrap();
    for (id, text, y) in [
        ("heading", "TRANSLATION / COLOR / OPACITY", 22.),
        ("custom-heading", "CUSTOM NUMERIC PROPERTY / GEOMETRY", 232.),
    ] {
        scene
            .insert(
                DisplayObject::new(id, display::Text::new(text, scene::Size::new(700., 28.)))
                    .with_transform(Affine::translation(35., y))
                    .with_style(Style::fill(Color::hex(0xc9d6e8))),
                None,
                None,
            )
            .unwrap();
    }
    scene
}
fn engine() -> SceneEngine {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "demo.disc",
            PropertySchema {
                properties: vec![PropertyDescriptor::number(
                    "radius",
                    35.,
                    10.,
                    90.,
                    DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT,
                )],
                allow_unknown: false,
            },
            |value: &serde_json::Value| {
                let circle = display::Ellipse::circle(value["radius"].as_f64().unwrap() as f32);
                Ok((circle.bounds(), vec![circle.flatten(0.1)]))
            },
        ))
        .unwrap();
    SceneEngine::with_registry(scene(), registry).unwrap()
}
use gpui_flow::editor::PlaybackToken;
struct Lab {
    view: Entity<SceneView>,
    animations: AnimationController,
    ids: Vec<AnimationId>,
    playback: Option<PlaybackToken>,
    last: Instant,
    frame_pending: bool,
    message: String,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let engine = engine();
        let animations = AnimationController::new(&engine);
        let view = cx.new(|cx| {
            let mut view = SceneView::from_engine(engine, cx);
            view.set_editable(false, cx);
            view
        });
        Self { view, animations, ids: vec![], playback: None, last: Instant::now(), frame_pending: false, message: "Restart begins two 2-second keyframe sequences; Finish is one Undo step, Cancel restores the baseline".into() }
    }
    fn restart(&mut self, cx: &mut Context<Self>) {
        self.playback = Some(self.view.update(cx, |view, cx| {
            view.set_scene(scene(), cx).unwrap();
            view.editor.begin_playback().unwrap()
        }));
        let engine = &self.view.read(cx).editor.engine;
        self.animations = AnimationController::new(engine);
        self.ids.clear();
        let mut card = AnimationSpec::new(
            1000,
            vec![
                Tween::translation(scene::Point::new(430., 70.)),
                Tween::opacity(0.3),
                Tween::fill_color(Color::hex(0xf391b5)),
            ],
        );
        card.easing = Easing::EaseInOut;
        let mut disc = AnimationSpec::new(
            1000,
            vec![
                Tween::translation(scene::Point::new(390., 280.)),
                Tween::custom_number("radius", 75.),
            ],
        );
        disc.easing = Easing::EaseOut;
        let card_back = AnimationSpec::new(
            1000,
            vec![
                Tween::translation(scene::Point::new(200., 70.)),
                Tween::opacity(0.8),
                Tween::fill_color(Color::hex(0x4bbfaa)),
            ],
        );
        let disc_back = AnimationSpec::new(
            1000,
            vec![
                Tween::translation(scene::Point::new(220., 280.)),
                Tween::custom_number("radius", 50.),
            ],
        );
        for (id, sequence) in [
            ("card", AnimationSequence::new(vec![card, card_back])),
            ("disc", AnimationSequence::new(vec![disc, disc_back])),
        ] {
            self.ids.push(
                self.animations
                    .animate_sequence(engine, engine.handle(&id.into()).unwrap(), sequence)
                    .unwrap(),
            );
        }
        self.last = Instant::now();
        self.message =
            "Playing in an editor-owned session; frames preserve existing history".into();
        cx.notify();
    }
    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.playback.take() {
            let result = self.view.update(cx, |view, cx| {
                let result = self.animations.finish_editor(&mut view.editor, token);
                cx.notify();
                result
            });
            self.message = match result {
                Ok(()) => "Finished as one Undo step; earlier edits preserved".into(),
                Err(error) => error.to_string(),
            };
        }
        cx.notify();
    }
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.playback.take() {
            let result = self.view.update(cx, |view, cx| {
                let result = self.animations.cancel_editor(&mut view.editor, token);
                cx.notify();
                result
            });
            self.message = match result {
                Ok(()) => "Cancelled and restored the baseline; earlier Undo/Redo preserved".into(),
                Err(error) => error.to_string(),
            };
        } else {
            self.message = "Playback already committed; use Undo".into();
        }
        cx.notify();
    }
    fn seek_half(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let token = match self.playback {
                Some(token) => token,
                None => match view.editor.begin_playback() {
                    Ok(token) => {
                        self.playback = Some(token);
                        token
                    }
                    Err(error) => {
                        self.message = error.to_string();
                        return;
                    }
                },
            };
            for id in &self.ids {
                if let Err(error) = self.animations.seek_editor(
                    &mut view.editor,
                    token,
                    *id,
                    Duration::from_millis(1000),
                ) {
                    self.message = error.to_string();
                }
            }
            cx.notify();
        });
        self.last = Instant::now();
        cx.notify();
    }
    fn tick(&mut self, cx: &mut Context<Self>) {
        self.frame_pending = false;
        let Some(token) = self.playback else {
            return;
        };
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last);
        self.last = now;
        let result = self.view.update(cx, |view, cx| {
            let result = self.animations.tick_editor(&mut view.editor, token, delta);
            cx.notify();
            result
        });
        match result {
            Ok(_) => {
                let done = self.ids.iter().all(|id| {
                    self.animations
                        .info(&self.view.read(cx).editor.engine, *id)
                        .is_ok_and(|info| {
                            !matches!(
                                info.status,
                                AnimationStatus::Playing | AnimationStatus::Paused
                            )
                        })
                });
                if done {
                    self.finish(cx);
                }
            }
            Err(error) => {
                self.message = error.to_string();
                for id in &self.ids {
                    let _ = self
                        .animations
                        .pause(&self.view.read(cx).editor.engine, *id);
                }
                if !self
                    .view
                    .update(cx, |view, _| view.editor.is_playback_active(token))
                {
                    self.playback = None;
                }
            }
        }
        cx.notify();
    }
    fn destroy_circle(&mut self, cx: &mut Context<Self>) {
        self.finish(cx);
        self.view.update(cx, |view, cx| {
            if view.editor.scene().get(&"disc".into()).is_some() {
                view.editor
                    .transaction(|scene| {
                        scene.remove(&"disc".into())?;
                        Ok(())
                    })
                    .unwrap();
            }
            let _ = self.animations.sync(&view.editor.engine);
            cx.notify();
        });
        self.message =
            "Host edit recorded separately; Undo restores the document with fresh identity".into();
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.playback.is_some() && self.animations.is_running() && !self.frame_pending {
            self.frame_pending = true;
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| this.tick(cx));
            });
        }
        let engine = &self.view.read(cx).editor.engine;
        let states = self
            .ids
            .iter()
            .filter_map(|id| self.animations.info(engine, *id).ok())
            .map(|info| {
                format!(
                    "{:?} {:.2}/{:.2}s",
                    info.status,
                    info.elapsed.as_secs_f32(),
                    info.duration.as_secs_f32()
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let status = format!(
            "{}\n{} | engine revision {}",
            self.message,
            states,
            engine.revision()
        );
        let theme = FlowTheme::default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(
                div()
                    .min_h(px(60.))
                    .p_3()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .bg(rgb(theme.panel))
                    .child("Animation engine")
                    .child(
                        ControlButton::new("restart", "Restart")
                            .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                    )
                    .child(ControlButton::new("pause", "Pause").on_click(cx.listener(
                        |this, _, _, cx| {
                            for id in &this.ids {
                                let _ = this
                                    .animations
                                    .pause(&this.view.read(cx).editor.engine, *id);
                            }
                            this.message = "Paused; time does not accumulate".into();
                            cx.notify();
                        },
                    )))
                    .child(ControlButton::new("resume", "Resume").on_click(cx.listener(
                        |this, _, _, cx| {
                            for id in &this.ids {
                                let _ = this
                                    .animations
                                    .resume(&this.view.read(cx).editor.engine, *id);
                            }
                            this.last = Instant::now();
                            this.message =
                                "Resume (completed/cancelled tracks need Restart)".into();
                            cx.notify();
                        },
                    )))
                    .child(
                        ControlButton::new("seek", "Seek 50%")
                            .on_click(cx.listener(|this, _, _, cx| this.seek_half(cx))),
                    )
                    .child(
                        ControlButton::new("finish", "Finish / keep")
                            .on_click(cx.listener(|this, _, _, cx| this.finish(cx))),
                    )
                    .child(
                        ControlButton::new("cancel", "Cancel / revert")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                    )
                    .child(ControlButton::new("undo", "Undo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.finish(cx);
                            this.view.update(cx, |view, cx| {
                                view.editor.undo();
                                cx.notify();
                            });
                        },
                    )))
                    .child(ControlButton::new("redo", "Redo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.finish(cx);
                            this.view.update(cx, |view, cx| {
                                view.editor.redo();
                                cx.notify();
                            });
                        },
                    )))
                    .child(
                        ControlButton::new("destroy", "Destroy circle")
                            .on_click(cx.listener(|this, _, _, cx| this.destroy_circle(cx))),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .min_h(px(58.))
                    .p_3()
                    .text_xs()
                    .text_color(rgb(theme.muted))
                    .child(status),
            )
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1260.), px(690.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · animation lifecycle");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

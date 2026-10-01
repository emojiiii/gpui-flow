use super::*;
use crate::display::composite::{self, RequestKey};
use gpui_flow_composite::{DomainSource, RasterViewport};
use gpui_flow_scene::{Affine, Bounds as SceneBounds};
use std::sync::Arc;

pub(super) fn geometry(
    bounds: Bounds<Pixels>,
    viewport: gpui_flow_core::Viewport,
    window: &Window,
) -> RasterViewport {
    let rect = |b: Bounds<Pixels>| {
        SceneBounds::new(
            WorldPoint::new(b.origin.x.into(), b.origin.y.into()),
            WorldSize::new(b.size.width.into(), b.size.height.into()),
        )
    };
    RasterViewport {
        world_to_screen: Affine::translation(bounds.origin.x.into(), bounds.origin.y.into())
            * Affine::translation(viewport.pan.x, viewport.pan.y)
            * Affine::scale(viewport.zoom, viewport.zoom),
        canvas_bounds: rect(bounds),
        content_mask: rect(window.content_mask().bounds),
        device_scale: window.scale_factor(),
    }
}
impl SceneView {
    pub(super) fn request_composites(
        &mut self,
        domains: Vec<DomainSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RequestKey>> {
        let mut viewport = self.composite_geometry?;
        // Pan/zoom may change between layout and this render. Canvas placement
        // and parent clipping are confirmed again by each actual paint closure.
        viewport.world_to_screen =
            Affine::translation(
                viewport.canvas_bounds.origin.x,
                viewport.canvas_bounds.origin.y,
            ) * Affine::translation(self.editor.viewport.pan.x, self.editor.viewport.pan.y)
                * Affine::scale(self.editor.viewport.zoom, self.editor.viewport.zoom);
        let key = Arc::new(
            RequestKey::new(
                domains,
                &self.editor.engine,
                viewport,
                window.window_handle().window_id().as_u64(),
            )
            .with_resources(&self.editor.engine, self.resources.as_ref())
            .with_text_backend(self.text_rasterizer.as_ref()),
        );
        let mut retired = self.compositing.request(key.clone());
        for frame in retired.drain(..) {
            self.compositing
                .retire(frame, window, cx.entity().downgrade());
        }
        for frame in self.compositing.drain_obsolete() {
            self.compositing
                .retire(frame, window, cx.entity().downgrade());
        }
        if let Some(work) = self.compositing.take_work() {
            cx.spawn(async move |this, cx| {
                let key = work.key.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { work.run() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.compositing.finish(key, result);
                    cx.notify();
                });
            })
            .detach();
        }
        self.compositing.requested.clone()
    }
    pub(super) fn composite_element(
        &self,
        id: DisplayId,
        key: Arc<RequestKey>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let viewport = self.editor.viewport;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                if !key.text_current() || geometry(bounds, viewport, window) != key.viewport {
                    return;
                }
                // Query immediately before upload, after earlier domains may
                // have accepted/retired images in this same paint phase.
                let Some((frame, candidate, fallback)) = entity
                    .update(cx, |this, _| {
                        if this.compositing.requested.as_ref() != Some(&key) {
                            return None;
                        }
                        this.compositing.frame(&id).map(|(frame, candidate)| {
                            (frame, candidate, this.compositing.old_frame(&id))
                        })
                    })
                    .ok()
                    .flatten()
                else {
                    return;
                };
                match composite::paint(&frame, window) {
                    Ok(()) if candidate => {
                        let entity2 = entity.clone();
                        let _ = entity.update(cx, |this, _| {
                            if let Some(old) = this.compositing.accepted(&key, &id, &frame.image) {
                                this.compositing.retire(old, window, entity2);
                            }
                        });
                        window.on_next_frame(move |_, cx| {
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        });
                    }
                    Err(error) => {
                        let _ = entity.update(cx, |this, _| {
                            this.compositing.upload_failed(&key, &id, error)
                        });
                        let restored = fallback
                            .as_ref()
                            .is_some_and(|old| composite::paint(old, window).is_ok());
                        if !restored {
                            let _ = entity
                                .update(cx, |this, _| this.compositing.block_upload(id.clone()));
                        }
                        window.on_next_frame(move |_, cx| {
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        });
                    }
                    _ => {}
                }
            },
        )
        .absolute()
        .size_full()
    }
}

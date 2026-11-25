use super::*;
use core::prelude::v1::test;
use gpui_flow_composite::RasterViewport;
use gpui_flow_scene::{Affine, Bounds as WBounds, Point as WPoint, Size as WSize};
fn key(revision: u64) -> FrameKey {
    FrameKey {
        engine: 1,
        document: 1,
        revision,
        viewport: RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: WBounds::new(WPoint::ZERO, WSize::new(1024., 512.)),
            content_mask: WBounds::new(WPoint::ZERO, WSize::new(1024., 512.)),
            device_scale: 1.,
        },
        background: 0x172131,
        grid: None,
    }
}
fn source(key: &FrameKey) -> g::Scene {
    let b = key.viewport.canvas_bounds;
    let d = key.viewport.device_scale;
    let bounds = Bounds::new(
        point(ScaledPixels(b.origin.x * d), ScaledPixels(b.origin.y * d)),
        size(
            ScaledPixels(b.size.width * d),
            ScaledPixels(b.size.height * d),
        ),
    );
    let mut source = g::Scene::default();
    source.insert_primitive(Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds,
        content_mask: ContentMask { bounds },
        background: rgba((key.background << 8) | 255).into(),
        border_color: transparent_black(),
        corner_radii: Default::default(),
        border_widths: Default::default(),
    });
    source.finish();
    source
}
fn changed(before: u64, revision: u64, x: f32) -> gpui_flow_engine::ChangeSet {
    gpui_flow_engine::ChangeSet {
        before_revision: before,
        revision,
        damage: vec![WBounds::new(WPoint::new(x, 10.), WSize::new(10., 10.))],
        ..Default::default()
    }
}
#[test]
fn contiguous_damage_coalesces_until_actual_acknowledgement() {
    let mut damage = Damage::new(1);
    damage.acknowledge(1);
    damage.observe(&changed(1, 2, 10.));
    damage.observe(&changed(2, 3, 600.));
    assert_eq!((damage.before, damage.revision), (1, 3));
    assert!(!damage.full);
    assert!(damage.bounds.unwrap().size.width >= 600.);
    damage.acknowledge(2);
    assert_eq!(damage.before, 1);
    assert!(damage.bounds.is_some());
    damage.acknowledge(3);
    assert_eq!(damage.before, 3);
    assert!(damage.bounds.is_none());
    damage.observe(&changed(4, 5, 30.));
    assert!(damage.full, "missing revision must be full");
}
#[test]
fn invalid_damage_and_full_paint_invalidation_never_become_partial() {
    let mut damage = Damage::new(1);
    damage.acknowledge(1);
    damage.observe(&changed(1, 2, f32::NAN));
    assert!(damage.full);
    damage.acknowledge(2);
    let mut next = changed(2, 3, 20.);
    next.full_paint_invalidation = true;
    damage.observe(&next);
    assert!(damage.full);
}
#[test]
fn unacknowledged_candidate_cannot_replace_a_committed_content_identity() {
    let mut cache = Cache::default();
    let a = key(1);
    let mut damage = Damage::new(1);
    let first = cache.prepare(
        source(&a),
        a.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert_eq!(first.admission.tiles, 2);
    cache.acknowledge(&a);
    damage.acknowledge(1);
    damage.observe(&changed(1, 2, 20.));
    let b = key(2);
    let pending = cache.prepare(
        source(&b),
        b.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert_eq!(pending.scene.layers.frames[0].dirty_tiles.len(), 1);
    // Return to an older presented frame after B was prepared but not submitted.
    let old = cache.prepare(
        source(&a),
        a.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert_eq!(old.scene.layers.frames[0].dirty_tiles.len(), 2);
    cache.acknowledge(&a);
    let stable = cache.prepare(
        source(&a),
        a,
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert!(stable.scene.layers.frames[0].dirty_tiles.is_empty());
}
#[test]
fn camera_epoch_backdrop_geometry_and_budget_are_conservative() {
    let a = key(1);
    let damage = Damage::new(1);
    let mut cache = Cache::default();
    let first = cache.prepare(
        source(&a),
        a.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert!(first.key.is_some());
    cache.acknowledge(&a);
    let mut camera = a.clone();
    camera.viewport.world_to_screen = Affine::translation(5., 7.);
    let full = cache.prepare(
        source(&camera),
        camera.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert_eq!(full.scene.layers.frames[0].dirty_tiles.len(), 2);
    let mut dpr = a.clone();
    dpr.viewport.device_scale = 2.;
    let dpr_frame = cache.prepare(
        source(&dpr),
        dpr,
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert_eq!(dpr_frame.scene.layers.frames[0].dirty_tiles.len(), 8);
    let budget = cache.prepare(
        source(&a),
        a.clone(),
        &damage,
        LayerKey(1),
        true,
        Admission::budget(1),
    );
    assert!(budget.key.is_none());
    assert!(budget.scene.layers.frames.is_empty());
    let mut resized = a.clone();
    resized.viewport.canvas_bounds.size.width = 1100.;
    let stale = source(&resized);
    assert!(
        cache
            .prepare(
                stale,
                a.clone(),
                &damage,
                LayerKey(1),
                true,
                Admission::budget(32)
            )
            .key
            .is_none()
    );
    let mut invalid = a.clone();
    invalid.viewport.world_to_screen.tx = f32::NAN;
    assert!(
        cache
            .prepare(
                source(&a),
                invalid,
                &damage,
                LayerKey(1),
                true,
                Admission::budget(32)
            )
            .key
            .is_none()
    );
    let mut fractional = a.clone();
    fractional.viewport.canvas_bounds.origin.x = 0.25;
    fractional.viewport.content_mask = fractional.viewport.canvas_bounds;
    let fallback = cache.prepare(
        source(&fractional),
        fractional,
        &damage,
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert!(fallback.key.is_none());
    assert_eq!(fallback.scene.quads.len(), 1);
}
#[test]
fn unsupported_or_foreign_layers_stay_unmodified_native_input() {
    let a = key(1);
    let damage = Damage::new(1);
    let mut cache = Cache::default();
    let fallback = cache.prepare(
        source(&a),
        a.clone(),
        &damage,
        LayerKey(9),
        false,
        Admission::budget(32),
    );
    assert!(fallback.key.is_none());
    assert_eq!(fallback.scene.quads.len(), 1);
    let mut foreign = source(&a);
    foreign.layers.frames.push(LayerFrame {
        key: LayerKey(9),
        generation: 77,
        background: rgba(0xff00ffff),
        tile_size: 512,
        content: LayerContent::default(),
        dirty_tiles: vec![],
    });
    let fallback = cache.prepare(
        foreign,
        a,
        &damage,
        LayerKey(9),
        true,
        Admission::budget(32),
    );
    assert_eq!(fallback.scene.layers.frames[0].generation, 77);
    assert!(fallback.key.is_none());
}

fn path_source(key: &FrameKey) -> g::Scene {
    let mut scene = source(key);
    let canvas = key.viewport.canvas_bounds;
    let mut path = PathBuilder::fill();
    path.move_to(point(px(20.), px(20.)));
    path.line_to(point(px(90.), px(40.)));
    path.line_to(point(px(55.), px(100.)));
    path.close();
    scene.insert_primitive(PaintedPath::new(
        path.build().unwrap(),
        key.viewport.device_scale,
        ContentMask {
            bounds: Bounds::new(
                point(px(canvas.origin.x), px(canvas.origin.y)),
                size(px(canvas.size.width), px(canvas.size.height)),
            ),
        },
        rgb(0xf5c232).into(),
    ));
    scene.finish();
    scene
}

#[test]
fn path_tile_fanout_and_multi_view_uploads_share_one_window_budget() {
    let a = key(1);
    let first = Cache::default().prepare(
        path_source(&a),
        a.clone(),
        &Damage::new(1),
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    assert!(first.key.is_some());
    assert_eq!(first.admission.tiles, 2);
    let without_path = Cache::default().prepare(
        source(&a),
        a.clone(),
        &Damage::new(1),
        LayerKey(1),
        true,
        Admission::budget(32),
    );
    // A path adds an operation and its packed curve/tile records.
    assert!(first.admission.upload_bytes > without_path.admission.upload_bytes + 2 * 512);
    let mut remaining = Admission {
        tiles: 32,
        upload_bytes: first.admission.upload_bytes * 2 - 1,
    };
    let mut large = key(1);
    large.viewport.canvas_bounds.size = WSize::new(4096., 2048.);
    large.viewport.content_mask = large.viewport.canvas_bounds;
    let fallback = Cache::default().prepare(
        path_source(&large),
        large,
        &Damage::new(1),
        LayerKey(2),
        true,
        remaining,
    );
    assert_eq!(
        fallback.fallback,
        Some("window tile-expanded upload budget exceeded")
    );
    assert!(fallback.scene.layers.frames.is_empty());
    assert_eq!(fallback.scene.paths.len(), 1);

    // Two otherwise eligible views must not each spend the full allowance.
    remaining.consume(first.admission);
    let second = Cache::default().prepare(
        path_source(&a),
        a,
        &Damage::new(1),
        LayerKey(3),
        true,
        remaining,
    );
    assert_eq!(
        second.fallback,
        Some("window tile-expanded upload budget exceeded")
    );
    assert_eq!(second.admission.tiles, 0);
    assert_eq!(second.scene.paths.len(), 1);
}

use gpui_flow_scene::{
    Affine, DisplayObject, EraserMode, HitPolicy, HitTestOptions, MaskMode, Point, Scene, Style,
    display,
};
fn scene(mode: MaskMode, hit_children: bool) -> Scene {
    let mut s = Scene::new();
    let mut group = DisplayObject::new("g", display::Group::new());
    group.hit_children = hit_children;
    s.insert(group, None, None).unwrap();
    s.insert(
        DisplayObject::new("mask", display::Rect::new(40., 30.)).with_mask(mode),
        Some("g".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(40., 30.))
            .with_transform(Affine::translation(20., 0.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    s
}
fn hits(s: &Scene, x: f32) -> Vec<String> {
    s.hit_test_all(Point::new(x, 10.), HitTestOptions::default())
        .unwrap()
        .into_iter()
        .map(|h| h.id.0)
        .collect()
}
#[test]
fn active_masks_use_first_marker_bounds_in_stable_z_order() {
    let mut s = scene(MaskMode::Alpha, true);
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
    s.update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 60.);
    s.update_object(&"mask".into(), |o| {
        o.visible = true;
        o.style.opacity = 0.;
    })
    .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 60.);
    s.update_object(&"mask".into(), |o| {
        o.style.opacity = 1.;
        o.z_index = 1;
    })
    .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 60.);
    s.update_object(&"mask".into(), |o| o.z_index = 0).unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
    // Bounds include preceding objects plus the first marker, not a union of all masks.
    s.insert(
        DisplayObject::new("later", display::Rect::new(200., 30.)).with_mask(MaskMode::Path),
        Some("g".into()),
        None,
    )
    .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
    s.update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
}
#[test]
fn mask_bounds_prefix_includes_preceding_eraser_geometry() {
    let mut s = scene(MaskMode::Alpha, true);
    let mut erase =
        DisplayObject::new("erase", display::Rect::new(100., 30.)).with_eraser(EraserMode::Path);
    erase.z_index = -1;
    s.insert(erase, Some("g".into()), None).unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 100.);
    s.update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 60.);
}
#[test]
fn normal_mask_pick_needs_content_overlap_but_content_keeps_geometric_hits() {
    let s = scene(MaskMode::Alpha, true);
    assert!(hits(&s, 10.).is_empty());
    assert_eq!(hits(&s, 30.), ["ink", "mask"]);
    assert_eq!(hits(&s, 50.), ["ink"]); // no blanket alpha-pixel clipping
    let s = scene(MaskMode::Clipping, true);
    assert_eq!(hits(&s, 10.), ["mask"]);
}
#[test]
fn mask_only_parent_routes_hit_with_parent_coordinates_and_keeps_holes() {
    let mut s = scene(MaskMode::Path, false);
    s.update_object(&"mask".into(), |o| {
        o.style = Style {
            fill: None,
            ..Default::default()
        };
        o.shape = display::Path::from_svg("M0 0H40V30H0Z M25 5H35V20H25Z")
            .unwrap()
            .into();
        o.style.fill_rule = gpui_flow_scene::FillRule::EvenOdd;
    })
    .unwrap();
    assert!(hits(&s, 10.).is_empty()); // marker alone, outside content
    assert_eq!(hits(&s, 22.), ["g"]);
    assert!(hits(&s, 30.).is_empty()); // geometric hole
    assert!(hits(&s, 50.).is_empty()); // content outside marker
    s.update_object(&"g".into(), |o| {
        o.transform = Affine::translation(100., 50.) * Affine::scale(-2., 1.)
    })
    .unwrap();
    let hit = s
        .hit_test(Point::new(56., 60.), HitTestOptions::default())
        .unwrap()
        .unwrap();
    assert_eq!(hit.id.as_str(), "g");
    assert_eq!(hit.local_point, Point::new(22., 10.));
    assert_eq!(hit.path, vec!["g".into()]);
    s.update_object(&"g".into(), |o| o.hit_policy = HitPolicy::None)
        .unwrap();
    assert!(
        s.hit_test(Point::new(56., 60.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
#[test]
fn clipping_parent_can_hit_self_paint_without_content_and_disabled_group_stays_disabled() {
    let s = scene(MaskMode::Clipping, false);
    assert_eq!(hits(&s, 10.), ["g"]);
    let mut s = scene(MaskMode::Path, false);
    s.update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert!(hits(&s, 30.).is_empty());
    let s = scene(MaskMode::None, false);
    assert!(hits(&s, 30.).is_empty());
}
#[test]
fn host_pixel_filters_run_before_mask_overlap_and_parent_proxy() {
    let s = scene(MaskMode::Alpha, false);
    let make = |id: &str| gpui_flow_scene::HitTestResult {
        id: id.into(),
        world_point: Point::new(30., 10.),
        local_point: Point::new(30., 10.),
        path: s.ancestor_path(&id.into()).unwrap(),
    };
    // A transparent content pixel cannot justify hitting its mask.
    assert!(
        s.resolve_mask_hits(vec![make("mask")], 0.)
            .unwrap()
            .is_empty()
    );
    let hits = s
        .resolve_mask_hits(vec![make("ink"), make("mask")], 0.)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id.as_str(), "g");
}

#[test]
fn mask_only_parent_keeps_first_marker_broad_phase_but_normal_children_bypass_it() {
    let mut s = scene(MaskMode::Alpha, false);
    s.insert(
        DisplayObject::new("later-mask", display::Rect::new(30., 30.))
            .with_transform(Affine::translation(100., 0.))
            .with_mask(MaskMode::Clipping),
        Some("g".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("later-ink", display::Rect::new(30., 30.))
            .with_transform(Affine::translation(100., 0.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    assert_eq!(s.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
    assert!(hits(&s, 110.).is_empty());
    s.update_object(&"g".into(), |o| o.hit_children = true)
        .unwrap();
    assert_eq!(hits(&s, 110.), ["later-ink", "later-mask"]);
}

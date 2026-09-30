//! Source-only compatibility regressions authored before the Taffy adapter.
//! UNRUN: compilation and test execution are intentionally prohibited for this slice.
//! These are contract fixtures, not evidence of a successful test run.
use gpui_flow_layout::*;
use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Size, display};
use std::sync::Arc;

fn fixture(width: f32, height: f32, sizes: &[(f32, f32)]) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(width, height)),
            None,
            None,
        )
        .unwrap();
    for (index, &(width, height)) in sizes.iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(index.to_string(), display::Rect::new(width, height)),
                Some("root".into()),
                None,
            )
            .unwrap();
    }
    let mut flow = FlowLayout::default();
    flow.containers.insert("root".into(), FlowStyle::default());
    (scene, flow)
}
fn style(flow: &mut FlowLayout) -> &mut FlowStyle {
    flow.containers.get_mut(&"root".into()).unwrap()
}
fn bounds(scene: &Scene, id: &str) -> Bounds {
    let object = &scene.get(&id.into()).unwrap().object;
    object
        .transform
        .transform_bounds(object.shape.local_bounds().unwrap())
}
fn assert_box(scene: &Scene, id: &str, x: f32, y: f32, width: f32, height: f32) {
    assert_eq!(
        bounds(scene, id),
        Bounds::new(Point::new(x, y), Size::new(width, height)),
        "{id}"
    );
}

#[test]
fn fractional_padding_and_alignment_are_not_pixel_rounded() {
    let (mut scene, mut flow) = fixture(100.5, 50.5, &[(20.25, 10.25), (10.5, 20.5)]);
    *style(&mut flow) = FlowStyle {
        padding: Insets {
            left: 0.25,
            right: 0.5,
            top: 0.75,
            bottom: 0.25,
        },
        gap: 0.5,
        justify: JustifyContent::Center,
        align_items: AlignItems::End,
        ..Default::default()
    };
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 34.5, 40., 20.25, 10.25);
    assert_box(&scene, "1", 55.25, 29.75, 10.5, 20.5);
}

#[test]
fn grow_uses_weights_below_one_again_after_maximum_freezes() {
    let (mut scene, mut flow) = fixture(210., 40., &[(10., 10.), (10., 10.), (10., 10.)]);
    style(&mut flow).gap = 5.;
    for (id, grow, max_width) in [
        ("0", 0.5, Some(20.)),
        ("1", 0.25, Some(30.)),
        ("2", 0.125, None),
    ] {
        flow.items.insert(
            id.into(),
            FlowItem {
                grow,
                max_width,
                ..Default::default()
            },
        );
    }
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 20., 10.);
    assert_box(&scene, "1", 25., 0., 30., 10.);
    assert_box(&scene, "2", 60., 0., 150., 10.);
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn grow_starts_at_clamped_basis_before_distributing_free_space() {
    let (mut scene, mut flow) = fixture(120., 20., &[(10., 10.), (10., 10.)]);
    flow.items.insert(
        "0".into(),
        FlowItem {
            grow: 0.25,
            basis: Some(10.),
            min_width: Some(40.),
            ..Default::default()
        },
    );
    flow.items.insert(
        "1".into(),
        FlowItem {
            grow: 0.25,
            basis: Some(10.),
            ..Default::default()
        },
    );
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 75., 10.);
    assert_box(&scene, "1", 75., 0., 45., 10.);
}

#[test]
fn extremely_different_finite_weights_still_redistribute_after_a_cap() {
    let (mut scene, mut flow) = fixture(100., 20., &[(10., 10.), (10., 10.)]);
    flow.items.insert(
        "0".into(),
        FlowItem {
            grow: f32::MAX,
            max_width: Some(30.),
            ..Default::default()
        },
    );
    flow.items.insert(
        "1".into(),
        FlowItem {
            grow: f32::from_bits(1),
            ..Default::default()
        },
    );
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 30., 10.);
    assert_box(&scene, "1", 30., 0., 70., 10.);
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn overflow_alignment_is_safe_and_never_shrinks_either_axis() {
    for justify in [
        JustifyContent::Center,
        JustifyContent::End,
        JustifyContent::SpaceBetween,
    ] {
        for align_items in [AlignItems::Center, AlignItems::End] {
            let (mut scene, mut flow) = fixture(30., 20., &[(40., 30.), (20., 10.)]);
            *style(&mut flow) = FlowStyle {
                padding: Insets::all(2.),
                gap: 3.,
                justify,
                align_items,
                ..Default::default()
            };
            flow.apply_to_scene(&mut scene).unwrap();
            assert_box(&scene, "0", 2., 2., 40., 30.);
            assert_eq!(bounds(&scene, "1").origin.x, 45.);
        }
    }
}

#[test]
fn wrapped_reverse_lines_stretch_with_constraints_without_reordering_the_tree() {
    let (mut scene, mut flow) = fixture(100., 120., &[(45., 10.), (45., 30.), (70., 20.)]);
    *style(&mut flow) = FlowStyle {
        direction: FlowDirection::RowReverse,
        wrap: true,
        wrap_reverse: true,
        gap: 10.,
        line_gap: 5.,
        align_items: AlignItems::Stretch,
        align_content: AlignContent::Center,
        ..Default::default()
    };
    flow.items.insert(
        "0".into(),
        FlowItem {
            max_height: Some(15.),
            ..Default::default()
        },
    );
    let children = scene.get(&"root".into()).unwrap().children.clone();
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 0., 32.5, 45., 30.);
    assert_box(&scene, "0", 55., 32.5, 45., 15.);
    assert_box(&scene, "2", 0., 67.5, 70., 20.);
    assert_eq!(scene.get(&"root".into()).unwrap().children, children);
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn content_main_maximum_keeps_normal_origin_no_wrap_reverse_placement() {
    let (mut scene, mut flow) = fixture(20., 20., &[(40., 10.), (50., 20.)]);
    *style(&mut flow) = FlowStyle {
        direction: FlowDirection::RowReverse,
        width_sizing: AxisSizing::content_between(1., Some(60.)),
        height_sizing: AxisSizing::content(),
        padding: Insets::all(5.),
        gap: 10.,
        wrap: true,
        wrap_reverse: true,
        justify: JustifyContent::End,
        align_items: AlignItems::End,
        ..Default::default()
    };
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "root", 0., 0., 60., 30.);
    assert_box(&scene, "1", 5., 5., 50., 20.);
    assert_box(&scene, "0", 65., 15., 40., 10.);
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn zero_content_extent_keeps_padding_origin_and_atomic_resize_failure() {
    let (mut scene, mut flow) = fixture(20., 20., &[(10., 10.)]);
    style(&mut flow).padding = Insets::all(30.);
    style(&mut flow).justify = JustifyContent::End;
    style(&mut flow).align_items = AlignItems::End;
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 30., 30., 10., 10.);
    flow.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    let before = scene.clone();
    assert!(flow.apply_to_scene(&mut scene).is_err());
    assert_eq!(scene, before);
}

#[test]
fn fixed_transformed_intrinsic_box_is_not_resized_or_reinterpreted() {
    let (mut scene, mut flow) = fixture(100., 100., &[(20., 10.)]);
    let rotation = Affine::rotation(90.);
    scene
        .update_object(&"0".into(), |object| object.transform = rotation)
        .unwrap();
    style(&mut flow).justify = JustifyContent::Center;
    style(&mut flow).align_items = AlignItems::Center;
    flow.apply_to_scene(&mut scene).unwrap();
    let transform = scene.get(&"0".into()).unwrap().object.transform;
    assert_eq!(
        (transform.a, transform.b, transform.c, transform.d),
        (rotation.a, rotation.b, rotation.c, rotation.d)
    );
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

struct FixtureMetrics;
impl TextMeasurer for FixtureMetrics {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        let width = if request.content.is_empty() { 0. } else { 120. };
        let lines = if request.wrap_width.is_some_and(|width| width < 120.) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: request.wrap_width.map_or(width, |limit| width.min(limit)),
            height: request.line_height * lines as f32,
            line_height: request.line_height,
            lines,
        })
    }
}
#[test]
fn text_measurement_stays_in_existing_bounded_outer_fixed_point() {
    let (mut scene, mut flow) = fixture(100., 200., &[]);
    let mut text = display::Text::new("fixture", Size::new(300., 10.));
    text.font_size = 20.;
    text.line_height = 1.;
    text.wrap = true;
    scene
        .insert(DisplayObject::new("text", text), Some("root".into()), None)
        .unwrap();
    *style(&mut flow) = FlowStyle {
        direction: FlowDirection::ColumnReverse,
        height_sizing: AxisSizing::content(),
        padding: Insets::all(10.),
        align_items: AlignItems::Stretch,
        ..Default::default()
    };
    flow.texts.insert("text".into(), TextSizing::AutoHeight);
    let measured = flow.with_text_measurer(Arc::new(FixtureMetrics));
    let report = measured.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "root", 0., 0., 100., 60.);
    assert_box(&scene, "text", 10., 10., 80., 40.);
    assert!((2..=MAX_FLOW_LAYOUT_PASSES).contains(&report.passes));
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn very_large_geometry_keeps_the_wide_compatibility_path() {
    let unit = f32::from_bits(251 << 23); // 2^124, above the bounded f32 solver domain.
    let (mut scene, mut flow) = fixture(unit * 4., 20., &[(unit, 10.), (unit, 10.)]);
    style(&mut flow).justify = JustifyContent::End;
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", unit * 2., 0., unit, 10.);
    assert_box(&scene, "1", unit * 3., 0., unit, 10.);
    assert!(flow.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn subnormal_geometry_does_not_disappear_in_the_f32_solver() {
    let unit = f32::from_bits(8);
    let (mut scene, mut flow) = fixture(unit * 16., 20., &[(unit * 2., 10.), (unit * 2., 10.)]);
    flow.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    flow.items.insert(
        "1".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    flow.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., unit * 8., 10.);
    assert_box(&scene, "1", unit * 8., 0., unit * 8., 10.);
}

#[test]
fn natural_and_empty_text_keep_measurement_ownership_and_positive_floor() {
    let (mut scene, mut flow) = fixture(100., 200., &[]);
    let mut text = display::Text::new("fixture", Size::new(30., 10.));
    text.font_size = 20.;
    text.line_height = 1.;
    text.wrap = false;
    scene
        .insert(DisplayObject::new("text", text), Some("root".into()), None)
        .unwrap();
    *style(&mut flow) = FlowStyle {
        width_sizing: AxisSizing::content(),
        height_sizing: AxisSizing::content(),
        padding: Insets::all(10.),
        ..Default::default()
    };
    flow.texts.insert("text".into(), TextSizing::Natural);
    let measured = flow.with_text_measurer(Arc::new(FixtureMetrics));
    measured.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "text", 10., 10., 120., 20.);
    assert_box(&scene, "root", 0., 0., 140., 40.);
    scene
        .update_object(&"text".into(), |object| {
            let gpui_flow_scene::Shape::Text(text) = &mut object.shape else {
                unreachable!()
            };
            text.content.clear();
        })
        .unwrap();
    measured.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "text", 10., 10., 1., 20.);
    assert_box(&scene, "root", 0., 0., 21., 40.);
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}

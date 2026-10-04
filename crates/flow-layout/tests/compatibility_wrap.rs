use gpui_flow_layout::{FlowDirection, FlowLayout, FlowStyle, WrapGapPolicy};
use gpui_flow_scene::{DisplayObject, Point, Scene, display};

fn positions(direction: FlowDirection, policy: WrapGapPolicy) -> Vec<Point> {
    let row = matches!(direction, FlowDirection::Row | FlowDirection::RowReverse);
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Box::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    for id in ["a", "b", "c"] {
        scene
            .insert(
                DisplayObject::new(
                    id,
                    display::Rect::new(if row { 50. } else { 20. }, if row { 20. } else { 50. }),
                ),
                Some("root".into()),
                None,
            )
            .unwrap();
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "root".into(),
        FlowStyle {
            direction,
            gap: 10.,
            line_gap: 5.,
            wrap: true,
            wrap_gap_policy: policy,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    let ids = if matches!(
        direction,
        FlowDirection::RowReverse | FlowDirection::ColumnReverse
    ) {
        ["c", "b", "a"]
    } else {
        ["a", "b", "c"]
    };
    ids.into_iter()
        .map(|id| {
            let transform = scene.get(&id.into()).unwrap().object.transform;
            if row {
                Point::new(transform.tx, transform.ty)
            } else {
                Point::new(transform.ty, transform.tx)
            }
        })
        .collect()
}

#[test]
fn pending_gap_policy_changes_real_line_formation_on_each_axis_and_reverse() {
    for direction in [
        FlowDirection::Row,
        FlowDirection::Column,
        FlowDirection::RowReverse,
        FlowDirection::ColumnReverse,
    ] {
        assert_eq!(
            positions(direction, WrapGapPolicy::IncludePendingGap),
            [Point::new(0., 0.), Point::new(0., 25.), Point::new(0., 50.),]
        );
        // First two widths total exactly 100; the accepted gap extends to 110.
        assert_eq!(
            positions(direction, WrapGapPolicy::LeaferExcludePendingGap),
            [Point::new(0., 0.), Point::new(60., 0.), Point::new(0., 25.),]
        );
    }
}

#[test]
fn missing_json_preserves_legacy_and_profile_roundtrips_strictly() {
    let legacy: FlowStyle = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.wrap_gap_policy, WrapGapPolicy::IncludePendingGap);
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("wrap_gap_policy")
            .is_none()
    );
    let mut profile = legacy;
    profile.wrap_gap_policy = WrapGapPolicy::LeaferExcludePendingGap;
    let value = serde_json::to_value(&profile).unwrap();
    assert_eq!(value["wrap_gap_policy"], "leafer_exclude_pending_gap");
    assert_eq!(serde_json::from_value::<FlowStyle>(value).unwrap(), profile);
    assert!(serde_json::from_str::<FlowStyle>(r#"{"wrap_gap_policy":"unbounded"}"#).is_err());
    assert!(
        serde_json::from_str::<FlowStyle>(
            r#"{"wrap_gap_policy":"leafer_exclude_pending_gap","unknown":true}"#
        )
        .is_err()
    );
}

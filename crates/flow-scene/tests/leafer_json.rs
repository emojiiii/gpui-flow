use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, FillRule, HitPolicy, Paint, Point, Scene, Shape, Size,
    Style, display, leafer_json::*,
};
use serde_json::{Value, json};

fn parse(value: Value) -> Scene {
    Scene::from_leafer_json(&value.to_string()).unwrap()
}
fn reject(value: Value, expected: &str) {
    let e = Scene::from_leafer_json(&value.to_string())
        .unwrap_err()
        .to_string();
    assert!(e.contains(expected), "{e:?} did not contain {expected:?}");
}
fn object<'a>(scene: &'a Scene, id: &str) -> &'a DisplayObject {
    &scene.get(&id.into()).unwrap().object
}
fn round_trip(scene: &Scene) -> Scene {
    let encoded = scene.to_leafer_json().unwrap();
    let restored = Scene::from_leafer_json(&encoded).unwrap();
    assert_eq!(scene, &restored);
    restored
}

#[test]
fn all_twelve_supported_tags_preserve_geometry_paint_and_order() {
    let scene = parse(
        json!({"tag":"Group","id":"root","name":"drawing","className":"b a","data":{"purpose":"fixture"},"children":[
            {"tag":"Rect","id":"rect","width":30,"height":20,"cornerRadius":[1,2,3,4],"fill":"#369c","stroke":"#fedcba98","strokeAlign":"center","strokeCap":"round","strokeJoin":"round","strokeWidth":2,"opacity":0.6,"windingRule":"evenodd"},
            {"tag":"Ellipse","id":"ellipse","width":40,"height":30,"startAngle":10,"endAngle":170,"innerRadius":0.25,"closed":true},
            {"tag":"Line","id":"line","points":[-5,2,10,20],"stroke":"#abc","strokeWidth":2},
            {"tag":"Polygon","id":"polygon","points":[{"x":0,"y":0},{"x":12,"y":0},{"x":3,"y":8}],"fill":"#f00"},
            {"tag":"Star","id":"star","width":60,"height":60,"corners":7,"innerRadius":0.4,"startAngle":20},
            {"tag":"Path","id":"path","path":"M0 0 Q10 30 20 0 C20 2 30 4 40 0 Z","fill":"#123456"},
            {"tag":"Text","id":"text","width":100,"height":40,"text":"Hello 世界","textOverflow":"hide","lineHeight":{"type":"percent","value":1.2}},
            {"tag":"Image","id":"image","url":"asset://example.png","width":30,"height":40,"cornerRadius":2},
            {"tag":"Canvas","id":"canvas","width":20,"height":20,"pixelRatio":2,"smooth":false,"safeResize":true},
            {"tag":"Box","id":"box","width":100,"height":60,"children":[{"tag":"Rect","id":"nested","x":1,"y":2}]},
            {"tag":"Frame","id":"frame","width":100,"height":100,"children":[]}
        ]}),
    );
    assert_eq!(scene.len(), 13);
    let restored = round_trip(&scene);
    assert_eq!(
        restored.get(&"root".into()).unwrap().children[2].as_str(),
        "line"
    );
    assert_eq!(object(&restored, "rect").style.fill_rule, FillRule::EvenOdd);
    assert_eq!(object(&restored, "image").style.fill, None);
    if let Shape::Image(image) = &object(&restored, "image").shape {
        assert_eq!(image.fit, display::ImageFit::Fill);
    } else {
        panic!("Image tag changed");
    }
}

#[test]
fn pen_branch_keeps_its_tag_order_styles_and_native_identity() {
    let input = json!({"tag":"Pen","id":"pen","x":12,"y":5,"scaleX":-2,"rotation":15,"children":[
        {"tag":"Path","id":"first","path":"M0 0L20 0L0 10Z","fill":"#f008","stroke":"#123","strokeWidth":2},
        {"tag":"Path","id":"second","path":"M30 0Q40 20 50 0","fill":null,"stroke":"#00f","strokeWidth":3,"strokeCap":"round"},
        {"tag":"Pen","id":"nested","children":[]}
    ]});
    let scene = parse(input);
    let pen = &object(&scene, "pen").shape;
    assert_eq!(pen.tag(), "pen_group");
    assert!(pen.allows_children());
    assert!(pen.local_bounds().is_none());
    let Shape::Group(group) = pen else {
        panic!("structural Pen branch")
    };
    assert_eq!(group.kind(), display::GroupKind::Pen);
    assert_eq!(
        scene
            .get(&"pen".into())
            .unwrap()
            .children
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "nested"]
    );
    let exported: Value = serde_json::from_str(&scene.to_leafer_json().unwrap()).unwrap();
    assert_eq!(exported[0]["tag"], "Pen");
    assert_eq!(exported[0]["children"][2]["tag"], "Pen");
    let native = scene.to_json().unwrap();
    assert!(native.contains("\"kind\": \"pen\"") || native.contains("\"kind\":\"pen\""));
    let restored = Scene::from_json(&native).unwrap();
    assert_eq!(restored, scene);
    assert_eq!(
        restored.get(&"pen".into()).unwrap().object.shape.tag(),
        "pen_group"
    );
    assert_eq!(
        restored.to_leafer_json().unwrap(),
        scene.to_leafer_json().unwrap()
    );
    round_trip(&scene);
    assert_ne!(
        object(&scene, "first").style,
        object(&scene, "second").style
    );
}

#[test]
fn ordinary_group_json_stays_unchanged_and_leaf_pen_is_not_relabelled() {
    assert_eq!(
        serde_json::to_value(display::Group::new()).unwrap(),
        json!({})
    );
    let group: display::Group = serde_json::from_value(json!({})).unwrap();
    assert_eq!(group.kind(), display::GroupKind::Group);
    assert!(serde_json::from_value::<display::Group>(json!({"kind":"unknown"})).is_err());
    let leaf: Shape = display::Pen::new().into();
    assert_eq!(leaf.tag(), "pen");
    assert!(!leaf.allows_children());
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("leaf", leaf), None, None)
        .unwrap();
    let before = scene.clone();
    assert!(scene.to_leafer_json().is_err());
    assert_eq!(scene, before);
    let empty = parse(json!({"tag":"Pen","id":"empty"}));
    assert_eq!(object(&empty, "empty").shape.tag(), "pen_group");
    round_trip(&empty);
}

#[test]
fn pen_numeric_path_builder_commands_preserve_actual_curve_geometry() {
    let numeric = json!([
        1, 0, 0, 2, 20, 0, 7, 30, 20, 40, 0, 5, 40, -10, 50, -10, 60, 0, 11
    ]);
    let scene = parse(
        json!({"tag":"Pen","id":"pen","children":[{"tag":"Path","id":"p","path":numeric,"stroke":"#28a","strokeWidth":2}]}),
    );
    let expected = display::Path::from_svg("M0 0 L20 0 Q30 20 40 0 C40 -10 50 -10 60 0 Z").unwrap();
    assert_eq!(object(&scene, "p").shape, Shape::Path(expected));
    round_trip(&scene);
    let encoded: Value = serde_json::from_str(&scene.to_leafer_json().unwrap()).unwrap();
    assert!(encoded[0]["children"][0]["path"].is_string());
}

#[test]
fn numeric_relative_smooth_and_arc_commands_use_the_shared_svg_normalization() {
    let cases = [
        (
            json!([1, 10, 10, 20, 4, 5, 30, 3, 40, -2, 11]),
            "M10 10 l4 5 h3 v-2 Z",
        ),
        (
            json!([
                1, 0, 0, 5, 10, 10, 20, 10, 30, 0, 6, 40, -10, 50, 0, 7, 60, 10, 70, 0, 8, 80, 0
            ]),
            "M0 0 C10 10 20 10 30 0 S40 -10 50 0 Q60 10 70 0 T80 0",
        ),
        (
            json!([1, 0, 0, 9, 10, 20, 30, 0, 1, 25, 5]),
            "M0 0 A10 20 30 0 1 25 5",
        ),
    ];
    for (path, svg) in cases {
        let scene = parse(json!({"tag":"Path","id":"p","path":path}));
        assert_eq!(
            object(&scene, "p").shape,
            Shape::Path(display::Path::from_svg(svg).unwrap())
        );
        round_trip(&scene);
    }
    round_trip(&parse(json!({"tag":"Path","id":"empty","path":[]})));
}

#[test]
fn malformed_numeric_paths_fail_atomically_without_interpreting_canvas_opcodes() {
    let before = parse(json!({"tag":"Rect","id":"keep"}));
    for path in [
        json!([1, 0]),
        json!([1, 0, 0, 5, 1, 2, 3]),
        json!([1.5, 0, 0]),
        json!([1, 0, "no"]),
        json!([1, 0, 0, 9, 10, 10, 0, 2, 0, 20, 20]),
        json!([21, 0, 0, 10, 10]),
        json!([1, 0, 0, 999]),
        json!([1, 1000001, 0]),
    ] {
        let input = json!({"tag":"Pen","children":[{"tag":"Rect"},{"tag":"Path","path":path}]});
        let mut destination = before.clone();
        assert!(
            destination
                .replace_from_leafer_json(&input.to_string())
                .is_err(),
            "{input}"
        );
        assert_eq!(destination, before);
    }
}

#[test]
fn pen_computed_builder_state_and_late_unknown_fields_are_atomic_errors() {
    let baseline = parse(json!({"tag":"Rect","id":"keep","width":20,"height":10}));
    for key in [
        "path",
        "__path",
        "pathElement",
        "pathStyle",
        "__layout",
        "worldTransform",
        "computedBounds",
        "childlessJSON",
    ] {
        let mut value =
            json!({"tag":"Pen","id":"pen","children":[{"tag":"Path","id":"p","path":"M0 0L5 8"}]});
        value[key] = json!(null);
        let mut destination = baseline.clone();
        assert!(
            destination
                .replace_from_leafer_json(&value.to_string())
                .is_err(),
            "{key}"
        );
        assert_eq!(destination, baseline);
    }
    let late = json!({"tag":"Pen","children":[{"tag":"Path","path":"M0 0L2 3"},{"tag":"Rect","computedBounds":false}]});
    let mut destination = baseline.clone();
    assert!(
        destination
            .replace_from_leafer_json(&late.to_string())
            .is_err()
    );
    assert_eq!(destination, baseline);
    // The same words inside explicit application metadata remain inert data.
    let inert = parse(
        json!({"tag":"Pen","data":{"__layout":{"private":"text only"},"onClick":"never executed"}}),
    );
    round_trip(&inert);
}

#[test]
fn upstream_defaults_are_not_flow_defaults() {
    let s = parse(
        json!([{"tag":"Rect","id":"r"},{"tag":"Frame","id":"f"},{"tag":"Line","id":"l","stroke":"#123"}]),
    );
    assert_eq!(object(&s, "r").style.fill, None);
    assert_eq!(object(&s, "f").style.fill, Some(Color::WHITE.into()));
    let stroke = object(&s, "l").style.stroke.as_ref().unwrap();
    assert_eq!(stroke.cap, gpui_flow_scene::StrokeCap::Butt);
    assert_eq!(stroke.join, gpui_flow_scene::StrokeJoin::Miter);
    assert_eq!(stroke.miter_limit, 10.);
    round_trip(&s);
}

#[test]
fn solid_hex_alpha_and_null_round_trip() {
    for (hex, color) in [
        ("#123", Color::rgb(17, 34, 51)),
        ("#1234", Color::rgba(17, 34, 51, 68)),
        ("#012345", Color::rgb(1, 35, 69)),
        ("#01234567", Color::rgba(1, 35, 69, 103)),
    ] {
        let s = parse(json!({"tag":"Rect","id":"r","fill":hex}));
        assert_eq!(object(&s, "r").style.fill, Some(color.into()));
        round_trip(&s);
    }
    round_trip(&parse(json!({"tag":"Frame","fill":null})));
}

#[test]
fn tree_metadata_visibility_z_order_and_empty_forest() {
    let s = parse(json!([
        {"tag":"Group","id":"g","visible":false,"locked":true,"zIndex":4,"children":[{"tag":"Rect","id":"b"},{"tag":"Rect","id":"a","zIndex":-7}]},
        {"tag":"Rect","id":"last","data":{"n":2,"array":[true,null,"ok"]}}
    ]));
    round_trip(&s);
    assert_eq!(s.roots(), &["g".into(), "last".into()]);
    assert_eq!(
        s.get(&"g".into()).unwrap().children,
        vec!["b".into(), "a".into()]
    );
    assert_eq!(Scene::new().to_leafer_json().unwrap(), "[]");
    assert!(parse(json!([])).is_empty());
}

#[test]
fn transforms_use_pinned_linear_shear_and_signed_scale() {
    let s = parse(
        json!({"tag":"Rect","id":"r","x":3,"y":-4,"scaleX":-2,"scaleY":3,"skewX":90,"skewY":45}),
    );
    let t = object(&s, "r").transform;
    assert_eq!(t.a, -2.);
    assert_eq!(t.d, 3.);
    assert!((t.c - 3. * std::f32::consts::FRAC_PI_2).abs() < 0.000001);
    assert!((t.b + std::f32::consts::FRAC_PI_2).abs() < 0.000001);
    assert_eq!((t.tx, t.ty), (3., -4.));
    round_trip(&s);
}

#[test]
fn rotation_reflection_skew_and_parent_world_geometry_round_trip() {
    for rotation in [-170., -90., -0.5, 0., 37., 90., 180.] {
        let s = parse(
            json!({"tag":"Group","id":"p","x":40,"rotation":rotation,"scaleX":-1.3,"scaleY":2.7,"children":[
            {"tag":"Rect","id":"c","x":2,"y":-5,"rotation":-23,"skewX":12,"skewY":-10,"scaleY":-1.2}]}),
        );
        let restored = Scene::from_leafer_json(&s.to_leafer_json().unwrap()).unwrap();
        for id in ["p", "c"] {
            let before = s.world_transform(&id.into()).unwrap();
            let after = restored.world_transform(&id.into()).unwrap();
            for p in [Point::ZERO, Point::new(31., -17.)] {
                let a = before.transform_point(p);
                let b = after.transform_point(p);
                assert!(
                    (a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001,
                    "{rotation} {a:?} {b:?}"
                );
            }
        }
        assert_eq!(
            object(&s, "p").transform.determinant().signum(),
            object(&restored, "p").transform.determinant().signum()
        );
    }
}

#[test]
fn affine_export_uses_real_fields_not_an_invented_matrix() {
    for t in [
        Affine::new(0., 2., -3., 0., 4., 5.),
        Affine::scale(0., 2.),
        Affine::new(2., 1., 4., 2., 0., 0.),
    ] {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("r", display::Rect::new(10., 20.)).with_transform(t),
            None,
            None,
        )
        .unwrap();
        let json = s.to_leafer_json().unwrap();
        assert!(!json.contains("matrix") && !json.contains("transform"));
        assert!(json.contains("scaleX") && json.contains("skewX"));
    }
    reject(json!({"tag":"Rect","matrix":{"a":1}}), "matrix");
    reject(json!({"tag":"Rect","transform":[1,0,0,1,0,0]}), "transform");
}

#[test]
fn strict_unknown_tag_fields_and_unsupported_features() {
    for v in [
        json!({"tag":"Unknown"}),
        json!({"tag":"rect"}),
        json!({"tag":"Pen","path":"M0 0L1 1"}),
        json!({"tag":"Custom"}),
        json!({"tag":"Rect","bogus":1}),
        json!({"tag":"Rect","text":"ignored?"}),
        json!({"tag":"Rect","rotation":2,"around":"center"}),
        json!({"tag":"Rect","fill":["#123"]}),
        json!({"tag":"Rect","shadow":{"x":1}}),
        json!({"tag":"Rect","animation":{}}),
        json!({"tag":"Rect","onClick":"alert(1)"}),
        json!({"tag":"Rect","children":[]}),
        json!({"tag":"Group","width":200}),
        json!({"tag":"Image","url":"asset://a","width":20,"height":20,"crop":[0,0,1,1]}),
        json!({"tag":"Image","url":"asset://a","width":20,"height":20,"fill":"#000"}),
        json!({"tag":"Canvas","url":"data:image/png;base64,test"}),
        json!({"tag":"Path","path":[1,0,0,999]}),
        json!({"tag":"Rect","strokeWidth":5}),
        json!({"tag":"Rect","fill":"red"}),
    ] {
        assert!(
            Scene::from_leafer_json(&v.to_string()).is_err(),
            "unexpected acceptance: {v}"
        );
    }
}

#[test]
fn numbers_types_duplicate_keys_and_trailing_input_are_strict() {
    for input in [
        r#"{"tag":"Rect","width":1e400}"#,
        r#"{"tag":"Rect","width":NaN}"#,
        r#"{"tag":"Rect","width":Infinity}"#,
        r#"{"tag":"Rect","tag":"Ellipse"}"#,
        r#"{"tag":"Rect","data":{"x":1,"x":2}}"#,
        r#"{"tag":"Rect"} {}"#,
    ] {
        assert!(Scene::from_leafer_json(input).is_err(), "{input}");
    }
    for v in [
        json!({"tag":"Rect","width":"20"}),
        json!({"tag":"Rect","width":0}),
        json!({"tag":"Rect","x":1e20}),
        json!({"tag":"Rect","visible":0}),
        json!({"tag":"Rect","opacity":2}),
        json!({"tag":"Rect","zIndex":1.5}),
        json!({"tag":"Rect","id":12}),
        json!({"tag":"Rect","data":[]}),
        json!({"tag":"Group","children":{}}),
        json!({"tag":"Polygon","points":[{"x":0,"y":0,"extra":0},{"x":2,"y":0},{"x":1,"y":1}]}),
    ] {
        assert!(Scene::from_leafer_json(&v.to_string()).is_err(), "{v}");
    }
}

#[test]
fn duplicate_explicit_and_generated_ids_fail() {
    reject(
        json!([{"tag":"Rect","id":"same"},{"tag":"Rect","id":"same"}]),
        "duplicate",
    );
    reject(
        json!([{"tag":"Rect","id":"leafer-1"},{"tag":"Rect"}]),
        "duplicate",
    );
    let s = parse(json!([{"tag":"Rect"},{"tag":"Rect"}]));
    assert_eq!(s.roots(), &["leafer-0".into(), "leafer-1".into()]);
}

#[test]
fn unsupported_text_and_star_geometry_fail_without_approximation() {
    reject(json!({"tag":"Text","text":"auto"}), "explicit");
    reject(json!({"tag":"Text","width":20,"height":20}), "show");
    reject(
        json!({"tag":"Text","width":20,"height":20,"textOverflow":"hide","textCase":"title"}),
        "title",
    );
    reject(
        json!({"tag":"Text","width":20,"height":20,"textOverflow":"hide","lineHeight":{"type":"percent","value":1.5,"other":1}}),
        "other",
    );
    reject(json!({"tag":"Star","width":20,"height":30}), "equal");
    reject(json!({"tag":"Star","corners":3.5}), "integer");
    reject(json!({"tag":"Line","points":[0,0,1]}), "pairs");
    reject(json!({"tag":"Line","points":[0,0,1,1,2,2]}), "exactly two");
}

#[test]
fn path_requires_explicit_geometry_instead_of_upstream_box_fallback() {
    reject(json!({"tag":"Path"}), "explicit SVG path");
    reject(
        json!({"tag":"Path","path":null}),
        "expected SVG string or numeric command array",
    );
    let s = parse(json!({"tag":"Path","id":"empty","path":""}));
    let Shape::Path(path) = &object(&s, "empty").shape else {
        panic!("Path tag changed");
    };
    assert!(path.commands.is_empty());
    round_trip(&s);
}

#[test]
fn text_fields_round_trip_without_rewriting_content() {
    let s = parse(
        json!({"tag":"Text","id":"t","width":120,"height":80,"text":"MiXeD\n世界","fontFamily":"sans-serif","fontSize":20,"fontWeight":700,"italic":true,
        "lineHeight":30,"textOverflow":"ellipsis","textAlign":"right","verticalAlign":"middle","textWrap":"none","textCase":"upper","textDecoration":"under-delete","paraSpacing":4,"padding":[1,2,3,4]}),
    );
    let restored = round_trip(&s);
    if let Shape::Text(t) = &object(&restored, "t").shape {
        assert_eq!(t.line_height, 1.5);
        assert_eq!(t.content, "MiXeD\n世界");
    }
}

fn nested(depth: usize) -> Value {
    let mut v = json!({"tag":"Rect","id":"leaf"});
    for i in 1..depth {
        v = json!({"tag":"Group","id":format!("g{i}"),"children":[v]});
    }
    v
}
#[test]
fn node_and_json_depth_budgets_reject_before_publication() {
    let s = parse(nested(MAX_LEAFER_DEPTH));
    assert_eq!(s.len(), MAX_LEAFER_DEPTH);
    round_trip(&s);
    reject(nested(MAX_LEAFER_DEPTH + 1), "depth");
    let deep = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    assert!(
        Scene::from_leafer_json(&deep)
            .unwrap_err()
            .to_string()
            .contains("depth")
    );
}

#[test]
fn byte_string_node_and_geometry_budgets() {
    assert!(
        Scene::from_leafer_json(&" ".repeat(MAX_LEAFER_JSON_BYTES + 1))
            .unwrap_err()
            .to_string()
            .contains("1 MiB")
    );
    reject(
        json!({"tag":"Rect","name":"x".repeat(MAX_LEAFER_STRING_BYTES+1)}),
        "string budget",
    );
    let roots: Vec<_> = (0..=MAX_LEAFER_NODES)
        .map(|i| json!({"tag":"Rect","id":format!("r{i}")}))
        .collect();
    reject(json!(roots), "node count");
    reject(
        json!([{"tag":"Star","corners":4096},{"tag":"Star","corners":4096},{"tag":"Line"}]),
        "geometry budget",
    );
    let too_many_values = serde_json::to_string(&vec![0; 65_536]).unwrap();
    assert!(
        Scene::from_leafer_json(&too_many_values)
            .unwrap_err()
            .to_string()
            .contains("value budget")
    );
}

#[test]
fn malformed_late_node_duplicate_and_world_overflow_replace_are_atomic() {
    let mut scene = parse(json!({"tag":"Rect","id":"original","fill":"#f00"}));
    let before = scene.clone();
    let legacy_before = scene.to_json().unwrap();
    for bad in [
        json!([{"tag":"Rect","id":"first"},{"tag":"Rect","width":-1}]),
        json!({"tag":"Group","id":"g","children":[{"tag":"Rect","id":"x"},{"tag":"Rect","id":"x"}]}),
        {
            let mut v = json!({"tag":"Rect"});
            for _ in 0..10 {
                v = json!({"tag":"Group","scaleX":1e6,"scaleY":1e6,"children":[v]});
            }
            v
        },
    ] {
        assert!(scene.replace_from_leafer_json(&bad.to_string()).is_err());
        assert_eq!(scene, before);
        assert_eq!(scene.to_json().unwrap(), legacy_before);
    }
    scene
        .replace_from_leafer_json(r#"{"tag":"Ellipse","id":"new"}"#)
        .unwrap();
    assert_eq!(scene.len(), 1);
    assert!(scene.get(&"new".into()).is_some());
}

#[test]
fn native_scene_json_v1_remains_a_separate_unchanged_format() {
    let scene = parse(json!({"tag":"Rect","id":"native"}));
    let native = scene.to_json().unwrap();
    assert!(native.contains("\"version\": 1"));
    assert_eq!(Scene::from_json(&native).unwrap(), scene);
    assert!(Scene::from_leafer_json(&native).is_err());
    assert!(Scene::from_json(&scene.to_leafer_json().unwrap()).is_err());
    let _: Value = serde_json::from_str(&scene.to_leafer_json().unwrap()).unwrap();
    assert_eq!(scene.to_json().unwrap(), native);
}

#[test]
fn export_rejects_every_unmapped_retained_state_instead_of_dropping_it() {
    let base = parse(json!({"tag":"Rect","id":"r"}));
    type Mutation = Box<dyn Fn(&mut DisplayObject)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|o| o.hit_policy = HitPolicy::Bounds),
        Box::new(|o| {
            o.classes.insert("two words".into());
        }),
        Box::new(|o| {
            o.style.fill = Some(Paint::LinearGradient {
                start: Point::ZERO,
                end: Point::new(1., 1.),
                stops: vec![
                    gpui_flow_scene::GradientStop {
                        offset: 0.,
                        color: Color::BLACK,
                    },
                    gpui_flow_scene::GradientStop {
                        offset: 1.,
                        color: Color::WHITE,
                    },
                ],
            })
        }),
        Box::new(|o| o.style.shadow = Some(gpui_flow_scene::Shadow::new(1., 2., 3., Color::BLACK))),
        Box::new(|o| {
            o.shape = display::Pen::new()
                .move_to(Point::ZERO)
                .line_to(Point::new(1., 1.))
                .into()
        }),
        Box::new(|o| o.data = json!([1, 2, 3])),
        Box::new(|o| {
            if let Shape::Rect(r) = &mut o.shape {
                r.corner_radius = 3.;
                r.corner_radii = Some(display::CornerRadii::uniform(1.));
            }
        }),
    ];
    for mutate in mutations {
        let mut s = base.clone();
        s.update_object(&"r".into(), mutate).unwrap();
        let old = s.clone();
        assert!(s.to_leafer_json().is_err());
        assert_eq!(s, old);
    }
    let mut metadata = base;
    metadata.data = json!({"scene-only":true});
    assert!(metadata.to_leafer_json().is_err());
}

#[test]
fn crop_fit_metadata_and_paint_order_have_no_silent_mapping() {
    let image = parse(json!({"tag":"Image","id":"i","url":"asset://a","width":20,"height":30}));
    for mode in 0..4 {
        let mut s = image.clone();
        s.update_object(&"i".into(), |o| {
            let Shape::Image(v) = &mut o.shape else {
                unreachable!()
            };
            match mode {
                0 => v.source_crop = Some(Bounds::new(Point::new(0.1, 0.2), Size::new(0.5, 0.5))),
                1 => v.fit = display::ImageFit::Contain,
                2 => v.natural_size = Some(Size::new(30., 40.)),
                _ => o.style.fill = Some(Color::BLACK.into()),
            }
        })
        .unwrap();
        assert!(s.to_leafer_json().is_err());
    }
    let mut s = parse(json!({"tag":"Box","id":"b"}));
    s.update_object(&"b".into(), |o| {
        if let Shape::Box(b) = &mut o.shape {
            b.paint_order = gpui_flow_scene::ContainerPaintOrder::StrokeAfterChildren;
        }
    })
    .unwrap();
    assert!(s.to_leafer_json().is_err());
}

#[test]
fn programmatic_defaults_export_paint_explicitly() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 20.)).with_style(Style::default()),
        None,
        None,
    )
    .unwrap();
    round_trip(&s);
    assert!(s.to_leafer_json().unwrap().contains("#000000ff"));
}

#[test]
fn export_programmatic_metadata_is_bounded_before_serialization() {
    let mut s = Scene::new();
    let mut data = json!(null);
    for _ in 0..70 {
        data = json!({"nested":data});
    }
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 20.)).with_data(data),
        None,
        None,
    )
    .unwrap();
    assert!(
        s.to_leafer_json()
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    s.update_object(&"r".into(), |o| o.data = json!({"many":vec![0;65_536]}))
        .unwrap();
    assert!(
        s.to_leafer_json()
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
}

#[test]
fn export_metadata_counts_pending_values_before_expanding_nested_arrays() {
    let mut nested = vec![Value::Null; 33_000];
    nested[32_999] = json!("x".repeat(MAX_LEAFER_STRING_BYTES + 1));
    let mut wide = vec![Value::Null; 33_000];
    wide[32_999] = Value::Array(nested);
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 20.)).with_data(json!({"queued":wide})),
        None,
        None,
    )
    .unwrap();
    // Reject the combined visited/queued budget before traversing a nested
    // branch and reaching its oversized string (or allocating its work stack).
    assert_eq!(
        s.to_leafer_json().unwrap_err().message,
        "export metadata value budget exceeded"
    );
}

#[test]
fn export_rejects_custom_before_validating_unsupported_geometry() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new(
            "custom",
            display::Custom::new(
                "example",
                Bounds::new(Point::ZERO, Size::new(10., 10.)),
                vec![],
            ),
        ),
        None,
        None,
    )
    .unwrap();
    let mut raw = serde_json::to_value(&s).unwrap();
    raw["nodes"]["custom"]["object"]["shape"]["geometry"]["kind"] = json!("");
    // Raw serde admission deliberately skips the Scene's public validation
    // boundary. Export must reject the unsupported tag in its cheap preflight.
    let invalid: Scene = serde_json::from_value(raw).unwrap();
    assert!(invalid.validate().is_err());
    assert_eq!(
        invalid.to_leafer_json().unwrap_err().message,
        "custom tags and runtime registrations are unsupported"
    );
}

#[test]
fn export_rejects_non_solid_paint_before_validating_unsupported_stops() {
    let s = parse(json!({"tag":"Rect","id":"r"}));
    let mut raw = serde_json::to_value(&s).unwrap();
    raw["nodes"]["r"]["object"]["style"]["fill"] = json!({
        "type":"linear_gradient", "start":{"x":0,"y":0}, "end":{"x":1,"y":1},
        "stops":[]
    });
    let invalid: Scene = serde_json::from_value(raw).unwrap();
    assert!(invalid.validate().is_err());
    assert_eq!(
        invalid.to_leafer_json().unwrap_err().message,
        "gradients and non-solid paints are unsupported"
    );
}

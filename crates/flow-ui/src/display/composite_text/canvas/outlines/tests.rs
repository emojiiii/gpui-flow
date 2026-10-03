//! Pure adapter tests: these do not construct a native App or claim font identity.
use super::*;
use gpui_flow_composite::{CanvasCommand, CanvasSurface, ImageInputs};
use gpui_flow_scene::{Color, FillRule, display::Canvas};
use gpui_flow_text::PositionedGlyphOutline;

fn viewport(dpr: f32) -> RasterViewport {
    let bounds = Bounds::new(Point::ZERO, Size::new(50., 50.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: dpr,
    }
}
fn fixture(commands: Vec<OutlineCommand>, budget: &mut OutlineBudget) -> OutlinedText {
    let glyphs = vec![PositionedGlyphOutline {
        font_id: gpui::FontId(7),
        glyph_id: gpui::GlyphId(9),
        baseline_origin: [-4.25, 11.75],
        commands,
    }];
    let allocated_bytes = glyphs.capacity() * size_of::<PositionedGlyphOutline>()
        + glyphs[0].commands.capacity() * size_of::<OutlineCommand>();
    charge_outline_budget(budget, allocated_bytes, 0).unwrap();
    OutlinedText {
        glyphs,
        rows: 1,
        allocated_bytes,
        peak_bytes: allocated_bytes,
    }
}

#[test]
fn canvas_outline_conversion_preserves_quadratic_cubic_and_fractional_negative_positions() {
    let m = Affine::translation(-9.75, 3.125) * Affine::scale(2.5, 2.5);
    let p = |x, y| [m.tx + x * m.a, m.ty + y * m.d];
    let source = vec![
        OutlineCommand::MoveTo(p(-1.25, 0.5)),
        OutlineCommand::QuadTo(p(2., -4.5), p(5.5, 6.25)),
        OutlineCommand::CurveTo(p(-3., 2.5), p(7.25, -8.5), p(9., 10.25)),
        OutlineCommand::LineTo(p(-1.25, 0.5)),
        OutlineCommand::Close,
    ];
    let mut budget = OutlineBudget::default();
    let initial = budget.remaining();
    let outlined = fixture(source, &mut budget);
    let path = local_path(outlined, m, &mut budget, &"stroke".into(), 20, || Ok(())).unwrap();
    assert_eq!(
        path.commands,
        vec![
            PathCommand::MoveTo {
                to: Point::new(-1.25, 0.5)
            },
            PathCommand::QuadraticTo {
                control: Point::new(2., -4.5),
                to: Point::new(5.5, 6.25)
            },
            PathCommand::CubicTo {
                control1: Point::new(-3., 2.5),
                control2: Point::new(7.25, -8.5),
                to: Point::new(9., 10.25)
            },
            PathCommand::LineTo {
                to: Point::new(-1.25, 0.5)
            },
            PathCommand::Close,
        ]
    );
    assert_eq!(
        initial[2] - budget.remaining()[2],
        path.commands.capacity() * size_of::<PathCommand>()
    );
    assert_eq!(initial[3] - budget.remaining()[3], 1 + 5 * 8);
}

#[test]
fn canvas_outline_full_source_uses_one_isotropic_mapping_before_dpr() {
    let mut text = Text::new("outline", Size::new(40., 25.));
    text.padding = [2., 3., 4., 5.];
    text.font_size = 10.;
    text.para_spacing = 3.;
    text.letter_spacing = 1.;
    text.word_spacing = 2.;
    text.para_indent = 5.;
    text.layout_origin = Point::new(-10., -5.);
    for world in [
        Affine::translation(-103.25, -75.125) * Affine::scale(1.5, 1.5),
        Affine::new(0., 2., -3., 0., -103.25, -75.125),
        Affine::new(-2., 0.5, 0., 1., -103.25, -75.125),
    ] {
        let mut previous = None;
        for dpr in [0.75, 1., 2.5] {
            let space = transform::space(world, text.content_bounds(), viewport(dpr), true)
                .unwrap()
                .unwrap();
            let mut budget = OutlineBudget::default();
            let request = request_for(&text, &space, dpr, &mut budget, &"stroke".into()).unwrap();
            assert_eq!(request.text.font_size, text.font_size * space.matrix.d);
            assert_eq!(
                request.text.letter_spacing,
                text.letter_spacing * space.matrix.d
            );
            assert_eq!(
                request.text.word_spacing,
                text.word_spacing * space.matrix.d
            );
            assert_eq!(request.text.para_indent, text.para_indent * space.matrix.d);
            assert_eq!(
                request.text.wrap_width,
                Some(text.content_bounds().size.width * space.matrix.a)
            );
            let local = Point::new(-2.25, 10.5);
            let raster = space.matrix.transform_point(local);
            assert_eq!(
                local_command(OutlineCommand::MoveTo([raster.x, raster.y]), space.matrix).unwrap(),
                PathCommand::MoveTo { to: local }
            );
            let geometry = (request.bounds, request.clip, request.text);
            if let Some(previous) = &previous {
                assert_eq!(&geometry, previous);
            }
            previous = Some(geometry);
        }
    }
}

#[test]
fn canvas_outline_typed_errors_keep_limits_staleness_and_identity_distinct() {
    let id = DisplayId::from("stroke");
    for (error, expected) in [
        (
            TextOutlineError::Binding(OutlineBindingError::StaleSnapshot),
            CompositeError::Superseded,
        ),
        (
            TextOutlineError::UnsupportedDecoration,
            CompositeError::UnsupportedTextStyle(id.clone()),
        ),
        (
            TextOutlineError::Outline(OutlineError::Allocation),
            CompositeError::AllocationFailed,
        ),
        (
            TextOutlineError::Outline(OutlineError::Limit("job work")),
            CompositeError::WorkLimit,
        ),
        (
            TextOutlineError::Outline(OutlineError::Limit("job storage")),
            CompositeError::WorkingSetLimit,
        ),
        (
            TextOutlineError::Outline(OutlineError::Limit("job glyphs")),
            CompositeError::InputLimit,
        ),
    ] {
        assert_eq!(map_error(&id, error), expected);
    }
    for error in [
        TextOutlineError::Binding(OutlineBindingError::WrongTextSystem),
        TextOutlineError::Binding(OutlineBindingError::UnboundFont),
        TextOutlineError::MissingGlyph,
        TextOutlineError::EmojiGlyph,
    ] {
        assert!(
            matches!(map_error(&id, error), CompositeError::TextRasterization { text, .. } if text == id)
        );
    }
}

#[test]
fn canvas_outline_simultaneous_storage_and_conversion_fail_atomically() {
    let id = DisplayId::from("stroke");
    let mut budget = OutlineBudget::new(20, 20, 4096, 4096);
    let output = reserved_vec::<CanvasStrokeTextOutput>(3, &mut budget, &id).unwrap();
    let output_bytes = output.capacity() * size_of::<CanvasStrokeTextOutput>();
    let outlined = fixture(
        vec![OutlineCommand::MoveTo([1., 2.]), OutlineCommand::Close],
        &mut budget,
    );
    let bytes = outlined.allocated_bytes;
    let before = budget.clone();
    let path_bytes = 2 * size_of::<PathCommand>();
    let r = budget.remaining();
    budget = OutlineBudget::new(r[0], r[1], path_bytes - 1, r[3]);
    let limited = budget.clone();
    assert_eq!(
        local_path(outlined, Affine::IDENTITY, &mut budget, &id, 20, || Ok(())).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    assert_eq!(budget, limited);
    assert_eq!(4096 - before.remaining()[2], output_bytes + bytes);

    let mut budget = OutlineBudget::default();
    let outlined = fixture(
        vec![
            OutlineCommand::MoveTo([1., 2.]),
            OutlineCommand::LineTo([f32::NAN, 3.]),
        ],
        &mut budget,
    );
    let before = budget.clone();
    assert!(matches!(
        local_path(outlined, Affine::IDENTITY, &mut budget, &id, 20, || Ok(())),
        Err(CompositeError::InvalidGeometry(_))
    ));
    assert_eq!(budget, before);
}

#[test]
fn canvas_outline_aggregate_work_output_and_request_storage_remain_charged() {
    let id = DisplayId::from("stroke");
    let mut budget = OutlineBudget::new(20, 20, 4096, 18);
    let output = reserved_vec::<CanvasStrokeTextOutput>(2, &mut budget, &id).unwrap();
    let mut paths = Vec::new();
    for _ in 0..2 {
        let outlined = fixture(vec![OutlineCommand::MoveTo([1., 2.])], &mut budget);
        paths
            .push(local_path(outlined, Affine::IDENTITY, &mut budget, &id, 20, || Ok(())).unwrap());
    }
    assert_eq!(budget.remaining()[3], 0);
    let retained = output.capacity() * size_of::<CanvasStrokeTextOutput>()
        + paths
            .iter()
            .map(|p| p.commands.capacity() * size_of::<PathCommand>())
            .sum::<usize>();
    assert_eq!(4096 - budget.remaining()[2], retained);
    let outlined = fixture(Vec::new(), &mut budget);
    assert_eq!(
        local_path(outlined, Affine::IDENTITY, &mut budget, &id, 20, || Ok(())).unwrap_err(),
        CompositeError::WorkLimit
    );

    let text = Text::new("request storage", Size::new(20., 20.));
    let space = transform::space(Affine::IDENTITY, text.content_bounds(), viewport(1.), true)
        .unwrap()
        .unwrap();
    let bytes = text.content.len() + text.font_family.len();
    let mut budget = OutlineBudget::new(20, 20, bytes - 1, 4096);
    assert!(matches!(
        request_for(&text, &space, 1., &mut budget, &id),
        Err(CompositeError::WorkingSetLimit)
    ));
}

#[test]
fn canvas_outline_caller_reduced_limits_bound_complete_batch() {
    let mut surface = CanvasSurface::new(Canvas::new(10., 10.), Default::default()).unwrap();
    let command = CanvasCommand::FillPath {
        path: Path::from_svg("M 1 1 L 2 1 L 2 2 Z").unwrap(),
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    };
    let error = surface
        .draw_commands_with_fonts(&[command], &ImageInputs::default(), |mut request| {
            request.max_outline_commands = 3;
            request.max_outline_path_bytes = 100;
            request.fills.limits.max_surface_bytes = request.fills.retained_bytes + 80;
            request.fills.limits.max_surface_writes = 15;
            let budget = batch_budget(&request);
            assert_eq!(budget.remaining(), [8192, 3, 80, 15]);
            request.fills.limits.max_path_bytes = 40;
            assert_eq!(batch_budget(&request).remaining()[2], 40);
            Err(CompositeError::Superseded)
        })
        .unwrap_err();
    assert_eq!(error, CompositeError::Superseded);
}

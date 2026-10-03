use super::*;
const TTF: &[u8] = include_bytes!("../../tests/fixtures/outline-quadratic.ttf");
const CFF: &[u8] = include_bytes!("../../tests/fixtures/outline-cubic.otf");

fn prepare(bytes: &[u8]) -> PreparedFont {
    PreparedFont::prepare_with_nonce(bytes, [7; 16]).unwrap()
}
fn repair(bytes: &mut [u8]) {
    let count = sfnt::u16_at(bytes, 4).unwrap() as usize;
    let mut head = 0;
    for i in 0..count {
        let at = 12 + i * 16;
        let start = sfnt::u32_at(bytes, at + 8).unwrap() as usize;
        let len = sfnt::u32_at(bytes, at + 12).unwrap() as usize;
        let is_head = &bytes[at..at + 4] == b"head";
        if is_head {
            head = start;
            bytes[start + 8..start + 12].fill(0);
        }
        let sum = sfnt::sum(&bytes[start..start + len], is_head);
        bytes[at + 4..at + 8].copy_from_slice(&sum.to_be_bytes());
    }
    let checksum = sfnt::CHECKSUM.wrapping_sub(sfnt::sum(bytes, false));
    bytes[head + 8..head + 12].copy_from_slice(&checksum.to_be_bytes());
}
fn range(bytes: &[u8], tag: &[u8; 4]) -> Range<usize> {
    sfnt::parse(bytes)
        .unwrap()
        .into_iter()
        .find(|t| &t.tag == tag)
        .unwrap()
        .range
}
fn metadata(bytes: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    Face::parse(bytes, 0)
        .unwrap()
        .names()
        .into_iter()
        .filter(|n| [0, 13, 14].contains(&n.name_id))
        .map(|n| (n.name_id, n.language_id, n.name.to_vec()))
        .collect()
}
#[test]
fn aliases_are_private_all_family_records_replaced_and_license_bytes_preserved() {
    for source in [TTF, CFF] {
        let font = prepare(source);
        assert_eq!(metadata(source), metadata(font.bytes()));
        assert!(!metadata(source).is_empty());
        for name in Face::parse(font.bytes(), 0).unwrap().names() {
            if [1, 3, 4, 6, 16, 18, 21, 25].contains(&name.name_id) {
                assert_eq!(name.to_string().as_deref(), Some(font.private_alias()));
            }
        }
        let original_tables = sfnt::parse(source).unwrap();
        let rewritten_tables = sfnt::parse(font.bytes()).unwrap();
        for table in original_tables
            .iter()
            .filter(|t| ![b"name", b"head"].contains(&&t.tag))
        {
            assert_eq!(
                sfnt::table(source, &original_tables, &table.tag).unwrap(),
                sfnt::table(font.bytes(), &rewritten_tables, &table.tag).unwrap()
            );
        }
        assert_eq!(
            font.identity().source_sha256,
            <[u8; 32]>::from(Sha256::digest(source))
        );
        assert_eq!(
            font.identity().registration_sha256,
            <[u8; 32]>::from(Sha256::digest(font.bytes()))
        );
        assert_ne!(
            font.identity().source_sha256,
            font.identity().registration_sha256
        );
        assert_eq!(font.identity().face_index, 0);
    }
}
#[test]
fn genuine_quadratic_and_cubic_outlines_preserve_coordinates() {
    let quadratic = prepare(TTF)
        .outline_glyph(2, &mut OutlineBudget::default())
        .unwrap();
    assert!(
        quadratic
            .commands
            .contains(&OutlineCommand::QuadTo([270., 700.], [500., 400.]))
    );
    assert_eq!(
        quadratic.commands.first(),
        Some(&OutlineCommand::MoveTo([40., 0.]))
    );
    assert_eq!(quadratic.commands.last(), Some(&OutlineCommand::Close));
    let cubic = prepare(CFF)
        .outline_glyph(2, &mut OutlineBudget::default())
        .unwrap();
    assert!(cubic.commands.contains(&OutlineCommand::CurveTo(
        [100., 650.],
        [350., 650.],
        [500., 400.]
    )));
}
#[test]
fn verified_empty_glyph_is_not_inferred_from_none_or_character() {
    for bytes in [TTF, CFF] {
        let font = prepare(bytes);
        let mut budget = OutlineBudget::default();
        let before = budget.remaining();
        let empty = font.outline_glyph(1, &mut budget).unwrap();
        assert!(empty.commands.is_empty());
        assert!(empty.bounds.is_none());
        assert_eq!(budget.remaining()[0], before[0] - 1);
        assert!(budget.remaining()[3] < before[3]);
    }
}
#[test]
fn translated_same_face_composite_is_real_geometry() {
    let font = prepare(TTF);
    let outline = font
        .outline_glyph(4, &mut OutlineBudget::default())
        .unwrap();
    assert_eq!(outline.commands[0], OutlineCommand::MoveTo([60., 30.]));
    assert!(
        outline
            .commands
            .contains(&OutlineCommand::QuadTo([290., 730.], [520., 430.]))
    );
}
#[test]
fn limits_and_bad_glyph_ids_leave_budget_unchanged() {
    for bytes in [TTF, CFF] {
        let font = prepare(bytes);
        for mut budget in [
            OutlineBudget::new(0, 999, 99999, 99999),
            OutlineBudget::new(10, 1, 99999, 99999),
            OutlineBudget::new(10, 999, 1, 99999),
            OutlineBudget::new(10, 999, 99999, 1),
        ] {
            let before = budget.clone();
            assert!(font.outline_glyph(2, &mut budget).is_err());
            assert_eq!(budget, before);
        }
        let mut budget = OutlineBudget::default();
        let before = budget.clone();
        assert_eq!(
            font.outline_glyph(65536, &mut budget),
            Err(OutlineError::GlyphOutOfRange)
        );
        assert_eq!(budget, before);
    }
}
#[test]
fn static_style_is_exact_and_nonce_changes_registration_identity() {
    let a = prepare(TTF);
    let b = PreparedFont::prepare_with_nonce(TTF, [8; 16]).unwrap();
    assert_ne!(a.private_alias(), b.private_alias());
    assert_ne!(
        a.identity().registration_sha256,
        b.identity().registration_sha256
    );
    assert_eq!(a.identity().source_sha256, b.identity().source_sha256);
    a.require_style(StaticStyle {
        weight: 400,
        italic: false,
    })
    .unwrap();
    assert_eq!(
        a.require_style(StaticStyle {
            weight: 700,
            italic: false
        }),
        Err(OutlineError::Unsupported(Unsupported::StaticStyle))
    );
    assert!(
        a.require_style(StaticStyle {
            weight: 400,
            italic: true
        })
        .is_err()
    );
}
#[test]
fn corrupted_checksums_offsets_and_duplicate_tables_are_rejected() {
    let mut bytes = TTF.to_vec();
    bytes[100] ^= 1;
    assert!(PreparedFont::prepare(&bytes).is_err());
    let mut bytes = TTF.to_vec();
    bytes[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(PreparedFont::prepare(&bytes).is_err());
    let mut bytes = TTF.to_vec();
    let tag: [u8; 4] = bytes[12..16].try_into().unwrap();
    bytes[28..32].copy_from_slice(&tag);
    assert!(PreparedFont::prepare(&bytes).is_err());
    for bytes in [&b"ttcf"[..], &b"wOFF"[..], &TTF[..100]] {
        assert!(PreparedFont::prepare(bytes).is_err());
    }
}
#[test]
fn variable_color_svg_and_signed_fonts_fail_closed() {
    for (tag, error) in [
        (b"fvar", Unsupported::Variable),
        (b"COLR", Unsupported::AlternateGlyphs),
        (b"SVG ", Unsupported::AlternateGlyphs),
        (b"DSIG", Unsupported::SignedFont),
    ] {
        let mut bytes = TTF.to_vec();
        let count = sfnt::u16_at(&bytes, 4).unwrap() as usize;
        let entry = (0..count)
            .find(|i| &bytes[12 + i * 16..16 + i * 16] == b"post")
            .unwrap();
        bytes[12 + entry * 16..16 + entry * 16].copy_from_slice(tag);
        let mut records: Vec<_> = bytes[12..12 + count * 16]
            .chunks(16)
            .map(|r| r.to_vec())
            .collect();
        records.sort();
        for (i, r) in records.iter().enumerate() {
            bytes[12 + i * 16..28 + i * 16].copy_from_slice(r);
        }
        repair(&mut bytes);
        assert_eq!(
            PreparedFont::prepare(&bytes).unwrap_err(),
            OutlineError::Unsupported(error)
        );
    }
}
#[test]
fn composite_cycles_missing_components_and_truncation_never_publish_partial_outline() {
    let original = prepare(TTF);
    let Source::Glyf(ranges) = &original.source else {
        unreachable!()
    };
    let component_offset = ranges[4].start + 12;
    for glyph in [4u16, 99] {
        let mut font = prepare(TTF);
        Arc::make_mut(&mut font.bytes)[component_offset..component_offset + 2]
            .copy_from_slice(&glyph.to_be_bytes());
        let mut budget = OutlineBudget::default();
        let before = budget.clone();
        assert!(font.outline_glyph(4, &mut budget).is_err());
        assert_eq!(budget, before);
    }
    let mut font = prepare(TTF);
    let Source::Glyf(ranges) = &mut font.source else {
        unreachable!()
    };
    ranges[4].end = ranges[4].start + 15;
    assert!(
        font.outline_glyph(4, &mut OutlineBudget::default())
            .is_err()
    );
}
#[test]
fn cff_subroutine_seac_and_missing_endchar_are_explicitly_rejected() {
    for bytes in [
        &[139, 10, 14][..],
        &[139, 29, 14],
        &[139, 139, 139, 139, 14],
    ] {
        let mut work = Work { left: 999, used: 0 };
        assert_eq!(
            cff::preflight(bytes, &mut work),
            Err(OutlineError::Unsupported(Unsupported::CffProgram))
        );
    }
    assert!(cff::preflight(&[139], &mut Work { left: 999, used: 0 }).is_err());
}
#[test]
fn cff_parser_error_after_partial_callbacks_is_not_an_empty_glyph() {
    let mut font = prepare(CFF);
    let Source::Cff(ranges) = &font.source else {
        unreachable!()
    };
    let r = ranges[3].clone();
    // Last drawing instruction is hlineto. Turn it into rrcurveto with too few
    // operands: preflight is bounded; the real parser emits then fails.
    let bytes = Arc::make_mut(&mut font.bytes);
    bytes[r.end - 2] = 8;
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert_eq!(
        font.outline_glyph(3, &mut budget),
        Err(OutlineError::InvalidOutline)
    );
    assert_eq!(budget, before);
}
#[test]
fn malformed_nonempty_truetype_glyph_does_not_become_whitespace() {
    let mut font = prepare(TTF);
    let Source::Glyf(ranges) = &font.source else {
        unreachable!()
    };
    let start = ranges[3].start;
    // Keep one contour but make it a single point; ttf-parser deliberately
    // emits no geometry for that contour. It is not a verified empty glyph.
    Arc::make_mut(&mut font.bytes)[start + 10..start + 12].copy_from_slice(&0u16.to_be_bytes());
    assert!(
        font.outline_glyph(3, &mut OutlineBudget::default())
            .is_err()
    );
}
#[test]
fn malformed_metadata_offset_is_not_silently_dropped() {
    let mut bytes = TTF.to_vec();
    let r = range(&bytes, b"name");
    bytes[r.start + 16..r.start + 18].copy_from_slice(&u16::MAX.to_be_bytes());
    repair(&mut bytes);
    assert!(PreparedFont::prepare(&bytes).is_err());
}
#[test]
fn batch_budget_is_shared_by_empty_and_repeated_glyphs() {
    let font = prepare(TTF);
    let mut budget = OutlineBudget::new(2, 1000, 100000, 100000);
    font.outline_glyph(1, &mut budget).unwrap();
    font.outline_glyph(2, &mut budget).unwrap();
    assert!(font.outline_glyph(2, &mut budget).is_err());
}

#[test]
fn overlapping_tables_are_rejected_before_payload_checksum_work() {
    let mut bytes = TTF.to_vec();
    let start: [u8; 4] = bytes[20..24].try_into().unwrap();
    let len: [u8; 4] = bytes[24..28].try_into().unwrap();
    bytes[36..40].copy_from_slice(&start);
    bytes[40..44].copy_from_slice(&len);
    assert_eq!(
        PreparedFont::prepare(&bytes).unwrap_err(),
        OutlineError::Malformed("overlapping tables")
    );
}
#[test]
fn every_fixture_truncation_is_rejected_without_panicking() {
    for source in [TTF, CFF] {
        for n in 0..source.len() {
            assert!(
                PreparedFont::prepare_with_nonce(&source[..n], [0; 16]).is_err(),
                "accepted truncation at {n}"
            );
        }
    }
}

#[test]
fn non_type_two_cff_declaration_is_rejected_before_outlining() {
    let data = [1, 0, 4, 4, 0, 1, 1, 1, 2, b'X', 0, 1, 1, 1, 4, 140, 12, 6];
    assert_eq!(
        cff::ranges(&data, 0, 1),
        Err(OutlineError::Unsupported(Unsupported::CffFormat))
    );
}
#[test]
fn output_capacity_is_charged_and_never_exceeds_the_admitted_bytes() {
    let font = prepare(TTF);
    let size = std::mem::size_of::<OutlineCommand>();
    let mut budget = OutlineBudget::new(1, 1000, 8 * size, 1000);
    let before = budget.remaining();
    let outline = font.outline_glyph(2, &mut budget).unwrap();
    assert!(outline.commands.capacity() <= 8);
    assert_eq!(
        before[2] - budget.remaining()[2],
        outline.commands.capacity() * size
    );
}
#[test]
fn malformed_required_headers_are_rejected_despite_repaired_checksums() {
    for (tag, offset, value) in [(b"head", 12, 0u16), (b"hhea", 34, 99), (b"maxp", 0, 0)] {
        let mut bytes = TTF.to_vec();
        let r = range(&bytes, tag);
        bytes[r.start + offset..r.start + offset + 2].copy_from_slice(&value.to_be_bytes());
        repair(&mut bytes);
        assert!(PreparedFont::prepare(&bytes).is_err());
    }
}

#[test]
fn cff_face_reparse_cost_is_charged_even_for_empty_glyphs() {
    let font = prepare(CFF);
    let cff_bytes = range(font.bytes(), b"CFF ").len();
    assert!(font.face_parse_work >= cff_bytes);
    let Source::Cff(ranges) = &font.source else {
        unreachable!()
    };
    let cost = font.face_parse_work + ranges[1].len(); // Includes width operands before endchar.
    let mut budget = OutlineBudget::new(8192, 1000, 10000, cost * 2);
    font.outline_glyph(1, &mut budget).unwrap();
    font.outline_glyph(1, &mut budget).unwrap();
    assert_eq!(budget.remaining()[3], 0);
    let before = budget.clone();
    assert!(font.outline_glyph(1, &mut budget).is_err());
    assert_eq!(budget, before);
}

#[test]
fn cff_face_reparse_admission_precedes_parser_and_keeps_budget_atomic() {
    let mut font = prepare(CFF);
    font.face_parse_work = MAX_GLYPH_WORK + 1;
    // A failed parse-work admission must return before trying the Face parser.
    Arc::make_mut(&mut font.bytes).fill(0);
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert_eq!(
        font.outline_glyph(1, &mut budget),
        Err(OutlineError::Limit("glyph or job work"))
    );
    assert_eq!(budget, before);
}

#[test]
fn mixed_composite_cannot_hide_a_one_point_child() {
    let glyf_range = range(TTF, b"glyf");
    let mut glyf_bytes = TTF[glyf_range.clone()].to_vec();
    let head = range(TTF, b"head");
    let format = sfnt::u16_at(TTF, head.start + 50).unwrap();
    let loca_range = range(TTF, b"loca");
    let mut loca = TTF[loca_range].to_vec();
    let ranges = glyf::ranges(&loca, glyf_range.clone(), format, 5).unwrap();
    // Turn the existing curve glyph into a one-point child while preserving a
    // real, reparsable SFNT and an ordinary m glyph for constructor admission.
    let at = ranges[3].start - glyf_range.start + 10;
    glyf_bytes[at..at + 2].copy_from_slice(&0u16.to_be_bytes());
    glyf_bytes.truncate(ranges[4].start - glyf_range.start);
    let mut composite = vec![0; 10];
    composite[..2].copy_from_slice(&(-1i16).to_be_bytes());
    for (flags, glyph) in [(0x23u16, 2u16), (0x03u16, 3u16)] {
        composite.extend_from_slice(&flags.to_be_bytes());
        composite.extend_from_slice(&glyph.to_be_bytes());
        composite.extend_from_slice(&[0; 4]);
    }
    glyf_bytes.extend_from_slice(&composite);
    if format == 0 {
        loca[10..12].copy_from_slice(&((glyf_bytes.len() / 2) as u16).to_be_bytes());
    } else {
        loca[20..24].copy_from_slice(&(glyf_bytes.len() as u32).to_be_bytes());
    }
    let source = replace_fixture_table(TTF, b"glyf", &glyf_bytes);
    let source = replace_fixture_table(&source, b"loca", &loca);
    let font = prepare(&source);
    // The upstream parser reports successful bounds after silently skipping
    // the one-point sibling; our recursive preflight must fail the whole job.
    let mut raw_builder = Builder {
        commands: Vec::new(),
        max_commands: 1000,
        failed: None,
        open: false,
    };
    assert!(
        Face::parse(font.bytes(), 0)
            .unwrap()
            .outline_glyph(GlyphId(4), &mut raw_builder)
            .is_some()
    );
    assert!(!raw_builder.commands.is_empty());
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert_eq!(
        font.outline_glyph(4, &mut budget),
        Err(OutlineError::InvalidOutline)
    );
    assert_eq!(budget, before);
}

// Rebuild a fixture table without changing the remaining payloads.
fn replace_fixture_table(source: &[u8], tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let tables = sfnt::parse(source).unwrap();
    let mut out = source[..12 + tables.len() * 16].to_vec();
    for (i, table) in tables.iter().enumerate() {
        let data = if &table.tag == tag {
            payload
        } else {
            &source[table.range.clone()]
        };
        let at = 12 + i * 16;
        let start = out.len();
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
        out[at + 8..at + 12].copy_from_slice(&(start as u32).to_be_bytes());
        out[at + 12..at + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
    }
    repair(&mut out);
    out
}

#[test]
fn oversized_cff_parse_cost_is_rejected_by_constructor_probe() {
    let r = range(CFF, b"CFF ");
    let mut cff = CFF[r].to_vec();
    // Legal ignored trailing bytes model a large admitted CFF table; every
    // dictionary/charset scan is bounded by this charged whole-table extent.
    cff.resize(MAX_GLYPH_WORK + 1, 0);
    let source = replace_fixture_table(CFF, b"CFF ", &cff);
    assert!(Face::parse(&source, 0).unwrap().tables().cff.is_some());
    assert_eq!(
        PreparedFont::prepare(&source).unwrap_err(),
        OutlineError::Limit("glyph or job work")
    );
}

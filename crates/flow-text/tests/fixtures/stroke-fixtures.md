# First-party stroke test fonts

`stroke-quadratic.ttf` and `stroke-cubic.otf` are tiny original geometric test
fonts, available with their generator under CC0-1.0. They are not production
letterforms and contain no copied system-font data. Regenerate with
`python3 generate_stroke_fonts.py` using fontTools 4.61.1. Deterministic timestamps
make the bytes repeatable. The separate `outline-*` identity fixtures are unchanged.

Both fonts have 24 glyphs, 1,000 units/em, normal width, static weight 400 and
upright style. They map space, period, `mMcCeEoOaAvVfFiIBb`, combining acute
(U+0301) and e-acute (U+00E9). The fi glyph is reached through a `liga` rule.

- O/o have an outer curved contour and an oppositely wound inner counter
- B/b have an outer rectangle and two oppositely wound rectangular counters
- Period supports the existing three-period ellipsis implementation
- Curved glyphs preserve quadratic geometry in TTF and cubic geometry in CFF
- TTF e-acute uses explicit unrounded XY-translated components; CFF contains
  equivalent direct geometry, without subroutines or seac
- GSUB contains f+i→fi; GPOS contains A/V kerning of -80 design units and
  combining-mark placement for m/e/O. The mark has zero advance

The Rust static-outline parser admits both fonts and verifies the counter,
period, accent and mark contour counts, zero mark advance and unchanged shaping
tables. Independent fontTools signed-area checks confirm opposite counter winding
and a larger outer area. SFNT checksums and deterministic regeneration also pass.
These checks do not prove GPUI ligature/kerning/mark behavior; that remains a
native shaping test using these new assets.

SHA-256:

- TTF: `baa84cec7f5f8b51981063d8ce98ba04e166c6998b18595df063248cbc4f04e3`
- CFF: `0d85275cdd5deb518954403700fdb76bd7ec9749c968e386694eee05e66889f1`

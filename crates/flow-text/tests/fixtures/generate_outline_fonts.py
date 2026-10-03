"""First-party geometric test fonts. Requires fontTools 4.61.1 to regenerate.

No existing font data or glyph design is copied. Fixtures and this generator are
available under CC0-1.0. Their names deliberately include license metadata so
rewriting tests can check that private aliases do not discard it.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.t2CharStringPen import T2CharStringPen

HERE = Path(__file__).resolve().parent
ORDER = [".notdef", "space", "m", "curve", "composite"]


def draw(pen, name, cff):
    if name == "space":
        return
    pen.moveTo((40, 0))
    pen.lineTo((40, 500))
    if cff:
        pen.curveTo((100, 650), (350, 650), (500, 400))
    else:
        pen.qCurveTo((270, 700), (500, 400))
    pen.lineTo((500, 0))
    pen.closePath()


def build(cff):
    fb = FontBuilder(1000, isTTF=not cff)
    fb.setupGlyphOrder(ORDER)
    fb.setupCharacterMap({32: "space", 109: "m", 99: "curve", 233: "composite"})
    fb.setupHorizontalMetrics({name: (600, 40) for name in ORDER})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "Flow Fixture", "styleName": "Regular",
                       "uniqueFontIdentifier": "FlowFixture-Original", "fullName": "Flow Fixture Regular",
                       "psName": "FlowFixture-Regular", "version": "Version 1.000",
                       "copyright": "First-party geometric fixture; CC0-1.0",
                       "licenseDescription": "CC0-1.0: original geometric test fixture",
                       "licenseInfoURL": "https://creativecommons.org/publicdomain/zero/1.0/"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    if cff:
        charstrings = {}
        for name in ORDER:
            pen = T2CharStringPen(600, None)
            draw(pen, name, True)
            charstrings[name] = pen.getCharString()
        fb.setupCFF("FlowFixture-Regular", {"FullName": "Flow Fixture Regular", "FamilyName": "Flow Fixture", "Weight": "Regular"}, charstrings, {})
    else:
        glyphs = {}
        for name in ORDER:
            pen = TTGlyphPen(glyphs)
            if name == "composite":
                pen.addComponent("m", (1, 0, 0, 1, 20, 30))
            else:
                draw(pen, name, False)
            glyphs[name] = pen.glyph()
            if name == "composite":
                glyphs[name].components[0].flags &= ~4  # Explicit unrounded XY translation
        fb.setupGlyf(glyphs)
    fb.setupMaxp()
    fb.font["head"].created = fb.font["head"].modified = 0
    fb.font.recalcTimestamp = False
    fb.save(HERE / ("outline-cubic.otf" if cff else "outline-quadratic.ttf"))


if __name__ == "__main__":
    build(False)
    build(True)

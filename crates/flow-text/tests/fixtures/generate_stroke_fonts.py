"""Original geometric stroke-test fonts, CC0-1.0; fontTools 4.61.1.

These are test assets, not production typefaces. No existing glyph designs or
system font bytes are copied. Existing outline-* identity fixtures are untouched.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString

HERE = Path(__file__).resolve().parent
ORDER = [".notdef", "space", "period", "m", "M", "c", "C", "e", "E", "o", "O",
         "a", "A", "v", "V", "f", "F", "i", "I", "fi", "acutecomb", "eacute", "B", "b"]
CMAP = {ord(c): c for c in "mMcCeEoOaAvVfFiIBb"}
CMAP.update({32: "space", 46: "period", 0x301: "acutecomb", 233: "eacute"})
FEATURES = """
languagesystem DFLT dflt;
languagesystem latn dflt;
feature liga { sub f i by fi; } liga;
feature kern { pos A V -80; } kern;
markClass acutecomb <anchor 0 0> @TOP;
feature mark {
  pos base m <anchor 300 650> mark @TOP;
  pos base e <anchor 300 650> mark @TOP;
  pos base O <anchor 300 750> mark @TOP;
} mark;
"""

def polygon(p, points):
    p.moveTo(points[0])
    for point in points[1:]: p.lineTo(point)
    p.closePath()

def ring(p, cubic):
    p.moveTo((40, 350))
    if cubic:
        for pts in [((40, 560), (150, 700), (300, 700)), ((450, 700), (560, 560), (560, 350)),
                    ((560, 140), (450, 0), (300, 0)), ((150, 0), (40, 140), (40, 350))]: p.curveTo(*pts)
    else:
        for pts in [((40, 700), (300, 700)), ((560, 700), (560, 350)),
                    ((560, 0), (300, 0)), ((40, 0), (40, 350))]: p.qCurveTo(*pts)
    p.closePath()
    # Opposite winding gives a genuine nonzero-fill counter.
    p.moveTo((180, 350))
    if cubic:
        for pts in [((180, 240), (230, 150), (300, 150)), ((370, 150), (420, 240), (420, 350)),
                    ((420, 460), (370, 550), (300, 550)), ((230, 550), (180, 460), (180, 350))]: p.curveTo(*pts)
    else:
        for pts in [((180, 150), (300, 150)), ((420, 150), (420, 350)),
                    ((420, 550), (300, 550)), ((180, 550), (180, 350))]: p.qCurveTo(*pts)
    p.closePath()

def basic(p, cubic, dx=0):
    p.moveTo((40+dx, 0)); p.lineTo((40+dx, 500))
    if cubic: p.curveTo((100+dx, 650), (350+dx, 650), (500+dx, 400))
    else: p.qCurveTo((270+dx, 700), (500+dx, 400))
    p.lineTo((500+dx, 0)); p.closePath()

def draw(p, name, cubic):
    if name == "space": return
    if name in ["o", "O"]: return ring(p, cubic)
    if name in ["B", "b"]:
        polygon(p, [(40,0),(40,700),(560,700),(560,0)])
        for bottom, top in [(100,280),(420,600)]: polygon(p,[(170,bottom),(430,bottom),(430,top),(170,top)])
    elif name == "period": polygon(p, [(200,0),(200,100),(300,100),(300,0)])
    elif name == "acutecomb": polygon(p, [(-40,0),(40,0),(160,160),(80,160)])
    elif name in ["i", "I"]:
        polygon(p,[(70,0),(70,500),(170,500),(170,0)])
        polygon(p,[(70,600),(70,700),(170,700),(170,600)])
    elif name == "fi":
        basic(p, cubic)
        polygon(p,[(550,0),(550,500),(650,500),(650,0)])
    elif name in ["A", "a"]: polygon(p,[(30,0),(250,700),(350,700),(570,0),(440,0),(300,480),(160,0)])
    elif name in ["V", "v"]: polygon(p,[(30,700),(170,700),(300,170),(430,700),(570,700),(350,0),(250,0)])
    elif name == "eacute":
        basic(p,cubic)
        polygon(p,[(260,650),(340,650),(460,810),(380,810)])
    else: basic(p,cubic)

def build(cubic):
    fb=FontBuilder(1000,isTTF=not cubic)
    fb.setupGlyphOrder(ORDER); fb.setupCharacterMap(CMAP)
    metrics={n:(0 if n=="acutecomb" else 300 if n in ["space","period"] else 250 if n in ["i","I"] else 800 if n=="fi" else 600,0) for n in ORDER}
    fb.setupHorizontalMetrics(metrics); fb.setupHorizontalHeader(ascent=950,descent=-200)
    family="Flow Stroke Cubic Fixture" if cubic else "Flow Stroke Quadratic Fixture"
    fb.setupNameTable({"familyName":family,"styleName":"Regular","uniqueFontIdentifier":family+"-1",
        "fullName":family,"psName":family.replace(" ",""),"version":"Version 1.000",
        "copyright":"Original geometric fixtures; CC0-1.0","licenseDescription":"CC0-1.0 original geometric test data",
        "licenseInfoURL":"https://creativecommons.org/publicdomain/zero/1.0/"})
    fb.setupOS2(sTypoAscender=950,sTypoDescender=-200,usWinAscent=950,usWinDescent=200)
    fb.setupPost()
    if cubic:
        cs={}
        for name in ORDER:
            p=T2CharStringPen(metrics[name][0],None); draw(p,name,True); cs[name]=p.getCharString()
        fb.setupCFF(family.replace(" ",""),{"FullName":family,"FamilyName":family,"Weight":"Regular"},cs,{})
    else:
        glyphs={}
        for name in ORDER:
            p=TTGlyphPen(glyphs)
            if name=="eacute":
                p.addComponent("e",(1,0,0,1,0,0)); p.addComponent("acutecomb",(1,0,0,1,300,650))
            else: draw(p,name,False)
            glyphs[name]=p.glyph()
            if name=="eacute":
                for component in glyphs[name].components: component.flags &= ~4
        fb.setupGlyf(glyphs)
    fb.setupMaxp(); addOpenTypeFeaturesFromString(fb.font,FEATURES)
    fb.font["head"].created=fb.font["head"].modified=0; fb.font.recalcTimestamp=False
    path=HERE/("stroke-cubic.otf" if cubic else "stroke-quadratic.ttf"); fb.save(path)
    return path

if __name__=="__main__":
    for cubic in [False,True]: print(build(cubic))

"""Original geometric typography test fonts, dedicated to the public domain.

Reproduce with Python and fonttools==4.60.1. No system/upstream outlines or
network sources are used. All intermediate masters live next to the output.
"""
from pathlib import Path
from hashlib import sha256
import struct
import zlib
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.designspaceLib import DesignSpaceDocument, AxisDescriptor, SourceDescriptor
from fontTools.varLib import build
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.sbixStrike import Strike
from fontTools.ttLib.tables.sbixGlyph import Glyph
from fontTools.misc.psCharStrings import T2CharString

ROOT = Path(__file__).resolve().parents[1] / "assets/fonts/typography"
ROOT.mkdir(parents=True, exist_ok=True)
ORDER = [".notdef", "space", "A", "B", "smile", "man", "woman", "girl",
         "zwj", "regionalU", "regionalS", "modifier", "family", "flag", "modified",
         "redLayer", "blueLayer"]
CMAP = {32: "space", 65: "A", 66: "B", 0x1f600: "smile", 0x1f468: "man",
        0x1f469: "woman", 0x1f467: "girl", 0x200d: "zwj",
        0x1f1fa: "regionalU", 0x1f1f8: "regionalS", 0x1f3fb: "modifier"}


def rectangle(left, bottom, right, top):
    pen = TTGlyphPen(None)
    if right > left:
        pen.moveTo((left, bottom))
        pen.lineTo((right, bottom))
        pen.lineTo((right, top))
        pen.lineTo((left, top))
        pen.closePath()
    return pen.glyph()


def font(family, width=500, color=False):
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(ORDER)
    fb.setupCharacterMap(CMAP)
    glyphs = {name: rectangle(0, 0, width, 700) for name in ORDER}
    glyphs["space"] = rectangle(0, 0, 0, 0)
    glyphs["zwj"] = rectangle(0, 0, 0, 0)
    glyphs["redLayer"] = rectangle(0, 0, 500, 700)
    glyphs["blueLayer"] = rectangle(250, 0, 500, 350)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({name: (0 if name == "zwj" else width + 100, 0) for name in ORDER})
    fb.font["hmtx"].metrics["blueLayer"] = (width + 100, 250)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": family, "styleName": "Regular",
                       "uniqueFontIdentifier": family, "fullName": family,
                       "psName": family.replace(" ", "")})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800,
               usWinDescent=200, usWeightClass=400)
    fb.setupPost(underlinePosition=-100, underlineThickness=50)
    fb.setupMaxp()
    fb.font["head"].created = fb.font["head"].modified = 3400000000
    fb.font.recalcTimestamp = False
    if color:
        layers = {name: [("redLayer", 0), ("blueLayer", 1)] for name in
                  ["A", "B", "smile", "man", "woman", "girl", "family", "flag", "modified"]}
        fb.setupCOLR(layers, version=0)
        # Blue is half-transparent, testing actual color-font composition.
        fb.setupCPAL([[(1, 0, 0, 1), (0, 0, 1, 0.5)]])
        addOpenTypeFeaturesFromString(fb.font, """
            feature liga {
                sub man zwj woman zwj girl by family;
                sub regionalU regionalS by flag;
                sub man modifier by modified;
            } liga;
        """)
    return fb.font


color = font("Geometry Color", color=True)
color.save(ROOT / "GeometryColor.ttf", reorderTables=False)

# A real PNG sbix strike, not a CSS image substituted for a font glyph.
def png():
    def chunk(name, data):
        return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))
    rows = []
    for y in range(14):
        row = b"".join(bytes((127, 0, 128, 255) if x >= 5 and y >= 7 else (255, 0, 0, 255)) for x in range(10))
        rows.append(b"\0" + row)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 10, 14, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")

bitmap = font("Geometry Bitmap")
bitmap["sbix"] = newTable("sbix")
strike = Strike(ppem=20)
strike.glyphs["A"] = Glyph(glyphName="A", originOffsetY=0, graphicType="png ", imageData=png())
bitmap["sbix"].strikes[20] = strike
bitmap.save(ROOT / "GeometryBitmap.ttf", reorderTables=False)

# CFF2 blend operands instantiate a genuine variable outline. Its advance
# deliberately remains constant; the TrueType fixture separately tests HVAR.
fb = FontBuilder(1000, isTTF=False)
fb.setupGlyphOrder([".notdef", "space", "A"])
fb.setupCharacterMap({32: "space", 65: "A"})
fb.setupHorizontalMetrics({name: (600, 0) for name in [".notdef", "space", "A"]})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Geometry CFF2", "styleName": "Regular", "uniqueFontIdentifier": "Geometry CFF2", "fullName": "Geometry CFF2", "psName": "GeometryCFF2"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
fb.setupPost()
fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
program = [0, 0, "rmoveto", 500, 300, 1, "blend", 0, "rlineto", 0, 700, "rlineto",
           -500, -300, 1, "blend", 0, "rlineto"]
fb.setupCFF2({".notdef": T2CharString(program=program), "space": T2CharString(program=[]), "A": T2CharString(program=program)},
             regions=[{"wght": (0, 1, 1)}])
fb.font["head"].created = fb.font["head"].modified = 3400000000
fb.font.recalcTimestamp = False
fb.font.save(ROOT / "GeometryCFF2.otf", reorderTables=False)

space = DesignSpaceDocument()
axis = AxisDescriptor()
axis.name, axis.tag, axis.minimum, axis.default, axis.maximum = "Weight", "wght", 100, 400, 900
space.addAxis(axis)
masters = []
for weight, width in [(100, 300), (400, 500), (900, 800)]:
    path = ROOT / f"master-{weight}.ttf"
    font("Geometry Variable", width).save(path, reorderTables=False)
    masters.append(path)
    source = SourceDescriptor()
    source.path, source.name, source.location = str(path), str(weight), {"Weight": weight}
    if weight == 400:
        source.copyInfo = source.copyLib = source.copyFeatures = True
    space.addSource(source)
designspace = ROOT / "geometry.designspace"
space.write(designspace)
variable, _, _ = build(str(designspace))
variable.recalcTimestamp = False
variable.save(ROOT / "GeometryVariable.ttf", reorderTables=False)
for path in masters + [designspace]:
    path.unlink()
for name in ["GeometryColor.ttf", "GeometryVariable.ttf", "GeometryBitmap.ttf", "GeometryCFF2.otf"]:
    print(name, sha256((ROOT / name).read_bytes()).hexdigest())

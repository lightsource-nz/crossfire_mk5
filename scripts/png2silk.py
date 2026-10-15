"""A PNG becomes a silkscreen footprint: dark pixels are ink.

The image is cropped to the pixel box given, its ink thickened at full resolution so that the
thinnest stroke prints at 0.2 mm or more, then scaled by area to 0.1 mm pixels at the height asked
for and kept wherever ink covers 40 % of a cell -- which keeps a thin continuous line continuous
where resampling would break it. The result is written as filled polygons on the front silkscreen,
one rectangle per run of ink pixels with runs that repeat on consecutive rows merged into taller
ones, as a footprint that is placed like any part. A preview PNG of what will print is written
beside it, at four times the size.

Needs Pillow, which KiCad's own Python has. The board's graphic was made with:
    png2silk.py pcb/art/crossfire-logo.png 1950 100 3700 1400 15.5 crossfire_logo pcb/crossfire.pretty 9

usage: png2silk.py in.png x0 y0 x1 y1 height_mm name out_dir [dilate_px]"""
import sys

from PIL import Image, ImageFilter, ImageOps

src, x0, y0, x1, y1, height, name, out_dir = sys.argv[1], *map(int, sys.argv[2:6]), float(sys.argv[6]), sys.argv[7], sys.argv[8]
PX = 0.1                                              # mm per silkscreen pixel

im = Image.open(src).convert("L").crop((x0, y0, x1, y1))
rows = int(round(height / PX))
cols = int(round(rows * im.width / im.height))
#   ink at full resolution, thickened so the thinnest stroke prints at 0.2 mm or more, then scaled
#   by area so a thin continuous line keeps its continuity, and kept where it covers 40 % of a cell
dilate = int(sys.argv[9]) if len(sys.argv) > 9 else 9
full = im.point(lambda v: 255 if v < 128 else 0)
full = full.filter(ImageFilter.MaxFilter(dilate))
cover = full.resize((cols, rows), Image.BOX)
ink = cover.point(lambda v: 255 if v >= 102 else 0)   # 255 where ink
px = ink.load()

#   runs per row, merged downwards where the same run repeats
runs = {}
for j in range(rows):
    i = 0
    while i < cols:
        if px[i, j]:
            s = i
            while i < cols and px[i, j]:
                i += 1
            runs.setdefault((s, i), []).append(j)
        else:
            i += 1
rects = []
for (s, e), js in runs.items():
    start = prev = js[0]
    for j in js[1:] + [None]:
        if j is None or j != prev + 1:
            rects.append((s, start, e, prev + 1))
            if j is not None:
                start = j
        prev = j if j is not None else prev

w_mm, h_mm = cols * PX, rows * PX
lines = ['(footprint "%s"' % name, "\t(version 20260206)", '\t(generator "pcbnew")', '\t(generator_version "10.0")',
         '\t(layer "F.Cu")', '\t(descr "silkscreen graphic, %.1f x %.1f mm, traced from a bitmap at 0.1 mm")' % (w_mm, h_mm),
         '\t(attr board_only exclude_from_pos_files exclude_from_bom)',
         '\t(property "Reference" "G***"\n\t\t(at 0 %.2f 0)\n\t\t(layer "F.SilkS")\n\t\t(hide yes)\n\t\t(uuid "00000000-0000-4000-8000-000000000001")\n\t\t(effects\n\t\t\t(font\n\t\t\t\t(size 1 1)\n\t\t\t\t(thickness 0.15)\n\t\t\t)\n\t\t)\n\t)' % (-h_mm / 2 - 1),
         '\t(property "Value" "%s"\n\t\t(at 0 %.2f 0)\n\t\t(layer "F.Fab")\n\t\t(hide yes)\n\t\t(uuid "00000000-0000-4000-8000-000000000002")\n\t\t(effects\n\t\t\t(font\n\t\t\t\t(size 1 1)\n\t\t\t\t(thickness 0.15)\n\t\t\t)\n\t\t)\n\t)' % (name, h_mm / 2 + 1)]
n = 0
for s, t, e, b in rects:
    xa, ya, xb, yb = s * PX - w_mm / 2, t * PX - h_mm / 2, e * PX - w_mm / 2, b * PX - h_mm / 2
    n += 1
    lines.append('\t(fp_poly\n\t\t(pts\n\t\t\t(xy %.2f %.2f) (xy %.2f %.2f) (xy %.2f %.2f) (xy %.2f %.2f)\n\t\t)\n'
                 '\t\t(stroke\n\t\t\t(width 0)\n\t\t\t(type solid)\n\t\t)\n\t\t(fill yes)\n\t\t(layer "F.SilkS")\n\t\t(uuid "%08x-0000-4000-8000-%012x")\n\t)'
                 % (xa, ya, xb, ya, xb, yb, xa, yb, n, n))
lines.append(")")
open("%s\\%s.kicad_mod" % (out_dir, name), "w", encoding="utf8", newline="\r\n").write("\n".join(lines) + "\n")
preview = ImageOps.invert(ink).resize((cols * 4, rows * 4), Image.NEAREST)
preview.save("%s\\%s-preview.png" % (out_dir, name))
print("%s: %d x %d pixels at %.1f mm = %.1f x %.1f mm, %d rectangles, ink %.0f%%" % (
    name, cols, rows, PX, w_mm, h_mm, len(rects), 100.0 * sum(1 for j in range(rows) for i in range(cols) if px[i, j]) / (rows * cols)))

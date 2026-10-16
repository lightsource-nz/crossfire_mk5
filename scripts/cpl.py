"""The placement file, in the fabricator's columns, with each part's position at the centre of its
pad pattern rather than at its footprint's anchor.

The fabricator places a part by its centre. KiCad's placement export gives the footprint's anchor,
which is central for chip parts and ICs but for a connector is wherever the library put it -- the
first peg, for the USB-C receptacle -- so the part was drawn a body-width off its pads in the
fabricator's preview. The centre of the bounding box of all the pads, holes included, is what the
fabricator's own footprints use as their origin.

Positions are in the Gerbers' frame, Y increasing downwards as KiCad's own export writes it, so
nothing else about the file changes. Parts marked not fitted or excluded from placement are left
out. Uses the Python that ships with KiCad.

usage: cpl.py board.kicad_pcb out.csv"""
import csv
import sys

import pcbnew

g = pcbnew.ToMM
board = pcbnew.LoadBoard(sys.argv[1])
rows = []
for f in board.GetFootprints():
    attrs = f.GetAttributes()
    if attrs & (pcbnew.FP_EXCLUDE_FROM_POS_FILES | pcbnew.FP_DNP):
        continue
    pads = list(f.Pads())
    if not pads:
        continue
    xs, ys = [], []
    for p in pads:
        b = p.GetBoundingBox()
        xs += [b.GetLeft(), b.GetRight()]
        ys += [b.GetTop(), b.GetBottom()]
    cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
    rows.append((f.GetReference(), round(g(cx), 4), round(-g(cy), 4),
                 "Top" if f.GetLayer() == pcbnew.F_Cu else "Bottom", round(f.GetOrientationDegrees(), 1)))
rows.sort(key=lambda r: (len(r[0].rstrip("0123456789")), r[0].rstrip("0123456789"), int(r[0][len(r[0].rstrip("0123456789")):] or 0)))
with open(sys.argv[2], "w", newline="", encoding="utf8") as fh:
    w = csv.writer(fh, quoting=csv.QUOTE_ALL)
    w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
    for r in rows:
        w.writerow(r)
print("%d parts placed, %d on the bottom" % (len(rows), sum(1 for r in rows if r[3] == "Bottom")))

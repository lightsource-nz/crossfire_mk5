"""Read the schematic's connection list and ask whether it says what the drawing looks like it says.

The rules check asks whether anything is illegal. That is a different question, and one of the
three below slips through it entirely:

  * A two-terminal part with a wire across its own two pins.  THIS IS THE ONE THE RULES CHECK
    CANNOT SEE.  Bridging a part is not illegal -- it is just a net -- so what gets reported, if
    anything, is a symptom rather than the fault.  Bridge a part between two NAMED nets and the
    check complains that two names share the same items, which is a true statement about the wrong
    thing and easy to wave through as a known one.  Bridge one where only a single side carries a
    name and the check says nothing whatsoever: the violation count does not move and a net quietly
    disappears.  Both behaviours were measured on this board, not assumed.  A part moved in the
    editor can be left bridged by the wire it was sitting on, which is how three of them arrived.

  * A pin whose only connection is a wire leading nowhere.
  * A wire ending off the grid, which looks connected at any sane zoom and is not.

The rules check does report those last two, as an unconnected wire endpoint and an off-grid
endpoint. They are here so that one command answers the whole question, and so the answer names the
part and the coordinate instead of pointing at a report file.

Pin geometry is read from each sheet's own cached symbol definitions rather than from the system
libraries, so this says what THIS schematic means regardless of what is installed.
"""
import os
import re
import subprocess
import sys
import tempfile

GRID = 1.27


def find_kicad_cli():
    for base in (r"C:\Program Files\KiCad", r"C:\Program Files (x86)\KiCad"):
        if not os.path.isdir(base):
            continue
        for ver in sorted(os.listdir(base), reverse=True):
            exe = os.path.join(base, ver, "bin", "kicad-cli.exe")
            if os.path.exists(exe):
                return exe
    for exe in ("kicad-cli", "kicad-cli.exe"):
        try:
            subprocess.run([exe, "--version"], capture_output=True, check=True)
            return exe
        except Exception:
            pass
    sys.exit("could not find kicad-cli")


def sexp_block(text, start):
    """The span of the balanced parenthesis run that starts at or after `start`."""
    i = text.index("(", start)
    depth = 0
    for j in range(i, len(text)):
        if text[j] == "(":
            depth += 1
        elif text[j] == ")":
            depth -= 1
            if depth == 0:
                return i, j + 1
    return i, len(text)


PIN_RE = re.compile(r'\(pin \w+ \w+\s*\(at ([\d.-]+) ([\d.-]+) (\d+)\).*?\(number "([^"]*)"', re.S)
WIRE_RE = re.compile(r'\(wire\s*\(pts\s*\(xy ([\d.-]+) ([\d.-]+)\) \(xy ([\d.-]+) ([\d.-]+)\)\s*\)')
INST_RE = re.compile(r'\(lib_id "([^"]+)"\)\s*\(at ([\d.-]+) ([\d.-]+) (\d+)\)\s*((?:\(mirror \w\)\s*)?)'
                     r'(.{0,600})', re.S)


def cached_pins(sheet_text):
    """lib_id -> [(number, px, py)], taken from the sheet's own (lib_symbols ...)."""
    out = {}
    at = sheet_text.find("(lib_symbols")
    if at < 0:
        return out
    a, b = sexp_block(sheet_text, at)
    lib = sheet_text[a:b]
    for m in re.finditer(r'\n\t\t\(symbol "([^"]+)"', lib):
        name = m.group(1)
        if "_" in name and re.search(r"_\d+_\d+$", name):
            continue                      # a unit body, not the part
        s, e = sexp_block(lib, m.start())
        out[name] = [(p.group(4), float(p.group(1)), float(p.group(2)))
                     for p in PIN_RE.finditer(lib[s:e])]
    return out


def pin_at(px, py, x, y, rot, mirror):
    """Where a pin lands once the symbol is placed, turned and flipped."""
    a, b = px, -py                        # schematic y runs downwards
    if mirror == "y":
        a = -a
    elif mirror == "x":
        b = -b
    for _ in range((rot // 90) % 4):
        a, b = b, -a
    return x + a, y + b


def body_of(sheet_text):
    """Everything after the cached symbol definitions -- the placed objects."""
    at = sheet_text.find("(lib_symbols")
    if at < 0:
        return sheet_text
    return sheet_text[sexp_block(sheet_text, at)[1]:]


def check_sheets(pcb_dir):
    bridged, offgrid, no_fp, parts = [], [], 0, 0
    for name in sorted(os.listdir(pcb_dir)):
        if not name.endswith(".kicad_sch"):
            continue
        path = os.path.join(pcb_dir, name)
        with open(path, encoding="utf8", newline="") as f:
            text = f.read()
        wires = [tuple(map(float, m.groups())) for m in WIRE_RE.finditer(text)]
        for w in wires:
            for v in w:
                if abs(v / GRID - round(v / GRID)) > 1e-6:
                    offgrid.append((name, w))
                    break
        pins = cached_pins(text)
        ends = {(round(w[0], 2), round(w[1], 2), round(w[2], 2), round(w[3], 2)) for w in wires}
        ends |= {(c, d, a, b) for a, b, c, d in ends}
        for m in INST_RE.finditer(body_of(text)):
            lib_id, x, y, rot, mirror, blk = m.groups()
            if lib_id.startswith("power:"):
                continue
            parts += 1
            fp = re.search(r'\(property "Footprint" "([^"]*)"', blk)
            if not (fp and fp.group(1).strip()):
                no_fp += 1
            p = pins.get(lib_id, [])
            if len(p) != 2:
                continue
            q = [pin_at(px, py, float(x), float(y), int(rot), mirror.strip()[8:9])
                 for _, px, py in p]
            key = (round(q[0][0], 2), round(q[0][1], 2), round(q[1][0], 2), round(q[1][1], 2))
            if key in ends:
                ref = re.search(r'\(property "Reference" "([^"]*)"', blk)
                bridged.append((name, ref.group(1) if ref else "?", lib_id))
    return bridged, offgrid, no_fp, parts


def check_netlist(cli, root):
    out = os.path.join(tempfile.mkdtemp(), "net.net")
    subprocess.run([cli, "sch", "export", "netlist", "--format", "kicadsexpr", root, "-o", out],
                   capture_output=True, check=True)
    with open(out, encoding="utf8") as f:
        text = f.read()
    at = text.index("(nets")
    stubs = []
    total = 0
    for b in re.findall(r"\(net\b(.*?)(?=\n\t\t\(net\b|\n\t\)\s*\)\s*$)", text[at:], re.S):
        total += 1
        name = re.search(r'\(name "([^"]*)"\)', b).group(1)
        nodes = sorted(set(re.findall(r'\(ref "([^"]+)"\)\s*\(pin "([^"]+)"\)', b)))
        if len(nodes) == 1 and not name.startswith("unconnected-"):
            stubs.append((name, "%s pin %s" % nodes[0]))
    return stubs, total


def run_erc(cli, root):
    rpt = os.path.join(tempfile.mkdtemp(), "erc.rpt")
    subprocess.run([cli, "sch", "erc", "--severity-error", "--severity-warning", root, "-o", rpt],
                   capture_output=True)
    with open(rpt, encoding="utf8", errors="replace") as f:
        text = f.read()
    kinds = re.findall(r"^\[(\w+)\]", text, re.M)
    return kinds


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    pcb = os.path.join(os.path.dirname(here), "pcb")
    roots = [f for f in os.listdir(pcb) if f.endswith(".kicad_sch")
             and os.path.exists(os.path.join(pcb, f[:-10] + ".kicad_pro"))]
    if len(roots) != 1:
        sys.exit("expected exactly one root schematic in %s, found %d" % (pcb, len(roots)))
    root = os.path.join(pcb, roots[0])
    cli = find_kicad_cli()

    kinds = run_erc(cli, root)
    stubs, nets = check_netlist(cli, root)
    bridged, offgrid, no_fp, parts = check_sheets(pcb)

    print("crossfire schematic -- %s, %d nets, %d parts\n" % (roots[0], nets, parts))
    counts = {}
    for k in kinds:
        counts[k] = counts.get(k, 0) + 1
    print("  rules check                    %d" % len(kinds))
    for k in sorted(counts):
        print("      %-28s %d" % (k, counts[k]))
    print("  pins wired to nothing          %d" % len(stubs))
    for n, who in stubs:
        print("      %-28s %s" % (n, who))
    print("  parts bridged by a wire        %d" % len(bridged))
    for sheet, ref, lib_id in bridged:
        print("      %-12s %-6s %s" % (sheet, ref, lib_id))
    print("  wire ends off the grid         %d" % len(offgrid))
    for sheet, w in offgrid:
        print("      %-12s (%.4f,%.4f)-(%.4f,%.4f)" % ((sheet,) + w))
    print("  parts without a footprint      %d of %d" % (no_fp, parts))

    bad = len(stubs) + len(bridged) + len(offgrid)
    print("\n%s" % ("OK" if not bad else "%d finding(s) the rules check cannot see" % bad))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())

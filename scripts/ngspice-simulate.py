#!/usr/bin/env python3
"""Run every netlist in crates/stave-test/spice/ through ngspice into a
waveform next to it (<name>.ngspice.txt: two columns, the abscissa —
time at 48 kHz, or frequency for an AC analysis — and the value).

First the oracle itself: the circuits with exact answers (rc_step against
1 - exp(-t/RC), rc_lowpass against |1 / (1 + j w RC)|) must match them,
or nothing is written or compared.

    scripts/ngspice-simulate.py            regenerate the committed waveforms
    scripts/ngspice-simulate.py --check    regenerate into a temporary
        directory and fail if any differs from the committed one by more
        than --rtol of its column's peak (print, platform and version noise
        passes; a changed netlist or a stale waveform does not). CI runs
        this with the pinned ngspice of .github/workflows/references.yml.

Drives the shared library when there is one (macOS: brew install
libngspice; NGSPICE_LIBRARY names it), else the ngspice program in batch
mode (Linux: apt install ngspice; NGSPICE names it) — the same simulator
either way. --expect-version fails on any other ngspice version. Only
Python's standard library: ngspice never runs in a test, the tests read
the files.
"""

import argparse
import ctypes
import ctypes.util
import math
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

SPICE_DIR = pathlib.Path(__file__).resolve().parent.parent / "crates/stave-test/spice"
CANDIDATES = [
    "/opt/homebrew/lib/libngspice.dylib",
    "/usr/local/lib/libngspice.dylib",
    "/usr/local/lib/libngspice.so",
    "/usr/lib/x86_64-linux-gnu/libngspice.so.0",
]

PRINT = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_void_p)
EXIT = ctypes.CFUNCTYPE(
    ctypes.c_int, ctypes.c_int, ctypes.c_bool, ctypes.c_bool, ctypes.c_int, ctypes.c_void_p
)

errors = []
output = []


@PRINT
def on_print(text, _id, _user):
    line = text.decode(errors="replace")
    output.append(line)
    if line.startswith("stderr") and "warning" not in line.lower():
        errors.append(line)
    return 0


@EXIT
def on_exit(status, _unload, _quit, _id, _user):
    return 0


class Library:
    """libngspice through ctypes."""

    def __init__(self, path):
        self.where = path
        self.spice = ctypes.CDLL(path)
        self.spice.ngSpice_Init(on_print, None, on_exit, None, None, None, None)
        self.spice.ngSpice_Command.argtypes = [ctypes.c_char_p]

    def version(self):
        output.clear()
        self.spice.ngSpice_Command(b"version")
        return "".join(output)

    def run(self, path):
        errors.clear()
        self.spice.ngSpice_Command(f"source {path}".encode())
        self.spice.ngSpice_Command(b"remcirc")
        return "".join(errors)


class Program:
    """The ngspice program, in batch mode."""

    def __init__(self, path):
        self.where = path

    def version(self):
        return subprocess.run([self.where, "--version"], capture_output=True, text=True).stdout

    def run(self, path):
        done = subprocess.run([self.where, "-b", path], capture_output=True, text=True)
        failed = done.returncode != 0 or re.search(r"^Error", done.stdout + done.stderr, re.M)
        return (done.stdout + done.stderr) if failed else ""


def load():
    for name in filter(None, [os.environ.get("NGSPICE_LIBRARY"), ctypes.util.find_library("ngspice"),
                              *CANDIDATES]):
        try:
            return Library(name)
        except OSError:
            continue
    program = os.environ.get("NGSPICE") or shutil.which("ngspice")
    if program:
        return Program(program)
    sys.exit("ngspice not found (brew install libngspice, apt install ngspice, "
             "or set NGSPICE_LIBRARY or NGSPICE)")


def version(spice):
    found = re.search(r"ngspice-(\S+)", spice.version())
    return found.group(1) if found else "unknown"


def simulate(spice, cir, waveform):
    netlist = cir.read_text().replace("OUTFILE", str(waveform))
    with tempfile.NamedTemporaryFile("w", suffix=".cir", delete=False) as tmp:
        tmp.write(netlist)
    problem = spice.run(tmp.name)
    pathlib.Path(tmp.name).unlink()
    if problem or not waveform.exists():
        sys.exit(f"{cir.name}: " + (problem or "no output written"))


def columns(path):
    rows = [list(map(float, line.split())) for line in path.read_text().splitlines() if line.strip()]
    return rows


# The oracle on exact answers: name -> (closed form of the abscissa, budget, what it measures).
RC = 1e3 * 100e-9
EXACT = {
    "rc_step": (lambda t: 1.0 - math.exp(-t / RC), 2e-5, "max |error|"),
    "rc_lowpass": (
        lambda f: 1.0 / math.sqrt(1.0 + (2.0 * math.pi * f * RC) ** 2),
        1e-9,
        "max |error| in dB",
    ),
}


def check_exact(spice, scratch):
    for name, (exact, budget, metric) in EXACT.items():
        waveform = scratch / f"{name}.exact.txt"
        simulate(spice, SPICE_DIR / f"{name}.cir", waveform)
        rows = columns(waveform)
        if metric.endswith("dB"):
            error = max(abs(20.0 * math.log10(v / exact(x))) for x, v in rows)
        else:
            error = max(abs(v - exact(x)) for x, v in rows)
        verdict = "ok" if error <= budget else "FAILED"
        print(f"exact {name}: {metric} {error:.3e} (budget {budget:.0e}) {verdict}")
        if error > budget:
            sys.exit(f"ngspice is wrong on {name}, a circuit with an exact answer: nothing compared")


def compare(name, committed, fresh, rtol):
    """The largest deviation of any column, over that column's peak in the
    committed waveform, and where it is."""
    a, b = columns(committed), columns(fresh)
    if len(a) != len(b):
        return math.inf, f"{len(b)} rows, committed {len(a)}"
    worst, where = 0.0, ""
    for c in range(len(a[0])):
        peak = max(abs(row[c]) for row in a) or 1.0
        for i, (ra, rb) in enumerate(zip(a, b)):
            d = abs(ra[c] - rb[c]) / peak
            if d > worst:
                worst, where = d, f"row {i + 1}, column {c + 1}: committed {ra[c]:.12e}, now {rb[c]:.12e}"
    return worst, where


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="compare, don't write")
    parser.add_argument("--rtol", type=float, default=2e-5,
                        help="--check: largest deviation, relative to a column's peak (default 2e-5: "
                             "ngspice 42 and 47 differ by up to 5e-6; a 1%% component change moves "
                             "a waveform by 1.6e-3 or more)")
    parser.add_argument("--expect-version", help="fail unless ngspice reports this version")
    args = parser.parse_args()

    spice = load()
    found = version(spice)
    print(f"ngspice {found} ({spice.where})")
    if args.expect_version and found != args.expect_version:
        sys.exit(f"ngspice {found}, expected {args.expect_version}")

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        check_exact(spice, scratch)
        failed = []
        for cir in sorted(SPICE_DIR.glob("*.cir")):
            committed = cir.with_suffix(".ngspice.txt")
            if not args.check:
                simulate(spice, cir, committed)
                print(f"wrote {committed.name} ({len(columns(committed))} points)")
                continue
            fresh = scratch / committed.name
            simulate(spice, cir, fresh)
            if not committed.exists():
                failed.append(committed.name)
                print(f"{committed.name}: not committed")
                continue
            worst, where = compare(cir.stem, committed, fresh, args.rtol)
            verdict = "ok" if worst <= args.rtol else "STALE"
            print(f"{committed.name}: max deviation {worst:.3e} of peak {verdict}"
                  + (f" ({where})" if worst > 0 else ""))
            if worst > args.rtol:
                failed.append(committed.name)
        if failed:
            sys.exit(f"{len(failed)} waveform(s) differ from a fresh simulation by more than "
                     f"{args.rtol:g} of peak: {', '.join(failed)}. Regenerate with "
                     "scripts/ngspice-simulate.py and commit, or find what changed.")


if __name__ == "__main__":
    main()

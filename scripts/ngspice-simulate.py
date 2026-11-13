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

Then the characterisation tables (DSP.md R17): every <name>.plan (written
by stave-test from stave-measure's stimulus suite) runs on <name>.cir into
<name>.table — the operating point (.op), the small-signal response at the
plan's frequencies (.ac, magnitude and phase), and every run's transient
(the input source split into its DC level and a behavioural source
carrying the plan's excitation), reduced to the complex amplitudes of the
plan's bins over whole periods, THD+N and the inharmonic floor: measurements,
never waveforms. --check compares them the same way.

Drives the shared library when there is one (macOS: brew install
libngspice; NGSPICE_LIBRARY names it), else the ngspice program in batch
mode (Linux: apt install ngspice; NGSPICE names it) — the same simulator
either way; the characterisation runs use the program, in parallel, when
there is one. --expect-version fails on any other ngspice version. The
waveforms need only Python's standard library; the tables need NumPy
(scripts/reference-requirements.txt). ngspice never runs in a test, the
tests read the files.
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
import threading
import time

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
# How many ngspice programs run at once (--jobs).
SLOTS = threading.BoundedSemaphore(os.cpu_count() or 1)


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

    def run_text(self, path):
        """The run's problem (empty when none) and its printed output."""
        output.clear()
        problem = self.run(path)
        return problem, "".join(output)


class Program:
    """The ngspice program, in batch mode."""

    def __init__(self, path):
        self.where = path

    def version(self):
        return subprocess.run([self.where, "--version"], capture_output=True, text=True).stdout

    def run(self, path):
        return self.run_text(path)[0]

    def run_text(self, path):
        with SLOTS:
            done = subprocess.run([self.where, "-b", path], capture_output=True, text=True)
        failed = done.returncode != 0 or re.search(r"^Error", done.stdout + done.stderr, re.M)
        return ((done.stdout + done.stderr) if failed else ""), done.stdout


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


# ------------------------------------------------------------ characterisation

def program():
    """The ngspice program, for parallel characterisation runs, or None."""
    where = os.environ.get("NGSPICE") or shutil.which("ngspice")
    return Program(where) if where else None


def significant(x):
    """Nine significant digits, as the plan rounds (exact in both)."""
    return float(f"{x:.8e}")


def random_phases(seed, count):
    """stave-measure's Multisine::random_phases: SplitMix64 from `seed`,
    uniform on [0, 2π), bit for bit."""
    state, out, mask = seed, [], (1 << 64) - 1
    for _ in range(count):
        state = (state + 0x9E3779B97F4A7C15) & mask
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & mask
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & mask
        z ^= z >> 31
        out.append(2.0 * math.pi * (z >> 11) / float(1 << 53))
    return out


def parse_excitation(text, bins=()):
    """An excitation's text form; a random-phase multisine's tones are
    regenerated from its RMS and seed on the run's excited bins (≡ 1 mod 4)."""
    w = text.split()
    assert w[0] == "period" and w[2] == "dc" and w[4] == "fade", text
    e = {"period": int(w[1]) / 300.0, "dc": float(w[3]), "fade": float(w[5]), "tones": []}
    if len(w) > 6 and w[6] == "multisine":
        rms, seed = float(w[7]), int(w[8])
        excited = [k for k in bins if k % 4 == 1]
        amplitude = significant(rms * math.sqrt(2.0 / len(excited)))
        for k, phase in zip(excited, random_phases(seed, len(excited))):
            e["tones"].append({"amp": amplitude, "bin": k, "phase": significant(phase),
                               "on": 0.0, "off": math.inf})
        return e
    i = 6
    while i < len(w):
        assert w[i] == "tone", text
        e["tones"].append({"amp": float(w[i + 1]), "bin": int(w[i + 2]), "phase": float(w[i + 3]),
                           "on": float(w[i + 4]), "off": float(w[i + 5])})
        i += 6
    return e


def parse_plan(path):
    plan = {"input": None, "dc": 0.0, "holds": [], "response": [], "runs": []}
    for line in path.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        head, _, rest = line.partition("|")
        w = head.split()
        if w[0] == "input":
            plan["input"], plan["dc"] = w[1], float(w[3])
            i = 4
            while i < len(w) and w[i] == "hold":
                plan["holds"].append((w[i + 1], float(w[i + 2])))
                i += 3
        elif w[0] == "response":
            plan["response"] = [float(x) for x in w[1:]]
        elif w[0] == "run":
            at = w.index("settle")
            bins = [int(x) for x in w[at + 5:]]
            plan["runs"].append({
                "id": w[1], "kind": w[3], "args": w[4:at],
                "settle": int(w[at + 1]), "measure": int(w[at + 3]),
                "bins": bins, "excitation": parse_excitation(rest, bins),
            })
    return plan


def expression(e):
    """The excitation as a B-source expression of time (the plan's
    Excitation::at, term for term)."""
    terms = []
    for t in e["tones"]:
        w = 2.0 * math.pi * t["bin"] / e["period"]
        term = f"{t['amp']!r}*sin({w!r}*time+{t['phase']!r})"
        gate = []
        if t["on"] > 0.0:
            gate.append(f"u(time-{t['on']!r})")
        if t["off"] != math.inf:
            gate.append(f"u({t['off']!r}-time)")
        terms.append("*".join(gate + [f"({term})"]))
    body = " + ".join(terms) if terms else "0"
    if e["fade"] > 0.0:
        body = (f"(time < {e['fade']!r} ? 0.5-0.5*cos({math.pi / e['fade']!r}*time) : 1)"
                f" * ({body})")
    return f"{e['dc']!r} + {body}"


def deck_parts(cir):
    """The deck's circuit lines (analyses, control block and .end taken
    out), its probe expression, and whether it starts from IC (UIC)."""
    lines, probe, uic, in_control = [], None, False, False
    for line in cir.read_text().splitlines():
        low = line.strip().lower()
        if low.startswith(".control"):
            in_control = True
            continue
        if low.startswith(".endc"):
            in_control = False
            continue
        if in_control:
            found = re.match(r"wrdata\s+\S+\s+(vm?\(.*\))", low)
            if found:
                probe = found.group(1).replace("vm(", "v(")
            continue
        if low.startswith(".tran"):
            uic = " uic" in low
            continue
        if low.startswith((".ac", ".op", ".end")):
            continue
        lines.append(line)
    return lines, probe, uic


def split_deck(lines, plan, stimulus):
    """Split the input source: its DC level stays, `stimulus(kind, a, b)`
    (the driven element's line) carries the rest — in series for a
    voltage, in parallel for a current; held sources go to DC."""
    out, found = [], False
    holds = dict(plan["holds"])
    for line in lines:
        w = line.split()
        name = w[0].lower() if w else ""
        if name == plan["input"]:
            found = True
            a, b = w[1], w[2]
            if name.startswith("v"):
                out.append(f"{w[0]} {a} char_x DC {plan['dc']!r}")
                out.append(stimulus("v", "char_x", b))
            else:
                out.append(f"{w[0]} {a} {b} DC {plan['dc']!r}")
                out.append(stimulus("i", a, b))
        elif name in holds:
            out.append(f"{w[0]} {w[1]} {w[2]} DC {holds[name]!r}")
        else:
            out.append(line)
    if not found:
        sys.exit(f"no input source {plan['input']} in the deck")
    return out


def continued(line, width=200):
    """A long element line as SPICE continuation lines."""
    if len(line) <= width:
        return line
    out, current = [], ""
    for word in line.split(" "):
        if len(current) + len(word) + 1 > width and current:
            out.append(current)
            current = "+ " + word
        else:
            current = f"{current} {word}" if current else word
    out.append(current)
    return "\n".join(out)


def reference_rate(run):
    """48 kHz × 2^j: at least 384 kHz, 128 points a cycle of the highest
    read bin up to 24 kHz, and 4 a cycle above it."""
    top = max(run["bins"]) / run["excitation"]["period"]
    need = max(384e3, 128 * min(top, 24e3), 4 * top)
    rate = 48e3
    while rate < need:
        rate *= 2
    return rate


def run_deck(spice, text, scratch, tag):
    path = scratch / f"{tag}.cir"
    path.write_text(text)
    problem, printed = spice.run_text(str(path))
    if problem:
        sys.exit(f"{tag}: {problem}")
    return printed


def simulate_run(spice, cir, plan, run, scratch):
    """One run's transient, reduced to its measurements: per measured
    period the complex amplitude of each bin, steady state against the
    period before, and for a tone THD+N and the inharmonic floor."""
    import numpy as np

    lines, probe, uic = deck_parts(cir)
    e = run["excitation"]
    expr = expression(e)

    def stimulus(kind, a, b):
        return continued(f"BCHAR {a} {b} {'V' if kind == 'v' else 'I'} = {{{expr}}}")

    period = e["period"]
    rate = reference_rate(run)
    step = 1.0 / rate
    n = round(period * rate)
    start = (run["settle"] - 1) * period
    stop = (run["settle"] + run["measure"]) * period
    tag = f"{cir.stem}.{run['id'].replace('/', '_')}"
    out = scratch / f"{tag}.txt"
    deck = "\n".join([f"* {cir.stem} {run['id']}"] + split_deck(lines, plan, stimulus) + [
        ".options interp",
        ".control",
        "set wr_singlescale",
        "set numdgt=15",
        # ngspice writes from one step after tstart: start a step early.
        f"tran {step!r} {stop!r} {max(start - step, 0.0)!r} {step!r}" + (" uic" if uic else ""),
        f"wrdata {out} {probe}",
        ".endc",
        ".end",
    ]) + "\n"
    run_deck(spice, deck, scratch, tag)
    data = np.loadtxt(out)
    out.unlink()
    index = np.rint((data[:, 0] - start) / step).astype(int)
    total = (run["measure"] + 1) * n
    x = np.full(total, np.nan)
    keep = (index >= 0) & (index < total)
    x[index[keep]] = data[keep, 1]
    if np.isnan(x).any():
        missing = np.flatnonzero(np.isnan(x))
        sys.exit(f"{cir.stem} {run['id']}: ngspice's output misses {len(missing)} grid points "
                 f"(first {missing[:3]} of {total}; it wrote {data[:2, 0]} … {data[-2:, 0]}, step {step})")
    periods = x.reshape(run["measure"] + 1, n)
    k = np.arange(n)
    rows = []
    for j in range(1, run["measure"] + 1):
        p = periods[j]
        for b in run["bins"]:
            c = (p * np.exp(-2j * np.pi * ((b * k) % n) / n)).sum() * (1.0 if b == 0 else 2.0) / n
            rows.append((j - 1, b, c))
    last, before = periods[1], periods[0]
    steady = math.sqrt(((last - before) ** 2).sum() / max(((last - last.mean()) ** 2).sum(), 1e-300))
    thdn = floor = None
    if run["kind"] == "tone":
        fundamental = int(run["args"][0])
        spectrum = np.abs(np.fft.rfft(last)) ** 2
        bins = np.arange(len(spectrum))
        band = (bins / period >= 20.0) & (bins / period <= 20e3) & (2 * bins < n)
        power = spectrum[fundamental]
        thdn = math.sqrt(spectrum[band & (bins != fundamental)].sum() / power)
        floor = math.sqrt(spectrum[band & (bins % fundamental != 0)].sum() / power)
    return run["id"], steady, thdn, floor, rows


def characterise(cir, plan_path, table, jobs, library):
    """Simulate a plan on its deck into a table."""
    plan = parse_plan(plan_path)
    lines, probe, uic = deck_parts(cir)
    spice = program() or library
    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        body = []
        if not uic:
            def quiet(kind, a, b):
                return f"{kind.upper()}CHAR {a} {b} DC 0 AC 1"
            ac = scratch / "ac.txt"
            control = [".control", "set numdgt=15", "op", "print all", "set wr_singlescale",
                       "set appendwrite"]
            for hz in plan["response"]:
                control += [f"ac lin 1 {hz!r} {hz!r}", f"wrdata {ac} {probe}"]
            text = "\n".join([f"* {cir.stem} op and ac"] + split_deck(lines, plan, quiet)
                             + control + [".endc", ".end"]) + "\n"
            printed = run_deck(spice, text, scratch, f"{cir.stem}.op")
            for line in printed.splitlines():
                found = re.match(r"^([a-z0-9_.]+) = (\S+)$", line.strip().lower())
                if found:
                    body.append(f"op {found.group(1)} {float(found.group(2)):.9e}")
            if plan["response"]:
                for row in columns(ac):
                    body.append(f"ac {row[0]:.9e} {row[1]:.9e} {row[2]:.9e}")
        runs = plan["runs"]
        if isinstance(spice, Program) and jobs > 1:
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(jobs) as pool:
                results = list(pool.map(lambda r: simulate_run(spice, cir, plan, r, scratch), runs))
        else:
            results = [simulate_run(spice, cir, plan, r, scratch) for r in runs]

        def fmt(x):
            return "-" if x is None else f"{x:.9e}"

        for rid, steady, thdn, floor, rows in sorted(results, key=lambda r: r[0]):
            body.append(f"run {rid} steady {fmt(steady)} thdn {fmt(thdn)} floor {fmt(floor)}")
            last = 0
            for j, b, c in rows:
                if j != last:
                    body.append(f"period {j}")
                    last = j
                body.append(f"{b} {c.real:.7e} {c.imag:.7e}")
    head = [f"# stave characterisation table v1: {cir.stem}",
            f"# source: ngspice {version(spice)}, spice/{cir.name} through spice/{plan_path.name}"]
    table.write_text("\n".join(head + body) + "\n")


def table_rows(path):
    rows, run, period = {}, None, "0"
    for line in path.read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        w = line.split()
        if w[0] == "op":
            rows[("op", w[1])] = [float(w[2])]
        elif w[0] == "ac":
            rows[("ac", w[1])] = [float(w[2]), float(w[3])]
        elif w[0] == "run":
            run, period = w[1], "0"
            rows[("run", w[1])] = [float("nan") if v == "-" else float(v) for v in w[3::2]]
        elif w[0] == "period":
            period = w[1]
        else:
            rows[("bin", run, period, w[0])] = [float(w[1]), float(w[2])]
    return rows


def compare_tables(committed, fresh, rtol):
    """The largest deviation, relative to its group's scale — the
    operating point's largest node (at least 1 V), the response's largest
    point, a run's largest bin; steady state, THD+N and the floor are
    already ratios to the signal and compare absolutely."""
    a, b = table_rows(committed), table_rows(fresh)
    if a.keys() != b.keys():
        return math.inf, f"rows differ: {sorted(set(a) ^ set(b))[:3]}"

    def group(key):
        return key[0] if key[0] in ("op", "ac") else ("bin", key[1])

    scale = {}
    for key, v in a.items():
        if key[0] != "run":
            scale[group(key)] = max(scale.get(group(key), 0.0), math.hypot(*v))
    worst, where = 0.0, ""
    for key, v in a.items():
        w = b[key]
        if key[0] == "run":
            for name, x, y in zip(("steady", "thdn", "floor"), v, w):
                if math.isnan(x) and math.isnan(y):
                    continue
                d = abs(x - y)
                if d > worst:
                    worst, where = d, f"{key[1]} {name}: committed {x:.9e}, now {y:.9e}"
            continue
        floor = 1.0 if key[0] == "op" else 1e-300
        d = math.dist(v, w) / max(scale[group(key)], floor)
        if d > worst:
            worst, where = d, f"{' '.join(key)}: committed {v}, now {w}"
    return worst, where


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="compare, don't write")
    parser.add_argument("--rtol", type=float, default=2e-5,
                        help="--check: largest deviation, relative to a column's peak (default 2e-5: "
                             "ngspice 42 and 47 differ by up to 5e-6; a 1%% component change moves "
                             "a waveform by 1.6e-3 or more)")
    parser.add_argument("--expect-version", help="fail unless ngspice reports this version")
    parser.add_argument("--only", help="only these decks (comma-separated names)")
    parser.add_argument("--tables-only", action="store_true", help="skip the waveforms")
    parser.add_argument("--keep", type=pathlib.Path,
                        help="--check: copy every stale table's fresh simulation here (CI uploads them)")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 1,
                        help="parallel characterisation runs (with the ngspice program)")
    args = parser.parse_args()
    only = set(args.only.split(",")) if args.only else None

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
            if args.tables_only or (only and cir.stem not in only):
                continue
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
        # Every plan at once: the runs of all of them share --jobs ngspice
        # programs, so a slow deck never leaves the others idle.
        global SLOTS
        SLOTS = threading.BoundedSemaphore(args.jobs)
        plans = [p for p in sorted(SPICE_DIR.glob("*.plan")) if not only or p.stem in only]

        def one(plan):
            cir, committed = plan.with_suffix(".cir"), plan.with_suffix(".table")
            started = time.monotonic()
            target = committed if not args.check else scratch / committed.name
            characterise(cir, plan, target, args.jobs, spice)
            return plan, committed, target, time.monotonic() - started

        if program():
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max(len(plans), 1)) as pool:
                done = list(pool.map(one, plans))
        else:
            done = [one(p) for p in plans]
        for plan, committed, fresh, seconds in done:
            if not args.check:
                print(f"wrote {committed.name} ({committed.stat().st_size // 1024} KiB, {seconds:.1f} s)")
                continue
            if not committed.exists():
                failed.append(committed.name)
                print(f"{committed.name}: not committed")
                continue
            worst, where = compare_tables(committed, fresh, args.rtol)
            verdict = "ok" if worst <= args.rtol else "STALE"
            print(f"{committed.name}: max deviation {worst:.3e} {verdict} ({seconds:.1f} s)"
                  + (f" ({where})" if worst > 0 else ""))
            if worst > args.rtol:
                failed.append(committed.name)
                if args.keep:
                    args.keep.mkdir(parents=True, exist_ok=True)
                    shutil.copy(fresh, args.keep / committed.name)
        if failed:
            sys.exit(f"{len(failed)} waveform(s) or table(s) differ from a fresh simulation by more than "
                     f"{args.rtol:g} of peak: {', '.join(failed)}. Regenerate with "
                     "scripts/ngspice-simulate.py and commit, or find what changed.")


if __name__ == "__main__":
    main()

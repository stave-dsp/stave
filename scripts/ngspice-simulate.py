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

Then the DC transfer sweeps (DSP.md R3): each line of dc_sweeps.list
names a deck, an element (a source, or a resistor for a pot or a trim)
and its range; the deck runs a `.dc` sweep of it, every other
time-varying source held at its value at t = 0 (or the line's `hold`),
into dc_sweeps.ngspice.txt — the operating point at each value, one
block a sweep.

Drives the shared library when there is one (macOS: brew install
libngspice; NGSPICE_LIBRARY names it), else the ngspice program in batch
mode (Linux: apt install ngspice; NGSPICE names it) — the same simulator
either way; the characterisation runs use the program, in parallel, when
there is one. --expect-version fails on any other ngspice version. The
waveforms need only Python's standard library; the tables need NumPy
(scripts/reference-requirements.txt). ngspice never runs in a test, the
tests read the files.

CI splits the check across machines (DSP.md R2): --shard i/n takes the
i-th of n groups of decks (a deck's waveform and table together), balanced
by each deck's measured ngspice time in spice/costs.tsv (--costs-out
writes a run's, to merge into it; a deck not listed counts as the median
of its kind). --shard-of n prints every deck's group and runs nothing.
--stamps DIR records each waveform and table that passes --check under a
key of everything its result depends on (this script, the ngspice and
NumPy versions, --rtol, the netlist, the plan, the committed file);
--skip-stamped skips one whose key is already there, unchanged since a
passing check, without simulating it again.
"""

import argparse
import ctypes
import ctypes.util
import hashlib
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
def default_jobs():
    """The offload slot's share of the cores when run through offload.sh, else all of them."""
    for key in ("STAVE_OFFLOAD_JOBS", "RAYON_NUM_THREADS"):
        try:
            return max(1, int(os.environ[key]))
        except (KeyError, ValueError):
            pass
    return os.cpu_count() or 1


SLOTS = threading.BoundedSemaphore(default_jobs())

# Seconds of ngspice per deck (its waveform and its table's runs, each
# timed while it holds a slot): --costs-out, the shards' balance.
BUSY = {}
BUSY_LOCK = threading.Lock()


def charge(deck, seconds):
    if deck:
        with BUSY_LOCK:
            BUSY[deck] = BUSY.get(deck, 0.0) + seconds


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


def no_operating_point(log):
    """When gmin and source stepping both fail, ngspice falls back to a
    pseudo-transient "operating point" and runs on from it without an
    error: a state no DC solution has (charged coupling caps, volts on a
    node with a resistor to ground), which no stave run starts from. That
    is a failed deck, not a reference."""
    if "source stepping failed" in log:
        return ("no DC operating point (gmin and source stepping failed; ngspice fell back to a "
                "transient op): loosen reltol/vntol or add a .nodeset")
    return ""


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

    def run(self, path, deck=None):
        errors.clear()
        output.clear()
        started = time.monotonic()
        self.spice.ngSpice_Command(f"source {path}".encode())
        self.spice.ngSpice_Command(b"remcirc")
        charge(deck, time.monotonic() - started)
        return "".join(errors) or no_operating_point("".join(output))

    def run_text(self, path, deck=None):
        """The run's problem (empty when none) and its printed output."""
        output.clear()
        problem = self.run(path, deck)
        return problem, "".join(output)


class Program:
    """The ngspice program, in batch mode."""

    def __init__(self, path):
        self.where = path

    def version(self):
        return subprocess.run([self.where, "--version"], capture_output=True, text=True).stdout

    def run(self, path, deck=None):
        return self.run_text(path, deck)[0]

    def run_text(self, path, deck=None):
        with SLOTS:
            started = time.monotonic()
            done = subprocess.run([self.where, "-b", path], capture_output=True, text=True)
            charge(deck, time.monotonic() - started)
        failed = done.returncode != 0 or re.search(r"^Error", done.stdout + done.stderr, re.M)
        log = done.stdout + done.stderr
        return (log if failed else no_operating_point(log)), done.stdout


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
    problem = spice.run(tmp.name, cir.stem)
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
        # The deck's analyses go (the plan's replace them); its .options
        # (tolerances, temp, gmin) stay: ".op" is a prefix of ".options".
        if re.match(r"\.(ac|op|end)\b", low):
            continue
        lines.append(line)
    return lines, probe, uic


def transient_tolerances(lines):
    """The deck's lines with the tolerances taken out of its .options
    (reltol, abstol, vntol, chgtol), the rest — temp, tnom, gmin — kept.
    The plan's transients run on a fixed step at the reference rate, so
    the step sets their accuracy: ngspice's default tolerances agree
    with a deck's 1e-9 to 1.3e-8 of peak (the Fuzz Face at +12 dB re
    its knee) in an eighth of the time. The operating point and the
    small-signal response keep the deck's own."""
    out = []
    for line in lines:
        if re.match(r"\s*\.opt", line, re.I):
            line = re.sub(r"\s+(reltol|abstol|vntol|chgtol)\s*=\s*\S+", "", line, flags=re.I)
        out.append(line)
    return out


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


def run_deck(spice, text, scratch, tag, deck):
    path = scratch / f"{tag}.cir"
    path.write_text(text)
    problem, printed = spice.run_text(str(path), deck)
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
    deck = "\n".join([f"* {cir.stem} {run['id']}"] + split_deck(transient_tolerances(lines), plan, stimulus) + [
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
    run_deck(spice, deck, scratch, tag, cir.stem)
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
            printed = run_deck(spice, text, scratch, f"{cir.stem}.op", cir.stem)
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


# ------------------------------------------------------------ DC sweeps

SWEEPS = SPICE_DIR / "dc_sweeps.list"
SWEPT = SPICE_DIR / "dc_sweeps.ngspice.txt"


def parse_sweeps():
    """dc_sweeps.list: `sweep <id> <deck> <element> <start> <stop> <points>
    [hold <source>=<value> …] probe <vector> …`."""
    sweeps = []
    for line in SWEEPS.read_text().splitlines():
        w = line.split()
        if not w or w[0].startswith("#"):
            continue
        assert w[0] == "sweep", line
        sweep = {"id": w[1], "deck": w[2], "element": w[3].lower(), "start": float(w[4]),
                 "stop": float(w[5]), "points": int(w[6]), "holds": {}, "probes": []}
        i = 7
        while w[i] == "hold":
            name, value = w[i + 1].split("=")
            sweep["holds"][name.lower()] = float(value)
            i += 2
        assert w[i] == "probe", line
        sweep["probes"] = w[i + 1:]
        sweeps.append(sweep)
    return sweeps


SUFFIX = {"t": 1e12, "g": 1e9, "meg": 1e6, "k": 1e3, "m": 1e-3, "u": 1e-6, "n": 1e-9, "p": 1e-12, "f": 1e-15}


def spice_number(text):
    found = re.match(r"^([-+]?[0-9.]+(?:e[-+]?[0-9]+)?)(meg|[tgkmunpf])?", text.lower())
    return float(found.group(1)) * SUFFIX.get(found.group(2), 1.0)


def at_rest(line):
    """A source's value at t = 0 as ngspice's transient starts it: SIN's
    offset plus its amplitude times the sine of its phase, PWL's first
    value; None for a source that is already DC."""
    found = re.search(r"\b(sin|pwl)\s*\(([^)]*)\)", line, re.I)
    if not found:
        return None
    args = [spice_number(x) for x in found.group(2).replace(",", " ").split()]
    if found.group(1).lower() == "pwl":
        return args[1]
    phase = args[5] if len(args) > 5 else 0.0
    return args[0] + args[1] * math.sin(math.radians(phase))


def sweep_deck(sweep, out):
    """The deck as a `.dc` sweep of its element, its other sources at rest."""
    lines, _, _ = deck_parts(SPICE_DIR / f"{sweep['deck']}.cir")
    deck, found = [f"* {sweep['deck']} {sweep['id']}"], False
    for line in lines:
        w = line.split()
        name = w[0].lower() if w else ""
        if name == sweep["element"]:
            found = True
            if name[0] in "vi":
                line = f"{w[0]} {w[1]} {w[2]} DC 0"
        elif name[:1] in ("v", "i") and len(w) > 3:
            if name in sweep["holds"]:
                line = f"{w[0]} {w[1]} {w[2]} DC {sweep['holds'][name]!r}"
            elif (rest := at_rest(line)) is not None:
                line = f"{w[0]} {w[1]} {w[2]} DC {rest!r}"
        deck.append(line)
    if not found:
        sys.exit(f"{sweep['id']}: no element {sweep['element']} in spice/{sweep['deck']}.cir")
    step = (sweep["stop"] - sweep["start"]) / (sweep["points"] - 1)
    deck += [".control", "set numdgt=15", "set wr_singlescale",
             f"dc {sweep['element']} {sweep['start']!r} {sweep['stop'] + 0.25 * step!r} {step!r}",
             f"wrdata {out} {' '.join(sweep['probes'])}", ".endc", ".end"]
    return "\n".join(deck) + "\n"


def simulate_sweeps(spice, target, only):
    """Every sweep (or those on the decks `only`) into `target`, keeping
    the committed blocks of the others; the ids simulated."""
    blocks, current = {}, None
    if SWEPT.exists():
        for line in SWEPT.read_text().splitlines():
            if line.startswith("sweep "):
                current = line.split()[1]
                blocks[current] = [line]
            elif current and line and not line.startswith("#"):
                blocks[current].append(line)
    every = parse_sweeps()
    sweeps = [s for s in every if not only or s["deck"] in only]
    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)

        def one(sweep):
            tag = sweep["id"].replace("/", "_")
            out = scratch / f"{tag}.txt"
            run_deck(spice, sweep_deck(sweep, out), scratch, tag, sweep["deck"])
            head = f"sweep {sweep['id']} {sweep['deck']} {sweep['element']} {' '.join(sweep['probes'])}"
            return sweep["id"], [head] + [" ".join(f"{x:.12e}" for x in row) for row in columns(out)]

        if program():
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max(len(sweeps), 1)) as pool:
                fresh = dict(pool.map(one, sweeps))
        else:
            fresh = dict(one(s) for s in sweeps)
    blocks.update(fresh)
    text = [f"# stave DC sweeps v1 (DSP.md R3): spice/{SWEEPS.name} through ngspice {version(spice)}",
            "# sweep <id> <deck> <element> <probes…>, then <value> <probe volts…> a point"]
    for sweep in every:
        text += blocks.get(sweep["id"], [])
    target.write_text("\n".join(text) + "\n")
    return list(fresh)


def sweep_rows(path):
    rows, current = {}, None
    for line in path.read_text().splitlines():
        if line.startswith("sweep "):
            current = line.split()[1]
            rows[current] = []
        elif current and line and not line.startswith("#"):
            rows[current].append(list(map(float, line.split())))
    return rows


def compare_sweeps(committed, fresh, ids, rtol):
    """The largest deviation of a fresh sweep's column from the committed
    one, over that column's peak, and where."""
    a, b = sweep_rows(committed), sweep_rows(fresh)
    worst, where = 0.0, ""
    for sid in ids:
        if sid not in a or len(a[sid]) != len(b[sid]):
            return math.inf, f"{sid}: not committed, or its points differ"
        for c in range(len(a[sid][0])):
            peak = max(abs(r[c]) for r in a[sid]) or 1.0
            for ra, rb in zip(a[sid], b[sid]):
                d = abs(ra[c] - rb[c]) / peak
                if d > worst:
                    worst, where = d, f"{sid} at {ra[0]:.6g}, column {c + 1}: {ra[c]:.12e} vs {rb[c]:.12e}"
    return worst, where


# ------------------------------------------------------------ shards and stamps

COSTS = SPICE_DIR / "costs.tsv"


def read_costs(path):
    """deck -> seconds of ngspice, from a costs file (deck<TAB>seconds)."""
    costs = {}
    if path.exists():
        for line in path.read_text().splitlines():
            w = line.split()
            if len(w) >= 2 and not line.startswith("#"):
                costs[w[0]] = float(w[1])
    return costs


def shards(n, costs):
    """deck -> its group of n (1-based): longest first onto the lightest
    group (ties to the lower number), so the groups' ngspice time is as
    even as whole decks allow. A deck the costs don't list counts as the
    median of the listed decks of its kind (with a plan, or a waveform
    only)."""
    stems = sorted(c.stem for c in SPICE_DIR.glob("*.cir"))
    planned = {p.stem for p in SPICE_DIR.glob("*.plan")}

    def median(kind):
        xs = sorted(costs[s] for s in stems if s in costs and (s in planned) == kind)
        return xs[len(xs) // 2] if xs else (300.0 if kind else 1.0)

    guess = {True: median(True), False: median(False)}
    cost = {s: costs.get(s, guess[s in planned]) for s in stems}
    load, group = [0.0] * n, {}
    for s in sorted(stems, key=lambda s: (-cost[s], s)):
        k = min(range(n), key=lambda k: (load[k], k))
        load[k] += cost[s]
        group[s] = k + 1
    return group, load


def parse_shard(text):
    i, _, n = text.partition("/")
    i, n = int(i), int(n)
    if not 1 <= i <= n:
        sys.exit(f"--shard {text}: want i/n with 1 <= i <= n")
    return i, n


def digest(*parts):
    h = hashlib.sha256()
    for part in parts:
        data = part if isinstance(part, bytes) else str(part).encode()
        h.update(len(data).to_bytes(8, "little"))
        h.update(data)
    return h.hexdigest()


def stamp_key(common, kind, *paths):
    """A check's stamp: everything its verdict depends on — `common` (this
    script, ngspice's version, --rtol, Python's and NumPy's), the kind, and
    each file's name and bytes."""
    return f"{kind}-{paths[0].stem}-" + digest(
        common, kind, *(p.name.encode() + b"\0" + p.read_bytes() for p in paths))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="compare, don't write")
    parser.add_argument("--rtol", type=float, default=2e-5,
                        help="--check: largest deviation, relative to a column's peak (default 2e-5: "
                             "ngspice 42 and 47 differ by up to 5e-6; a 1%% component change moves "
                             "a waveform by 1.6e-3 or more)")
    parser.add_argument("--expect-version", help="fail unless ngspice reports this version")
    parser.add_argument("--only", help="only these decks (comma-separated names)")
    parser.add_argument("--exclude", help="every deck but these (comma-separated names)")
    parser.add_argument("--shard", help="i/n: only the i-th of n groups of decks, balanced by "
                        "spice/costs.tsv (CI's references shards)")
    parser.add_argument("--shard-of", type=int, metavar="N",
                        help="print each deck's group of N (deck<TAB>group); run nothing")
    parser.add_argument("--costs", type=pathlib.Path, default=COSTS,
                        help="the decks' measured ngspice seconds (default spice/costs.tsv)")
    parser.add_argument("--costs-out", type=pathlib.Path,
                        help="write the seconds of ngspice each simulated deck took here")
    parser.add_argument("--stamps", type=pathlib.Path,
                        help="--check: record each passing waveform and table here")
    parser.add_argument("--skip-stamped", action="store_true",
                        help="--check: skip a waveform or table whose stamp is in --stamps")
    parser.add_argument("--tables-only", action="store_true", help="skip the waveforms")
    parser.add_argument("--sweeps-only", action="store_true", help="only the DC sweeps")
    parser.add_argument("--keep", type=pathlib.Path,
                        help="--check: copy every stale table's fresh simulation here (CI uploads them)")
    parser.add_argument("--jobs", type=int, default=default_jobs(),
                        help="parallel ngspice runs (with the ngspice program)")
    args = parser.parse_args()
    only = set(args.only.split(",")) if args.only else None
    exclude = set(args.exclude.split(",")) if args.exclude else set()
    costs = read_costs(args.costs)

    if args.shard_of:
        group, weight = shards(args.shard_of, costs)
        for stem in sorted(group):
            print(f"{stem}\t{group[stem]}")
        for k, seconds in enumerate(weight):
            print(f"# group {k + 1}: {seconds:.0f} s of ngspice", file=sys.stderr)
        return
    shard = None
    if args.shard:
        i, n = parse_shard(args.shard)
        group, weight = shards(n, costs)
        shard = {s for s, k in group.items() if k == i}
        print(f"shard {i}/{n}: {len(shard)} deck(s), about {weight[i - 1]:.0f} s of ngspice "
              f"({', '.join(sorted(shard))})")

    def chosen(stem):
        return (not only or stem in only) and stem not in exclude and (shard is None or stem in shard)

    if args.skip_stamped and not args.stamps:
        sys.exit("--skip-stamped needs --stamps")
    if args.stamps and not args.check:
        sys.exit("--stamps records passing checks: it needs --check")

    spice = load()
    found = version(spice)
    print(f"ngspice {found} ({spice.where})", flush=True)
    if args.expect_version and found != args.expect_version:
        sys.exit(f"ngspice {found}, expected {args.expect_version}")
    common = digest(pathlib.Path(__file__).read_bytes(), spice.version(), repr(args.rtol),
                    sys.version_info[:2])
    stamped = []

    def skip(key):
        if args.skip_stamped and (args.stamps / key).exists():
            stamped.append(key)
            return True
        return False

    def record(key):
        if args.stamps:
            args.stamps.mkdir(parents=True, exist_ok=True)
            (args.stamps / key).write_text("")

    global SLOTS
    SLOTS = threading.BoundedSemaphore(args.jobs)
    parallel = program() is not None

    def pool_map(fn, items):
        if parallel and len(items) > 1:
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(len(items)) as pool:
                return list(pool.map(fn, items))
        return [fn(x) for x in items]

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        check_exact(spice, scratch)
        BUSY.clear()  # the oracle's runs are every shard's, no deck's
        failed = []
        waves = []
        for cir in sorted(SPICE_DIR.glob("*.cir")):
            if args.tables_only or args.sweeps_only or not chosen(cir.stem):
                continue
            committed = cir.with_suffix(".ngspice.txt")
            key = None
            if args.check:
                if not committed.exists():
                    failed.append(committed.name)
                    print(f"{committed.name}: not committed")
                    continue
                key = stamp_key(common, "waveform", cir, committed)
                if skip(key):
                    continue
            waves.append((cir, committed, key))

        # The waveforms side by side when the ngspice program is there
        # (each is one run of it; the shared library runs one at a time).
        spice_waves = program() or spice

        def wave(item):
            cir, committed, key = item
            target = committed if not args.check else scratch / committed.name
            simulate(spice_waves, cir, target)
            return item, target

        for (cir, committed, key), fresh in pool_map(wave, waves):
            if not args.check:
                print(f"wrote {committed.name} ({len(columns(committed))} points)")
                continue
            worst, where = compare(cir.stem, committed, fresh, args.rtol)
            verdict = "ok" if worst <= args.rtol else "STALE"
            print(f"{committed.name}: max deviation {worst:.3e} of peak {verdict}"
                  + (f" ({where})" if worst > 0 else ""))
            if worst > args.rtol:
                failed.append(committed.name)
            else:
                record(key)
        sys.stdout.flush()

        # The DC sweeps of the chosen decks (all in one committed file,
        # each deck's sweeps stamped together).
        if SWEEPS.exists() and not args.tables_only:
            swept, keys = set(), {}
            for deck in sorted({sw["deck"] for sw in parse_sweeps() if chosen(sw["deck"])}):
                if args.check:
                    if not SWEPT.exists():
                        swept.add(deck)
                        continue
                    keys[deck] = stamp_key(common, "sweeps", SPICE_DIR / f"{deck}.cir", SWEEPS, SWEPT)
                    if skip(keys[deck]):
                        continue
                swept.add(deck)
            if swept:
                target = scratch / SWEPT.name if args.check else SWEPT
                ids = simulate_sweeps(spice, target, swept)
                if not args.check:
                    print(f"wrote {SWEPT.name} ({len(ids)} sweeps)")
                elif ids:
                    worst, where = compare_sweeps(SWEPT, target, ids, args.rtol) if SWEPT.exists() \
                        else (math.inf, "not committed")
                    verdict = "ok" if worst <= args.rtol else "STALE"
                    print(f"{SWEPT.name}: {len(ids)} sweeps, max deviation {worst:.3e} of peak {verdict}"
                          + (f" ({where})" if worst > 0 else ""))
                    if worst > args.rtol:
                        failed.append(SWEPT.name)
                        if args.keep:
                            args.keep.mkdir(parents=True, exist_ok=True)
                            shutil.copy(target, args.keep / SWEPT.name)
                    else:
                        for deck in swept:
                            if deck in keys:
                                record(keys[deck])

        # Every plan at once: the runs of all of them share --jobs ngspice
        # programs, so a slow deck never leaves the others idle.
        plans = []
        for plan in sorted(SPICE_DIR.glob("*.plan")):
            if args.sweeps_only or not chosen(plan.stem):
                continue
            cir, committed = plan.with_suffix(".cir"), plan.with_suffix(".table")
            key = None
            if args.check:
                if not committed.exists():
                    failed.append(committed.name)
                    print(f"{committed.name}: not committed")
                    continue
                import numpy
                key = stamp_key(common + numpy.__version__, "table", cir, plan, committed)
                if skip(key):
                    continue
            plans.append((plan, key))

        def one(item):
            plan, key = item
            cir, committed = plan.with_suffix(".cir"), plan.with_suffix(".table")
            started = time.monotonic()
            target = committed if not args.check else scratch / committed.name
            characterise(cir, plan, target, args.jobs, spice)
            return plan, key, committed, target, time.monotonic() - started

        for plan, key, committed, fresh, seconds in pool_map(one, plans):
            if not args.check:
                print(f"wrote {committed.name} ({committed.stat().st_size // 1024} KiB, {seconds:.1f} s)")
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
            else:
                record(key)
        if stamped:
            kinds = [k.split("-")[0] for k in stamped]
            print(f"unchanged since a passing check (stamped in {args.stamps}), not simulated: "
                  f"{kinds.count('waveform')} waveform(s), {kinds.count('table')} table(s)")
        if BUSY:
            print("seconds of ngspice: " + ", ".join(f"{d} {s:.0f}" for d, s in
                                                   sorted(BUSY.items(), key=lambda x: -x[1])))
        if args.costs_out:
            args.costs_out.parent.mkdir(parents=True, exist_ok=True)
            args.costs_out.write_text("".join(f"{d}\t{s:.1f}\n" for d, s in sorted(BUSY.items())))
        if failed:
            sys.exit(f"{len(failed)} waveform(s) or table(s) differ from a fresh simulation by more than "
                     f"{args.rtol:g} of peak: {', '.join(failed)}. Regenerate with "
                     "scripts/ngspice-simulate.py and commit, or find what changed.")


if __name__ == "__main__":
    main()

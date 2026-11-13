#!/usr/bin/env python3
"""Reference magnetization for the Jiles-Atherton core: the J-A ODE
dM/dt = chi(M, H, sign dH/dt) * dH/dt integrated by SciPy (DOP853,
rtol 1e-12) for H(t) = 5000 sin(2 pi 50 t), half-cycle by half-cycle so
each turning point (where the branch direction flips) is a segment
boundary. Written from the equations, independently of the Rust core.
Output: crates/stave-test/reference/jiles_atherton.txt, time and M
(A/m) at 48 kHz over two cycles.

Also the transformer on that core (N 2000 turns, A 6 cm², le 10 cm,
Rs 1 kOhm, 400 V at 50 Hz, secondary unloaded): state (i, M) with
di/dt = e / (N A mu0 (N/le) (1 + chi)), dM/dt = chi (N/le) di/dt,
e = v - Rs i; the branch direction is sign(e), restarted at each zero of
e. Output: reference/transformer.txt, time and the primary EMF (V).
Offline only; tests read the files.

    scripts/ja-reference.py           regenerate both files
    scripts/ja-reference.py --check   solve again and fail if either file
        differs by more than --rtol of its peak (CI runs this)

Dependencies, pinned in scripts/reference-requirements.txt (pip install -r
it): NumPy and SciPy. Deterministic: the same versions write the same
bytes; across platforms the solutions agree far inside --rtol.
"""
import argparse
import pathlib
import sys

import numpy as np
from scipy.integrate import solve_ivp

MS, A, ALPHA, K, C = 1.6e6, 1100.0, 1.6e-3, 400.0, 0.2
PEAK, HZ, RATE, CYCLES = 5000.0, 50.0, 48_000.0, 2


def langevin(x):
    return x / 3.0 if abs(x) < 1e-4 else 1.0 / np.tanh(x) - 1.0 / x


def langevin_slope(x):
    return 1.0 / 3.0 - x * x / 15.0 if abs(x) < 1e-4 else 1.0 / (x * x) - 1.0 / np.sinh(x) ** 2


def chi(m, h, direction):
    x = (h + ALPHA * m) / A
    man = MS * langevin(x)
    dman = MS / A * langevin_slope(x)
    lag = man - m
    irr = (1 - C) * lag / ((1 - C) * direction * K - ALPHA * lag) if lag * direction > 0 else 0.0
    return (irr + C * dman) / (1 - ALPHA * C * dman)


def field(t):
    return PEAK * np.sin(2 * np.pi * HZ * t)


def field_rate(t):
    return PEAK * 2 * np.pi * HZ * np.cos(2 * np.pi * HZ * t)


def jiles_atherton():
    end = CYCLES / HZ
    samples = np.arange(int(round(end * RATE)) + 1) / RATE
    # Turning points of the sine: quarter periods.
    bounds = [0.0] + [(2 * i + 1) / (4 * HZ) for i in range(int(2 * CYCLES))] + [end]
    m, out = 0.0, {}
    for t0, t1 in zip(bounds, bounds[1:]):
        mid = 0.5 * (t0 + t1)
        direction = 1.0 if field_rate(mid) > 0 else -1.0
        sol = solve_ivp(
            lambda t, y: [chi(y[0], field(t), direction) * field_rate(t)],
            (t0, t1), [m], method="DOP853", rtol=1e-12, atol=1e-6, dense_output=True,
        )
        for t in samples[(samples >= t0) & (samples <= t1)]:
            out[t] = sol.sol(t)[0]
        m = sol.y[0][-1]
    return "jiles_atherton", samples, out


MU0 = 1.25663706212e-6
TURNS, AREA, PATH, RS, VOLTS = 2000.0, 6e-4, 0.1, 1000.0, 400.0


def transformer():
    end = CYCLES / HZ
    samples = np.arange(int(round(end * RATE)) + 1) / RATE
    k_h = TURNS / PATH
    drive = lambda t: VOLTS * np.sin(2 * np.pi * HZ * t)

    def rhs(t, y, direction):
        i, m = y
        e = drive(t) - RS * i
        x = chi(m, k_h * i, direction)
        di = e / (TURNS * AREA * MU0 * k_h * (1 + x))
        return [di, x * k_h * di]

    def emf_zero(t, y, direction):
        return drive(t) - RS * y[0]

    emf_zero.terminal = True
    t0, y0, out = 0.0, [0.0, 0.0], {}
    direction = 1.0
    while t0 < end:
        # A branch ends when e crosses zero against its own sign.
        emf_zero.direction = -direction
        sol = solve_ivp(rhs, (t0, end), y0, method="DOP853", rtol=1e-12, atol=[1e-12, 1e-6],
                        dense_output=True, events=emf_zero, args=(direction,),
                        first_step=1e-9)
        t1 = sol.t[-1]
        for t in samples[(samples >= t0) & (samples <= t1)]:
            out[t] = drive(t) - RS * sol.sol(t)[0]
        t0, y0, direction = t1, sol.y[:, -1], -direction
        if sol.status == 0:
            break
    return "transformer", samples, out


REFERENCE_DIR = pathlib.Path(__file__).resolve().parent.parent / "crates/stave-test/reference"


def text(samples, out):
    return "".join(f"{t:.12e} {out[t]:.12e}\n" for t in samples)


def deviation(committed, fresh):
    """The largest deviation of either column over its committed peak."""
    a = np.loadtxt(committed.splitlines(), ndmin=2)
    b = np.loadtxt(fresh.splitlines(), ndmin=2)
    if a.shape != b.shape:
        return np.inf
    return float(np.max(np.abs(a - b) / np.maximum(np.max(np.abs(a), axis=0), 1e-300)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="compare, don't write")
    parser.add_argument("--rtol", type=float, default=1e-8,
                        help="--check: largest deviation, relative to a column's peak (default 1e-8: "
                             "macOS arm64 and Linux x86_64 differ by 2e-10; a 1%% parameter change "
                             "moves a solution by 1e-2)")
    args = parser.parse_args()
    stale = []
    for name, samples, out in (jiles_atherton(), transformer()):
        path = REFERENCE_DIR / f"{name}.txt"
        if not args.check:
            path.write_text(text(samples, out))
            print(f"wrote {path.name} ({len(samples)} points)")
            continue
        worst = deviation(path.read_text(), text(samples, out))
        verdict = "ok" if worst <= args.rtol else "STALE"
        print(f"{path.name}: max deviation {worst:.3e} of peak {verdict}")
        if worst > args.rtol:
            stale.append(path.name)
    if stale:
        sys.exit(f"{', '.join(stale)} differ from a fresh solution by more than {args.rtol:g} "
                 "of peak: regenerate with scripts/ja-reference.py and commit, or find what changed.")

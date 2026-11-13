#!/usr/bin/env python3
"""Fit a loudspeaker driver's small-signal parameters to its published
impedance curve (DSP.md E9).

Reads the R23 tables crates/stave-test/datasheet/<driver>_ts.txt (the
sheet's printed Thiele-Small parameters) and <driver>_impedance.txt (its
impedance curve), and fits Re, fs, Qms, Qes and an LR-2 voice coil (Le in
series with L2 || R2) to the curve by least squares on log|Z| at every
published point, holding the sheet's moving mass Mms and piston area Sd
(the impedance cannot see them: only the products the fit moves). Prints
the fitted part as a `ThieleSmall` constant for
crates/stave-dsp/src/physical/speaker/parts.rs and the curve error before
and after.

    scripts/speaker-fit.py jensen_c12n 8     # the 8-ohm version
"""
import math
import pathlib
import sys

import numpy as np
from scipy.optimize import least_squares

ROOT = pathlib.Path(__file__).resolve().parent.parent
SHEETS = ROOT / "crates" / "stave-test" / "datasheet"


def curves(path):
    """`{curve key: [(x, y)]}` of an R23 table."""
    out, key = {}, None
    for line in path.read_text().splitlines():
        if line.startswith("# curve"):
            key = line[len("# curve"):].strip()
            out[key] = []
        elif line.startswith("#") or not line.strip():
            continue
        else:
            x, y = (float(v) for v in line.split())
            out[key].append((x, y))
    return {k: np.array(v) for k, v in out.items()}


def printed(driver, nominal):
    """The sheet's parameters for one nominal impedance, SI units."""
    table = curves(SHEETS / f"{driver}_ts.txt")
    value = {}
    for key, points in table.items():
        name = key.split("=")[0]
        for x, y in points:
            if x == nominal:
                value[name] = y
    scale = {"mms": 1e-3, "cms": 1e-6, "le": 1e-3, "sd": 1e-4, "vas": 1e-3}
    return {k: v * scale.get(k, 1.0) for k, v in value.items()}


def impedance(f, re, fs, qms, qes, le, l2, r2, mms):
    w = 2 * np.pi * f
    ws = 2 * np.pi * fs
    cms = 1 / (ws * ws * mms)
    rms = ws * mms / qms
    bl2 = ws * mms * re / qes
    ze = re + 1j * w * le
    if l2 > 0:
        ze = ze + 1j * w * l2 * r2 / (r2 + 1j * w * l2)
    zm = rms + 1j * w * mms + 1 / (1j * w * cms)
    return ze + bl2 / zm


def main():
    driver, nominal = sys.argv[1], float(sys.argv[2])
    p = printed(driver, nominal)
    curve = curves(SHEETS / f"{driver}_impedance.txt")[f"nominal={nominal:g}"]
    f, z = curve[:, 0], curve[:, 1]
    mms = p["mms"]

    def residual(q):
        re, fs, qms, qes, le, l2, r2 = np.exp(q)
        return np.log(np.abs(impedance(f, re, fs, qms, qes, le, l2, r2, mms)) / z)

    sheet = [p["re"], p["fs"], p["qms"], p["qes"], p["le"], 0.0, 1.0]
    start = [p["re"], p["fs"], p["qms"], p["qes"], 0.5 * p["le"], p["le"], 8.0]
    fit = least_squares(residual, np.log(start), x_scale=1.0)
    re, fs, qms, qes, le, l2, r2 = np.exp(fit.x)
    before = np.abs(np.abs(impedance(f, *sheet, mms)) / z - 1)
    after = np.abs(np.exp(residual(fit.x)) - 1)
    ws = 2 * math.pi * fs
    cms = 1 / (ws * ws * mms)
    bl = math.sqrt(ws * mms * re / qes)
    rms = ws * mms / qms
    print(f"// {driver} {nominal:g} ohm, fitted to its impedance curve by scripts/speaker-fit.py")
    print(f"// fs {fs:.2f} Hz, Qms {qms:.3f}, Qes {qes:.3f}, Qts {qms * qes / (qms + qes):.3f}")
    print(f"// worst |Z| error {before.max():.3f} (sheet, pure Le) -> {after.max():.3f}; rms {np.sqrt(np.mean(before**2)):.3f} -> {np.sqrt(np.mean(after**2)):.3f}")
    print("ThieleSmall {")
    for name, v in [("re", re), ("le", le), ("l2", l2), ("r2", r2), ("bl", bl), ("mms", mms), ("cms", cms), ("rms", rms), ("sd", p["sd"])]:
        print(f"    {name}: {v:.6e},")
    print("}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Fit the vactrol parts of `circuit::devices::photocell` (DSP.md E7) to
their digitised datasheet tables, and print the Rust constants.

The cell is the photoresistor of Najnudel, Müller, Hélie & Roze,
"Power-Balanced Dynamic Modeling of Vactrols: Application to a VTL5C3/2"
(DAFx-23): free holes q+ and electrons q- (coulombs, C+ = C- = 1 F)
recombining through one defect level (Shockley-Read-Hall, eqs. 3-4),

    dq-/dt = g - nu- (q- - q+) q-
    dq+/dt = g - nu+ (q_tau + q+ - q-) q+,

its photoresistance 1/(mu+ q+ + mu- q-) in series with the light
resistance and in parallel with the dark resistance (eqs. 7-8). Light
enters as the carrier generation rate g (amperes), a flow source, as in
Iverson's homogeneous model the paper builds on. The paper's
power-balanced coupling (g = P / (q+ + q-), P the absorbed optical
power) turns a dark cell on as sqrt(t), where the VTL5C3 sheet's
turn-on curves rise roughly as t, and a fit of the cell's rates with it
misses the sheet's four response times by up to 2.6x where the flow source
misses by 1.36x; so the flow source it is.

What is fitted, per part:

- the LED: a Shockley card (IS, N >= 1, RS) to the sheet's input
  characteristic (VTL5C3), or the stock red LED where the sheet prints
  no curve (NSL-32: only 2.5 V maximum at 20 mA);
- the coupling, generation current from LED electrical power PD: at
  every reading of the dark-adapted resistance curve the steady state is
  inverted for the generation that holds it (the paper's eqs. 14-16), and
  ln g is fitted as a smooth minimum of two lines in ln PD,
  ln g = m - w ln(1 + exp(-|l1 - l2| / w)), m = min(l1, l2),
  l = ln gain + slope ln PD. The paper's own law (a sum of two powers of
  PD) bends the other way, and its printed coefficients nearly cancel
  (P0 = -5.47e-5, P1 = 5.63e-5: negative below PD = 56 mW);
- the light-history ratio RLH/RDH (VTL5C3 curve 2 over curve 1, the
  same current): ln ratio = ln_ratio + slope ln(g / g_1mA);
- the cell's rates. VTL5C3: q_tau, nu+, nu- to the sheet's response
  figure, by its four times — turn-on to 63% of the final conductance
  at 40 and 10 mA (3.3 and 5.8 ms on the curves), turn-off to 100 kohm
  from 40 and 10 mA (20.7 and 16.8 ms) — and the resistance 90 ms after
  turn-off from 40 mA (729 kohm), in log time. NSL-32: nu+ and nu- to
  its two printed times (rise to 63% in 5 ms, at 20 mA as assumed;
  decay from 16 mA to 100 kohm in 5 ms) with q_tau, the decay weighted
  three times the rise (the model, like every CdS cell, cannot turn off
  as fast as it turns on: both at 5 ms is out of its reach).
  The mobilities and the light resistance stay the paper's.

    python3 scripts/vactrol-fit.py            # a few minutes; prints the constants
    python3 scripts/vactrol-fit.py nsl32      # one part

Needs numpy and scipy (scripts/reference-requirements.txt).
"""
import math
import os

import numpy as np
from scipy.integrate import solve_ivp
from scipy.optimize import least_squares

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
TABLES = os.path.join(ROOT, "crates", "stave-test", "datasheet")
VT27 = 1.380649e-23 * 300.15 / 1.602176634e-19


def table(name):
    """{label: [(x, y)]} of a datasheet table."""
    curves, cur = {}, None
    for line in open(os.path.join(TABLES, name + ".txt")):
        line = line.strip()
        if line.startswith("# curve "):
            cur = line[8:].strip()
            curves[cur] = []
        elif line and not line.startswith("#"):
            x, y = map(float, line.split())
            curves[cur].append((x, y))
    return curves


class Led:
    def __init__(self, is_, n, rs):
        self.is_, self.n, self.rs = is_, n, rs

    def volts(self, amps):
        return self.n * VT27 * math.log1p(amps / self.is_) + amps * self.rs


class Cell:
    def __init__(self, dark=10e6, light=2.0, defects=0.977, hole_rate=135.0, electron_rate=1.79e8,
                 hole_mobility=4.0, electron_mobility=35.0):
        self.dark, self.light = dark, light
        self.defects, self.hole_rate, self.electron_rate = defects, hole_rate, electron_rate
        self.hole_mobility, self.electron_mobility = hole_mobility, electron_mobility

    def ohms(self, qp, qm):
        g = self.hole_mobility * max(qp, 0.0) + self.electron_mobility * max(qm, 0.0)
        if g <= 0:
            return self.dark
        r = 1.0 / g + self.light
        return self.dark * r / (self.dark + r)

    def steady(self, ohms):
        """The generation current holding `ohms` at rest."""
        s = ohms * self.dark / (self.dark - ohms)
        g = 1.0 / (s - self.light)
        hp, hm = self.hole_mobility, self.electron_mobility

        def f(qp):
            qm = (g - hp * qp) / hm
            d = qm - qp
            return self.hole_rate * (self.defects - d) * qp - self.electron_rate * d * qm

        # f < 0 as q+ -> 0, f > 0 at q+ = q- (d = 0): bisect in log q+.
        lo, hi = math.log(1e-300), math.log(g / (hp + hm))
        for _ in range(200):
            mid = 0.5 * (lo + hi)
            if f(math.exp(mid)) > 0:
                hi = mid
            else:
                lo = mid
        qp = math.exp(0.5 * (lo + hi))
        qm = (g - hp * qp) / hm
        return self.electron_rate * (qm - qp) * qm

    def rhs(self, t, y, gen):
        qp, qm = y
        return [gen - self.hole_rate * (self.defects + qp - qm) * qp,
                gen - self.electron_rate * (qm - qp) * qm]

    def jac(self, t, y, gen):
        qp, qm = y
        hr, er, d = self.hole_rate, self.electron_rate, self.defects
        return [[-hr * (d + 2 * qp - qm), hr * qp], [er * qm, -er * (2 * qm - qp)]]

    def run(self, gen, y0, t_end):
        return solve_ivp(self.rhs, [0, t_end], y0, args=(gen,), method="Radau", jac=self.jac,
                         rtol=1e-9, atol=1e-20, dense_output=True)

    def rest(self, gen):
        return self.run(gen, [0.0, 0.0], 30.0).y[:, -1]


def smooth_min(u, p):
    a1, s1, a2, s2, w = p
    l1, l2 = a1 + s1 * u, a2 + s2 * u
    return np.minimum(l1, l2) - w * np.log1p(np.exp(-np.abs(l1 - l2) / w))


def fit_coupling(cell, led, points):
    u = np.log(np.array([i * led.volts(i) for i, _ in points]))
    lg = np.log(np.array([cell.steady(r) for _, r in points]))
    best = None
    for start in ([50.0, 9.4, -12.0, 1.0, 13.0], [18.0, 3.9, -2.5, 0.87, 1.3], [10.0, 3.0, -5.0, 1.0, 3.0]):
        res = least_squares(lambda p: smooth_min(u, p) - lg, start,
                            bounds=([-200, 0.0, -200, 0.0, 0.05], [200, 20.0, 200, 20.0, 50.0]))
        if best is None or res.cost < best.cost:
            best = res
    return best.x, np.abs(best.fun).max()


def generation(led, p, amps):
    return float(np.exp(smooth_min(math.log(amps * led.volts(amps)), p)))


def crossing(cell, sol, target, rising, t_end):
    ts = np.linspace(0.0, t_end, 40001)
    rs = np.array([cell.ohms(a, b) for a, b in sol.sol(ts).T])
    idx = np.nonzero(rs >= target if rising else rs <= target)[0]
    if len(idx) == 0:
        return 10.0 * t_end
    i = idx[0]
    if i == 0:
        return 0.0
    f = (math.log(target) - math.log(rs[i - 1])) / (math.log(rs[i]) - math.log(rs[i - 1]))
    return ts[i - 1] + f * (ts[i] - ts[i - 1])


def turn_on(cell, led, p, amps):
    """Dark to 63% of the final conductance (seconds)."""
    g = generation(led, p, amps)
    final = cell.ohms(*cell.rest(g))
    return crossing(cell, cell.run(g, [0.0, 0.0], 0.05), final / 0.63, False, 0.05)


def turn_off(cell, led, p, amps, ohms=1e5):
    """From rest at `amps` to `ohms` after the LED goes dark (seconds)."""
    st = cell.rest(generation(led, p, amps))
    return crossing(cell, cell.run(0.0, st, 0.2), ohms, True, 0.2)


def fit_led(points):
    v = np.array([x for x, _ in points])
    i = np.array([y for _, y in points])
    keep = i > 1e-3

    def resid(p):
        led = Led(10**p[0], p[1], p[2])
        return np.array([led.volts(a) for a in i[keep]]) - v[keep]
    res = least_squares(resid, [-12.0, 2.0, 4.0], bounds=([-40, 1.0, 0.0], [-3, 10.0, 50.0]))
    return Led(10**res.x[0], res.x[1], res.x[2]), np.abs(res.fun).max()


def conductance(cell, q):
    return cell.hole_mobility * q[0] + cell.electron_mobility * q[1]


def history(cell, led, p, dark, light):
    """ln(RLH/RDH) = ln_ratio + slope ln(G / G_1mA), G the cell's
    photoconductance at rest (dark adapted): a function of the carriers,
    so the ratio follows the cell through a transient, never jumps."""
    g1 = conductance(cell, cell.rest(generation(led, p, 1e-3)))
    xs, ys = [], []
    for i, r in dark:
        for j, rl in light:
            if abs(math.log(j / i)) < 1e-3:
                xs.append(math.log(conductance(cell, cell.rest(generation(led, p, i))) / g1))
                ys.append(math.log(rl / r))
    a = np.polyfit(xs, ys, 1)
    return a[1], a[0], g1, np.abs(np.polyval(a, xs) - ys).max()


def rust_coupling(p):
    a1, s1, a2, s2, w = p
    return (f"Coupling {{ low: Line {{ ln_gain: {a1:.10g}, slope: {s1:.10g} }}, "
            f"high: Line {{ ln_gain: {a2:.10g}, slope: {s2:.10g} }}, softness: {w:.10g} }}")


def main(parts):
    if "vtl5c3" in parts:
        vtl5c3()
    if "nsl32" in parts:
        nsl32()


def vtl5c3():
    print("== VTL5C3 ==")
    led, err = fit_led(table("vtl5c3_led")["t=25"])
    print(f"LED: IS={led.is_:.10g} N={led.n:.10g} RS={led.rs:.10g}  (worst {err*1e3:.2f} mV)")
    rt = table("vtl5c3_resistance")
    targets = [("on", 0.04, 3.3e-3), ("on", 0.01, 5.8e-3), ("off", 0.04, 20.7e-3), ("off", 0.01, 16.8e-3)]

    def model(lg):
        cell = Cell(defects=10**lg[0], hole_rate=10**lg[1], electron_rate=10**lg[2])
        p, _ = fit_coupling(cell, led, rt["adapt=1"])
        return cell, p

    def resid(lg):
        cell, p = model(lg)
        out = []
        for kind, amps, t in targets:
            got = turn_on(cell, led, p, amps) if kind == "on" else turn_off(cell, led, p, amps)
            out.append(math.log(got / t))
        st = cell.rest(generation(led, p, 0.04))
        late = cell.ohms(*cell.run(0.0, st, 0.09).y[:, -1])
        out.append(math.log(late / 7.29e5))
        return out

    x0 = [math.log10(6.5e-7), math.log10(4.16e10), math.log10(3.63e8)]
    fit = least_squares(resid, x0, x_scale=0.3, diff_step=1e-3, bounds=([-9.0, 1.0, 6.0], [0.0, 11.0, 11.0]))
    cell, p = model(fit.x)
    print(f"cell: defects={cell.defects:.10g} hole_rate={cell.hole_rate:.10g} electron_rate={cell.electron_rate:.10g}"
          f"  (log-time residuals {np.round(fit.fun, 3)})")
    _, err = fit_coupling(cell, led, rt["adapt=1"])
    print(f"coupling: {rust_coupling(p)}  (worst {100*math.expm1(err):.2f}% in g)")
    h = history(cell, led, p, rt["adapt=1"], rt["adapt=2"])
    print(f"history: ln_ratio={h[0]:.10g} slope={h[1]:.10g} at G_1mA={h[2]:.10g} S (worst {100*math.expm1(h[3]):.2f}%)")
    print(f"  generation at 1 mA {generation(led, p, 1e-3):.10g} A")
    for amps in (0.04, 0.01):
        print(f"  turn-on {amps*1e3:.0f} mA {1e3*turn_on(cell, led, p, amps):.3f} ms;"
              f" turn-off to 100k {1e3*turn_off(cell, led, p, amps):.3f} ms")


def nsl32():
    print("== NSL-32 ==")
    red = Led(93.2e-12, 3.73, 42e-3)  # Shockley::LED_RED
    nsl = table("nsl32_resistance")["t=23"]

    def nsl_model(lg):
        c = Cell(dark=5e6, defects=10**lg[2], hole_rate=10**lg[0], electron_rate=10**lg[1])
        pp, _ = fit_coupling(c, red, nsl)
        return c, pp

    def nsl_resid(lg):
        c, pp = nsl_model(lg)
        # The model cannot turn off as fast as it turns on (no CdS cell
        # does): the release, what a compressor or tremolo is heard by,
        # is held three times as hard as the attack.
        return [0.33 * math.log(turn_on(c, red, pp, 0.02) / 5e-3), math.log(turn_off(c, red, pp, 0.016) / 5e-3)]
    fit = least_squares(nsl_resid, [10.0, math.log10(8.9e8), math.log10(6.5e-7)],
                        x_scale=0.3, diff_step=1e-3, bounds=([1.0, 6.0, -9.0], [11.0, 11.0, 0.0]))
    c, pp = nsl_model(fit.x)
    print(f"cell: defects={c.defects:.10g} hole_rate={c.hole_rate:.10g} electron_rate={c.electron_rate:.10g}"
          f"  (weighted log-time residuals {np.round(fit.fun, 4)})")
    print(f"  turn-on 20 mA {1e3*turn_on(c, red, pp, 0.02):.3f} ms; turn-off from 16 mA to 100k {1e3*turn_off(c, red, pp, 0.016):.3f} ms")
    _, err = fit_coupling(c, red, nsl)
    print(f"coupling: {rust_coupling(pp)}  (worst {100*math.expm1(err):.2f}% in g)")


if __name__ == "__main__":
    import sys
    main(sys.argv[1:] or ["vtl5c3", "nsl32"])

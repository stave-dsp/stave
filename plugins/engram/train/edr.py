#!/usr/bin/env python3
"""Engram — the reverb's TIME–FREQUENCY ANATOMY (energy decay relief):
how much energy each octave band holds in each moment, and what real
rooms do with it that a plain algorithmic reverb doesn't:

* BLOOM — a band's energy density peaking AFTER the onset (rising first);
* TWO SLOPES — a band's early decay rate ≠ its late decay rate;
* EARLY vs LATE colour — the spectrum of 0–50 ms vs 200 ms on;
* RESONANCE — narrow modes in the late spectrum (peaks over the smooth
  spectrum, and how long they ring vs their neighbours).

    edr.py --compare DIR   real IRs vs the plugin's renders in DIR
                           (space_check.py's renders, named s<i>.wav)

Bounded: ≤ 12 s per file; run under train/guard.py.
"""

import argparse
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, resample_poly, sosfilt

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ir_measure as irm  # noqa: E402

SR = 48_000
OCT = [63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
# Time bins (ms after the direct sound): fine early, log late.
EDGES_MS = [2, 5, 10, 20, 40, 80, 160, 320, 640, 1280, 2560, 5120, 10240]
FLOOR = 1e-14


def load_mid(path):
    x, sr = sf.read(str(path), frames=int(12 * sf.info(str(path)).samplerate), always_2d=True, dtype="float64")
    if sr != SR:
        g = np.gcd(sr, SR)
        x = resample_poly(x, SR // g, sr // g, axis=0)
    m = x.mean(1)
    a = np.abs(m)
    onset = int(np.argmax(a >= 0.1 * a.max()))
    return m[onset:] / max(a.max(), FLOOR)


def octave(m, c):
    lo, hi = c / 2 ** 0.5, min(c * 2 ** 0.5, 0.45 * SR)
    sos = butter(3, [lo, hi], "bandpass", fs=SR, output="sos")
    return sosfilt(sos, m)


def anatomy(m):
    """EDR (bands × time bins, dB re the band's total), and the summary
    stats: bloom time, early/late slope, early/late colour, resonance.
    `m` starts at the direct sound, which is removed (2 ms): the ROOM
    only — the direct's level in a render is arbitrary, and band filters
    smear it into the early bins."""
    m = m.copy()
    m[: int(0.002 * SR)] = 0.0
    e_bins = np.zeros((len(OCT), len(EDGES_MS) - 1))
    slopes = np.zeros((len(OCT), 2))
    bloom = np.zeros(len(OCT))
    for k, c in enumerate(OCT):
        b = octave(m, c) ** 2
        tot = b.sum() + FLOOR
        for j in range(len(EDGES_MS) - 1):
            i0, i1 = int(EDGES_MS[j] * SR / 1000), int(EDGES_MS[j + 1] * SR / 1000)
            seg = b[i0:i1]
            # Energy DENSITY (per ms), so bins of different widths compare.
            e_bins[k, j] = 10 * np.log10(seg.sum() / tot / (EDGES_MS[j + 1] - EDGES_MS[j]) + FLOOR) if len(seg) else -150
        # Bloom: when the smoothed density peaks (10 ms window), ms.
        w = int(0.01 * SR)
        sm = np.convolve(b[: int(0.5 * SR)], np.ones(w) / w, mode="same")
        bloom[k] = 1000 * np.argmax(sm) / SR
        # Two slopes from the Schroeder curve: −0…−10 dB vs −15…−35 dB.
        edc = np.cumsum(b[::-1])[::-1]
        db = 10 * np.log10(edc / edc[0] + FLOOR)
        for s, (lo, hi) in enumerate(((0, -10), (-15, -35))):
            i0, i1 = np.argmax(db <= lo), np.argmax(db <= hi)
            if db[i1] <= hi and i1 > i0 + 16:
                t = np.arange(i0, i1) / SR
                slopes[k, s] = -60 / np.polyfit(t, db[i0:i1], 1)[0]  # as a T60, s
            else:
                slopes[k, s] = np.nan
    # Colour: band levels in 0–50 ms vs 200 ms–2 s (dB, mean removed).
    def colour(a, b):
        lv = np.array([10 * np.log10((octave(m[int(a * SR):int(b * SR)], c) ** 2).sum() + FLOOR) for c in OCT])
        return lv - lv.mean()
    early, late = colour(0.0015, 0.05), colour(0.2, 2.0)
    # Resonance: late spectrum (200 ms – 1.2 s) peaks above a 1/3-oct
    # smoothed version, 40 Hz – 2 kHz.
    seg = m[int(0.2 * SR):int(1.2 * SR)]
    res = 0.0
    if len(seg) > SR // 4:
        S = 20 * np.log10(np.abs(np.fft.rfft(seg * np.hanning(len(seg)))) + FLOOR)
        f = np.fft.rfftfreq(len(seg), 1 / SR)
        band = (f > 40) & (f < 2000)
        smooth = np.array([S[(f > fi / 2 ** (1 / 6)) & (f < fi * 2 ** (1 / 6))].mean() for fi in f[band][::8]])
        peaks = S[band][::8] - smooth
        res = np.percentile(peaks, 99)  # how far the strongest modes stand out, dB
    return e_bins, slopes, bloom, early, late, res


def acoustics(x):
    """Standard room-acoustics parameters (ISO 3382) of a stereo IR whose
    sample 0 is the direct sound: C50, C80 (clarity, dB), EDT (s, from the
    0…−10 dB decay), Ts (centre time, ms), IACC early (0–80 ms) and late
    (80 ms on) — all on the 500 Hz–2 kHz mid band, direct included as in
    the standard."""
    x = x.copy()
    x[: int(0.002 * SR)] = 0.0  # the ROOM only (direct cut, as everywhere)
    m = x.mean(1)
    def mid(v):
        return octave(v, 500) + octave(v, 1000) + octave(v, 2000)
    e = mid(m) ** 2
    t = np.arange(len(e)) / SR
    c = lambda ms: 10 * np.log10(e[: int(ms * SR / 1000)].sum() / max(e[int(ms * SR / 1000):].sum(), FLOOR))
    edc = np.cumsum(e[::-1])[::-1]
    db = 10 * np.log10(edc / edc[0] + FLOOR)
    i10 = np.argmax(db <= -10)
    edt = 6 * i10 / SR if i10 > 0 else np.nan
    ts = 1000 * (t * e).sum() / e.sum()
    l, r = mid(x[:, 0]), mid(x[:, 1])
    def iacc(a, b):
        n = len(a)
        best = 0.0
        for lag in range(-48, 49):
            u, v = (a[max(lag, 0):n + min(lag, 0)], b[max(-lag, 0):n - max(lag, 0)])
            best = max(best, abs((u * v).sum()) / np.sqrt((u * u).sum() * (v * v).sum() + FLOOR))
        return best
    e80 = int(0.08 * SR)
    return np.array([c(50), c(80), edt, ts, iacc(l[:e80], r[:e80]), iacc(l[e80:e80 + SR], r[e80:e80 + SR])])


ACOUSTICS = ["C50 dB", "C80 dB", "EDT s", "Ts ms", "IACC early", "IACC late"]


def load_stereo(path, real):
    """Stereo, starting at the direct sound (real IRs: their onset)."""
    if real:
        x = irm.load(real)
    else:
        x, sr = sf.read(str(path), always_2d=True, dtype="float64")
        if x.shape[1] == 1:
            x = np.repeat(x, 2, 1)
    a = np.abs(x).max(1)
    on = int(np.argmax(a >= 0.1 * a.max()))
    return x[on:on + int(4 * SR)] / a.max()


def summarise(rows, label):
    E = np.array([r[0] for r in rows])
    S = np.array([r[1] for r in rows])
    B = np.array([r[2] for r in rows])
    EL = np.array([r[4] - r[3] for r in rows])
    R = np.array([r[5] for r in rows])
    print(f"\n== {label} ({len(rows)})")
    print("  bloom (ms to peak density) by octave, median:   " + " ".join(f"{c:>5}" for c in OCT))
    print("                                                  " + " ".join(f"{v:5.0f}" for v in np.median(B, 0)))
    print("  share blooming > 15 ms:                         " + " ".join(f"{v:5.2f}" for v in (B > 15).mean(0)))
    ratio = S[:, :, 1] / S[:, :, 0]
    print("  late/early decay ratio (1 = one slope), median: " + " ".join(f"{v:5.2f}" for v in np.nanmedian(ratio, 0)))
    print("  share two-slope (ratio > 1.5 or < 0.67):        " + " ".join(f"{v:5.2f}" for v in np.nanmean((ratio > 1.5) | (ratio < 0.67), 0)))
    print("  late − early colour (dB), median:               " + " ".join(f"{v:5.1f}" for v in np.median(EL, 0)))
    print("  |late − early colour| (dB), median:             " + " ".join(f"{v:5.1f}" for v in np.median(np.abs(EL), 0)))
    print(f"  resonance (strongest modes over smooth, dB): median {np.median(R):.1f}, p90 {np.percentile(R, 90):.1f}")
    return E, S, B, EL, R


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--compare", default=str(Path.home() / ".cache/engram-train/irs/check/generated"))
    ap.add_argument("--every", type=int, default=12)
    a = ap.parse_args()
    groups = list(irm.spaces().items())
    d = np.load(Path.home() / ".cache/engram-train/irs/features.npz", allow_pickle=True)
    names = list(d["names"])
    # The same spaces space_check rendered (features order = names order).
    pick = list(range(0, len(names), a.every))
    by_key = dict(groups)
    real, plug = [], []
    for i in pick:
        paths = by_key[names[i]]
        mid = irm.load(paths).mean(1)
        on = int(np.argmax(np.abs(mid) >= 0.1 * np.abs(mid).max()))
        real.append(anatomy(mid[on:] / np.abs(mid).max()))
        r = Path(a.compare) / f"s{i}.wav"
        if r.exists():
            plug.append(anatomy(load_mid(r)))
    Er, Sr, Br, ELr, Rr = summarise(real, "REAL IRs")
    if plug:
        Ep, Sp, Bp, ELp, Rp = summarise(plug, "PLUGIN (generated room at each dot)")
        err = np.abs(Ep - Er)
        print("\n  EDR error |plugin − real| dB, median by octave:  " + " ".join(f"{v:5.1f}" for v in np.median(err, (0, 2))))
        print("  EDR error by time bin (ms → ):                  " + " ".join(f"{e}:{v:.1f}" for e, v in zip(EDGES_MS[1:], np.median(err, (0, 1)))))
        np.savez(Path.home() / ".cache/engram-train/irs/edr_compare.npz", Er=Er, Ep=Ep, Br=Br, Bp=Bp)
    # ISO 3382 parameters on the ROOM (direct cut — its level in a render
    # is arbitrary), real vs plugin.
    A_r, A_p = [], []
    for i in pick:
        r = Path(a.compare) / f"s{i}.wav"
        if not r.exists():
            continue
        A_r.append(acoustics(load_stereo(None, by_key[names[i]])))
        A_p.append(acoustics(load_stereo(r, None)))
    A_r, A_p = np.array(A_r), np.array(A_p)
    print("\n  ISO 3382, room only (mid band)  real median   plugin median   corr")
    for k, name in enumerate(ACOUSTICS):
        ok = np.isfinite(A_r[:, k]) & np.isfinite(A_p[:, k])
        print(f"  {name:<12} {np.median(A_r[ok, k]):14.2f} {np.median(A_p[ok, k]):15.2f} {np.corrcoef(A_r[ok, k], A_p[ok, k])[0, 1]:7.2f}")


if __name__ == "__main__":
    main()

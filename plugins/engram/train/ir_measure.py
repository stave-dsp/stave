#!/usr/bin/env python3
"""Engram generative room — measure every reverb IR in the library.

Each SPACE (an IR file, or an L/R mono pair, de-duplicated across the
library's .aif/.wav copies) becomes one row of numbers the engine can
RESYNTHESIZE — not a spectrogram, but the reverb's anatomy:

* EARLY  — per low/mid/high band (the direct sound cut): slot 0 = the
           reflections' share of the band's reverb energy (0–115 ms, as a
           logit); slots 1–23 = their PATTERN, 5 ms bins 0–115 ms after
           the direct (log10 of each bin's share of the reflections). The total is
           explicit so a decoder can't lose it averaging logs;
* DECAY  — T60 per octave band, 125 Hz … 8 kHz (log seconds);
* TONE   — the IR's energy per octave band (log, mean removed);
* DENSITY— mixing time (normalised echo density reaching ~Gaussian) and
           how dense the first 50 ms are;
* SHAPE  — the broadband envelope in log-spaced bins to 12 s, relative to
           an exponential fit: gated, reversed, swelling "strange places"
           that a T60 alone can't say;
* STEREO — early and late L/R correlation, side/mid energy;
* PREDELAY.

Writes `~/.cache/engram-train/irs/features.npz` (X, names, categories,
layout). Reads at most 12 s of each file (bounded memory); run it under
train/guard.py.
"""

import os
import re
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, resample_poly, sosfilt

# The IR library (third-party, never committed): $ENGRAM_IRS or ../refs/irs.
LIB = Path(os.environ.get("ENGRAM_IRS", Path(__file__).resolve().parent.parent / "refs/irs"))
OUT = Path.home() / ".cache/engram-train/irs"
SR = 48_000
MAX_S = 12.0
CENTRES = [125, 250, 500, 1000, 2000, 4000, 8000]
ER_BANDS = [(20, 500), (500, 4000), (4000, 20000)]
ER_BIN_MS, ER_BINS = 5, 24
SHAPE_EDGES = np.geomspace(0.05, MAX_S, 13)  # 12 bins, 50 ms … 12 s
FLOOR = 1e-12


def layout():
    parts = [("early", len(ER_BANDS) * ER_BINS), ("decay", len(CENTRES)), ("tone", len(CENTRES)),
             ("density", 2), ("shape", len(SHAPE_EDGES) - 1), ("stereo", 3), ("predelay", 1)]
    out, at = {}, 0
    for name, n in parts:
        out[name] = (at, at + n)
        at += n
    return out, at


def spaces():
    """{key: [paths]} — one key per space; L/R mono pairs share a key."""
    groups = {}
    for p in sorted(LIB.rglob("*")):
        if not p.is_file() or p.name.startswith("._") or p.suffix.lower() not in (".wav", ".aif", ".aiff"):
            continue
        stem = p.stem
        side = None
        m = re.match(r"^(.*)\s([LR])$", stem)
        if m:
            stem, side = m.group(1), m.group(2)
        key = (p.parent.relative_to(LIB).as_posix() + "/" + stem).lower()
        g = groups.setdefault(key, {})
        # .aif and .wav copies of one IR: keep one per side.
        g.setdefault(side, p)
    return groups


def load(paths):
    """Stereo float64 at SR, at most MAX_S, from one file or an L/R pair."""
    chans = []
    for side in ("L", "R") if "L" in paths and "R" in paths else (None,):
        p = paths[side] if side else next(iter(paths.values()))
        info = sf.info(str(p))
        x, sr = sf.read(str(p), frames=int(MAX_S * info.samplerate), always_2d=True, dtype="float64")
        if sr != SR:
            g = np.gcd(sr, SR)
            x = resample_poly(x, SR // g, sr // g, axis=0)
        chans.append(x)
    if len(chans) == 2:
        n = min(len(chans[0]), len(chans[1]))
        x = np.stack([chans[0][:n, 0], chans[1][:n, 0]], axis=1)
    else:
        x = chans[0]
    if x.shape[1] == 1:
        x = np.repeat(x, 2, axis=1)
    return x[:, :2]


def band(x, lo, hi):
    if lo <= 20:
        sos = butter(4, hi, "lowpass", fs=SR, output="sos")
    elif hi >= 20000:
        sos = butter(4, lo, "highpass", fs=SR, output="sos")
    else:
        sos = butter(4, [lo, hi], "bandpass", fs=SR, output="sos")
    return sosfilt(sos, x)


def t60(e):
    """T60 from a band's energy, by Schroeder backward integration:
    the −5…−25 dB slope (else −5…−15), ×60/dB-span. Seconds."""
    edc = np.cumsum(e[::-1])[::-1]
    edc_db = 10 * np.log10(edc / max(edc[0], FLOOR) + FLOOR)
    for lo, hi in ((-5, -25), (-5, -15)):
        i0 = np.argmax(edc_db <= lo)
        i1 = np.argmax(edc_db <= hi)
        if edc_db[i0] <= lo and edc_db[i1] <= hi and i1 > i0 + 8:
            t = np.arange(i0, i1) / SR
            slope = np.polyfit(t, edc_db[i0:i1], 1)[0]
            if slope < 0:
                return float(np.clip(-60 / slope, 0.03, 60))
    return 60.0  # never decays within the window (drones, loops)


def echo_density(m, win_ms=20):
    """Abel–Huang normalised echo density: the share of samples beyond one
    std in a sliding window, over the Gaussian expectation (0.3173)."""
    w = int(win_ms * SR / 1000)
    hop = w // 4
    out = []
    for at in range(0, max(len(m) - w, 1), hop):
        s = m[at:at + w]
        sd = s.std()
        out.append(np.mean(np.abs(s) > sd) / 0.3173 if sd > 0 else 0.0)
    return np.array(out), hop / SR


def measure(item):
    key, paths = item
    try:
        x = load(paths)
    except Exception as e:  # unreadable / protected
        return key, None, str(e)
    peak = np.abs(x).max()
    if peak <= 0:
        return key, None, "silent"
    x = x / peak
    a = np.abs(x).max(axis=1)
    onset = int(np.argmax(a >= 0.1))
    pre = onset / SR
    x = x[max(onset - SR // 1000, 0):].copy()
    # The DIRECT sound out (1 ms before the onset … 2 ms after): it isn't
    # the room, and band filters smear it into the first reflections'
    # bins (a fake 5–10 ms reflection the plugin then faithfully made).
    x[: int(0.003 * SR)] = 0.0
    mid = x.mean(axis=1)
    side = (x[:, 0] - x[:, 1]) / 2
    lay, n = layout()
    f = np.zeros(n)

    # EARLY: energy per 5 ms bin per band, relative to the band's total.
    b = int(ER_BIN_MS * SR / 1000)
    for k, (lo, hi) in enumerate(ER_BANDS):
        e = band(mid, lo, hi) ** 2
        tot = e.sum() + FLOOR
        bins = np.array([e[i * b:(i + 1) * b].sum() for i in range(ER_BINS)])
        # (The direct is cut, so bin 0 — 2–5 ms — is reflections too.)
        refl = bins[:-1].sum()
        frac = np.clip(refl / max(tot, FLOOR), 1e-4, 1 - 1e-4)
        o = lay["early"][0] + k * ER_BINS
        f[o] = np.log(frac / (1 - frac))
        f[o + 1:o + ER_BINS] = np.log10(bins[:-1] / max(refl, FLOOR) + 1e-4)

    # DECAY and TONE per octave band.
    tone = []
    for k, c in enumerate(CENTRES):
        lo, hi = (20, c * 2 ** 0.5) if k == 0 else ((c / 2 ** 0.5, 20000) if k == len(CENTRES) - 1 else (c / 2 ** 0.5, c * 2 ** 0.5))
        e = band(mid, lo, hi) ** 2
        f[lay["decay"][0] + k] = np.log10(t60(e))
        tone.append(np.log10(e.sum() + FLOOR))
    tone = np.array(tone)
    f[lay["tone"][0]:lay["tone"][1]] = tone - tone.mean()

    # DENSITY: mixing time (NED first ≥ 0.9) and early density.
    ned, dt = echo_density(mid)
    hit = np.argmax(ned >= 0.9) if (ned >= 0.9).any() else len(ned)
    f[lay["density"][0]] = np.log10(max(hit * dt, 0.001))
    f[lay["density"][0] + 1] = ned[: max(int(0.05 / dt), 1)].mean() if len(ned) else 0.0

    # SHAPE: log energy per log-spaced bin, minus the exponential implied
    # by the 1 kHz T60 — what's left is non-exponential character.
    e = mid ** 2
    tot = e.sum() + FLOOR
    t_mid = 10 ** f[lay["decay"][0] + 3]
    for k in range(len(SHAPE_EDGES) - 1):
        i0, i1 = int(SHAPE_EDGES[k] * SR), int(SHAPE_EDGES[k + 1] * SR)
        seg = e[i0:i1]
        level = 10 * np.log10(seg.mean() / tot + FLOOR) if len(seg) else -120.0
        t = np.sqrt(SHAPE_EDGES[k] * SHAPE_EDGES[k + 1])
        expect = -60 * t / t_mid
        f[lay["shape"][0] + k] = np.clip(level - expect - 10 * np.log10(1 / (SR * 0.1) + FLOOR), -60, 60) / 20

    # STEREO.
    def corr(u, v):
        d = np.sqrt((u ** 2).sum() * (v ** 2).sum())
        return float((u * v).sum() / d) if d > 0 else 1.0
    e80 = int(0.08 * SR)
    f[lay["stereo"][0]] = corr(x[:e80, 0], x[:e80, 1])
    f[lay["stereo"][0] + 1] = corr(x[e80:, 0], x[e80:, 1]) if len(x) > e80 + 64 else 1.0
    f[lay["stereo"][0] + 2] = np.log10(((side ** 2).sum() + FLOOR) / ((mid ** 2).sum() + FLOOR)) / 3

    f[lay["predelay"][0]] = np.log10(pre + 0.001)
    return key, f, None


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    groups = spaces()
    items = list(groups.items())
    print(f"{len(items)} spaces from {sum(len(g) for g in groups.values())} files", flush=True)
    rows, names, cats, failed = [], [], [], []
    workers = int(os.environ.get("IR_WORKERS", "4"))
    with ProcessPoolExecutor(workers) as ex:
        for key, f, err in ex.map(measure, items, chunksize=4):
            if f is None:
                failed.append((key, err))
                continue
            rows.append(f)
            names.append(key)
            parts = key.split("/")
            cats.append("/".join(parts[:2]))
    X = np.array(rows, dtype=np.float32)
    lay, n = layout()
    np.savez(OUT / "features.npz", X=X, names=np.array(names), categories=np.array(cats),
             layout=np.array([(k, a, b) for k, (a, b) in lay.items()], dtype=object))
    print(f"measured {len(rows)} spaces x {n} numbers -> {OUT / 'features.npz'}; {len(failed)} failed")
    for k, e in failed[:10]:
        print("  failed:", k, e[:100])
    # Sanity: per category, median T60 @1 kHz and mixing time.
    d0, m0 = lay["decay"][0] + 3, lay["density"][0]
    by = {}
    for c, row in zip(cats, X):
        by.setdefault(c, []).append(row)
    for c in sorted(by):
        r = np.array(by[c])
        print(f"  {c:<45} n {len(r):>3}  T60@1k {np.median(10 ** r[:, d0]):6.2f} s  mix {1000 * np.median(10 ** r[:, m0]):6.0f} ms")


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Deep corpus analysis of the York reference IRs — the measurement
behind tolex's M2-M4 modeling laws (docs/tolex.md).

Dimensions, per IR:
  - modal structure: peak freqs / prominences / Q in 500-8000 Hz
  - per-1/3-octave decay times (Schroeder T20) -> the ring law
  - cepstral reflections 0.3-6 ms -> box geometry
  - excess (non-minimum-phase) group delay per band
  - energy timeline: t10/t50/t90, direct/early/late split
  - deep notches below trend
  - macro: tilt, HF corner, per position class

Group questions:
  - do mode FREQUENCIES hold across mic positions on one cab while
    amplitudes move? (peak-match rate same-cab vs cross-cab) -> if
    yes, M2 = fixed modal set + position-dependent weights
  - fine-structure correlation same-cab vs cross-cab
  - decay-time law T(f) population curve
  - mode density and Q distributions per octave
  - reflection-delay histogram

Outputs: out/corpus.json + out/*.png + printed summary.
"""

import json
import os
import re
import sys
import wave

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import naming
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy import signal

ROOT = Path(__file__).resolve().parent.parent / "refs/york"
OUT = Path(__file__).resolve().parent.parent / "refs/analysis/out"
OUT.mkdir(exist_ok=True)

FS = 48000
NFFT = 32768
GRID_N = 1600
GRID = 40.0 * (20000.0 / 40.0) ** (np.arange(GRID_N) / (GRID_N - 1))
PTS_PER_OCT = GRID_N / np.log2(20000.0 / 40.0)

def read_wav_mono(path):
    with wave.open(str(path), "rb") as w:
        n, ch, sw, sr = w.getnframes(), w.getnchannels(), w.getsampwidth(), w.getframerate()
        raw = w.readframes(n)
    if sr != FS:
        return None
    if sw == 3:
        a = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
        x = (
            a[:, 0].astype(np.int32)
            | (a[:, 1].astype(np.int32) << 8)
            | (a[:, 2].astype(np.int32) << 16)
        )
        x = (x ^ 0x800000) - 0x800000
        x = x.astype(np.float64) / 2**23
    elif sw == 2:
        x = np.frombuffer(raw, dtype="<i2").astype(np.float64) / 2**15
    elif sw == 4:
        x = np.frombuffer(raw, dtype="<i4").astype(np.float64) / 2**31
    else:
        return None
    if ch > 1:
        x = x.reshape(-1, ch)[:, 0]
    return x


def smoothed_db(power, frac_oct):
    half = 2.0 ** (1.0 / (2.0 * frac_oct))
    bins = np.fft.rfftfreq(NFFT, 1.0 / FS)
    lo = np.searchsorted(bins, GRID / half)
    hi = np.searchsorted(bins, GRID * half)
    hi = np.maximum(hi, lo + 2)
    csum = np.concatenate([[0.0], np.cumsum(power)])
    mean = (csum[hi] - csum[lo]) / (hi - lo)
    return 10.0 * np.log10(np.maximum(mean, 1e-30))


def octave_of(w_pts):
    return w_pts / PTS_PER_OCT


THIRD_OCT = 1000.0 * 2.0 ** (np.arange(-10, 11) / 3.0)  # 99 Hz .. 10.08 kHz


def band_t20(ir, fc):
    lo, hi = fc / 2 ** (1 / 6), fc * 2 ** (1 / 6)
    if hi > FS / 2 * 0.95:
        return None
    sos = signal.butter(4, [lo, hi], btype="band", fs=FS, output="sos")
    y = signal.sosfiltfilt(sos, ir)
    e = y[::-1] ** 2
    edc = np.cumsum(e)[::-1]
    edc = 10.0 * np.log10(np.maximum(edc / edc.max(), 1e-12))
    try:
        i5 = np.flatnonzero(edc <= -5.0)[0]
        i25 = np.flatnonzero(edc <= -25.0)[0]
    except IndexError:
        return None
    if i25 <= i5:
        return None
    # T20 -> extrapolated T60-style: time for 20 dB * 3.
    return (i25 - i5) / FS * 3.0


def analyze(ir):
    ir = ir[:24000]
    pk = np.argmax(np.abs(ir))
    start = np.flatnonzero(np.abs(ir) > 0.1 * np.abs(ir[pk]))[0]

    h = np.fft.rfft(ir, NFFT)
    power = np.abs(h[: NFFT // 2]) ** 2
    fine = smoothed_db(power[: NFFT // 2], 48.0)
    trend = smoothed_db(power[: NFFT // 2], 3.0)
    resid = fine - trend

    band = (GRID >= 500.0) & (GRID <= 8000.0)
    bi = np.flatnonzero(band)
    peaks, props = signal.find_peaks(
        resid[bi], prominence=1.5, width=1
    )
    pfreqs = GRID[bi][peaks]
    proms = props["prominences"]
    widths_oct = octave_of(props["widths"])
    qs = 1.0 / np.maximum(2.0 ** (widths_oct / 2) - 2.0 ** (-widths_oct / 2), 1e-6)

    # Deep notches below trend.
    notch_band = (GRID >= 200.0) & (GRID <= 8000.0)
    ni = np.flatnonzero(notch_band)
    notches, nprops = signal.find_peaks(-resid[ni], prominence=3.0)
    notch_depths = resid[ni][notches]

    # Per-band decay.
    t20 = {}
    for fc in THIRD_OCT:
        t = band_t20(ir, fc)
        if t is not None:
            t20[round(fc, 1)] = t

    # Cepstral reflections (log-magnitude cepstrum).
    logmag = np.log(np.maximum(np.abs(h), 1e-12))
    cep = np.fft.irfft(logmag)
    q0, q1 = int(0.00005 * FS), int(0.006 * FS)
    cpk, cprops = signal.find_peaks(cep[q0:q1], prominence=0.02)
    refl = [((q0 + i) / FS * 1000.0, float(cep[q0 + i])) for i in cpk[np.argsort(cprops["prominences"])[::-1][:5]]]

    # Excess group delay vs minimum phase.
    c = np.fft.irfft(logmag)
    fold = np.zeros_like(c)
    fold[0] = c[0]
    fold[1 : len(c) // 2] = 2.0 * c[1 : len(c) // 2]
    fold[len(c) // 2] = c[len(c) // 2]
    hmin = np.exp(np.fft.rfft(fold))
    excess = np.unwrap(np.angle(h / np.maximum(np.abs(hmin), 1e-12) * np.exp(-1j * np.angle(hmin))))
    freqs = np.fft.rfftfreq(NFFT, 1.0 / FS)
    gd = -np.gradient(excess, 2.0 * np.pi * freqs + 1e-9) * 1000.0  # ms
    sel = (freqs >= 200.0) & (freqs <= 5000.0)
    gd_dev = float(np.sqrt(np.mean((gd[sel] - np.median(gd[sel])) ** 2)))

    # Energy timeline relative to arrival.
    e = ir[start:] ** 2
    ce = np.cumsum(e) / max(np.sum(e), 1e-30)
    t_at = lambda p: float(np.flatnonzero(ce >= p)[0]) / FS * 1000.0
    ms = lambda t: int(t * FS / 1000.0)
    direct = float(np.sum(e[: ms(0.5)]))
    early = float(np.sum(e[ms(0.5) : ms(5.0)]))
    late = float(np.sum(e[ms(5.0) :]))
    tot = direct + early + late

    tilt = float(
        np.mean(trend[(GRID >= 2500) & (GRID <= 4000)])
        - np.mean(trend[(GRID >= 300) & (GRID <= 600)])
    )
    ref_db = np.mean(trend[(GRID >= 800) & (GRID <= 1200)])
    above = np.flatnonzero((GRID > 1500) & (trend < ref_db - 12.0))
    hf_corner = float(GRID[above[0]]) if len(above) else 20000.0

    return {
        "peak_freqs": pfreqs.tolist(),
        "peak_proms": proms.tolist(),
        "peak_qs": qs.tolist(),
        "notch_depths": notch_depths.tolist(),
        "t20": t20,
        "reflections_ms": refl,
        "gd_dev_ms": gd_dev,
        "t10_ms": t_at(0.10),
        "t50_ms": t_at(0.50),
        "t90_ms": t_at(0.90),
        "direct_frac": direct / tot,
        "early_frac": early / tot,
        "late_frac": late / tot,
        "tilt_db": tilt,
        "hf_corner": hf_corner,
        "resid_sig": resid[(GRID >= 500) & (GRID <= 8000)].tolist(),
    }


def main():
    files = sorted(ROOT.rglob("*.wav"))
    files = [f for f in files if "48k" in str(f) and "Natural Phase Singles" in str(f)]
    print(f"{len(files)} reference IRs")

    results = []
    for i, f in enumerate(files):
        x = read_wav_mono(f)
        if x is None or len(x) < 4800:
            continue
        cab = f.parts[len(ROOT.parts)]  # pack dir name
        r = analyze(x)
        r.update({"file": f.name, "cab": cab})
        r.update(naming.parse(f.name))
        results.append(r)
        if (i + 1) % 200 == 0:
            print(f"  {i+1}/{len(files)}")

    with open(OUT / "corpus.json", "w") as fh:
        json.dump(results, fh)
    print(f"wrote {OUT/'corpus.json'} ({len(results)} IRs)")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Engram — MORPH quality: does the room change cleanly while it moves?

Reads tests/space_renders.rs `render_morphs` output (static, sweep, drift
renders of steady pink-ish noise) and reports, against the static render:

* CLICKS — one-sample spikes in the second difference (> 8× the local
  robust spread): a discontinuity in the signal or its slope;
* PUMPING — short-term level (50 ms) wobble, dB rms of its deviation from
  a 1 s moving average (a room changing slowly may drift; it shouldn't
  wobble);
* ZIPPER — spectral flux (frame-to-frame, 2.5 ms) above the static
  render's own flux: block-rate stepping shows as extra flux.
"""

import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt

SR = 48_000


def analyse(path):
    x, sr = sf.read(str(path))
    m = x.mean(1)[SR:]  # skip the settling second
    # CLICKS: the second difference turns a discontinuity (a step in the
    # signal or its slope) into a one-sample spike; count spikes above 8×
    # the local robust spread (median absolute value over 10 ms).
    d2 = np.abs(m[2:] - 2 * m[1:-1] + m[:-2])
    w10 = int(0.01 * SR)
    spread = np.array([np.median(d2[i:i + w10]) for i in range(0, len(d2), w10)])
    local = np.repeat(spread, w10)[: len(d2)] / 0.6745 + 1e-12
    clicks = int(np.sum(d2 > 8 * local))
    w = int(0.05 * SR)
    lv = np.array([10 * np.log10(np.mean(m[i:i + w] ** 2) + 1e-15) for i in range(0, len(m) - w, w // 2)])
    k = 40
    avg = np.convolve(lv, np.ones(k) / k, mode="same")
    pump = float(np.sqrt(np.mean((lv[k:-k] - avg[k:-k]) ** 2))) if len(lv) > 2 * k + 4 else float("nan")
    g = int(0.0025 * SR)
    frames = [np.abs(np.fft.rfft(m[i:i + 2 * g] * np.hanning(2 * g))) for i in range(0, len(m) - 2 * g, g)]
    S = np.log(np.array(frames) + 1e-9)
    flux = float(np.median(np.sqrt(np.mean(np.diff(S, axis=0) ** 2, axis=1))))
    return clicks, pump, flux


def main():
    d = Path(sys.argv[1] if len(sys.argv) > 1 else Path.home() / ".cache/engram-train/irs/morph")
    base = None
    print(f"{'':14}{'clicks':>8}{'pumping dB':>12}{'flux':>8}")
    for name in ("morph-static", "morph-sweep", "morph-drift"):
        c, p, fl = analyse(d / f"{name}.wav")
        base = base or fl
        print(f"{name:<14}{c:>8}{p:>12.2f}{fl:>8.3f}  (flux ×{fl / base:.2f} of static)")


if __name__ == "__main__":
    main()

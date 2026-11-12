#!/usr/bin/env python3
"""Render the silk behaviour atlas from <dir>/*.csv → *.png.

<dir> is the first argument, else $SILK_ATLAS_DIR, else <temp dir>/silk_atlas
(where crates/stave-test/tests/silk_atlas.rs writes by default).

Run after:
  cargo test -p stave-test --release --test silk_atlas -- --ignored --nocapture
"""
import csv
import os
import sys
import tempfile

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

D = (sys.argv[1] if len(sys.argv) > 1 else None) or os.environ.get(
    "SILK_ATLAS_DIR", os.path.join(tempfile.gettempdir(), "silk_atlas")
)
FS = 48_000


def col(path, name=None, idx=0):
    with open(os.path.join(D, path)) as f:
        r = csv.reader(f)
        header = next(r)
        if name is not None:
            idx = header.index(name)
        return np.array([float(row[idx]) for row in r])


def load(path):
    with open(os.path.join(D, path)) as f:
        r = csv.reader(f)
        header = next(r)
        data = np.array([[float(v) for v in row] for row in r])
    return header, data


plt.rcParams.update({
    "figure.facecolor": "#101318",
    "axes.facecolor": "#151a21",
    "axes.edgecolor": "#3a4150",
    "axes.labelcolor": "#c7cdd8",
    "text.color": "#c7cdd8",
    "xtick.color": "#9aa2b1",
    "ytick.color": "#9aa2b1",
    "font.size": 9,
})

# ---------------------------------------------------------------- Fig 1
# Spectrograms 20–800 Hz: dry vs silk 0.1 / 0.5 / 0.9 on the kick pattern.
files = [("kick_dry.csv", "DRY"), ("kick_s010.csv", "SILK 0.1"),
         ("kick_s050.csv", "SILK 0.5"), ("kick_s090.csv", "SILK 0.9")]
fig, axes = plt.subplots(1, 4, figsize=(16, 4.2), sharey=True)
for ax, (fname, title) in zip(axes, files):
    y = col(fname)
    nfft, hop = 8192, 256
    win = np.hanning(nfft)
    frames = [y[i:i + nfft] * win for i in range(0, len(y) - nfft, hop)]
    S = np.abs(np.fft.rfft(np.array(frames), axis=1)).T
    Sdb = 20 * np.log10(S / (S.max() + 1e-12) + 1e-6)
    freqs = np.fft.rfftfreq(nfft, 1 / FS)
    times = np.arange(S.shape[1]) * hop / FS
    sel = (freqs >= 20) & (freqs <= 800)
    pc = ax.pcolormesh(times, freqs[sel], Sdb[sel], vmin=-70, vmax=0,
                       cmap="magma", shading="auto")
    ax.set_yscale("log")
    ax.set_ylim(20, 800)
    ax.set_title(title)
    ax.set_xlabel("time (s)")
axes[0].set_ylabel("Hz (log)")
fig.colorbar(pc, ax=axes, label="dB", fraction=0.015)
fig.suptitle("Kick pattern, 20–800 Hz — what the low end GENERATES vs silk (Ember defaults)")
fig.savefig(f"{D}/fig1_spectrograms.png", dpi=110, bbox_inches="tight")
print("fig1 done")

# ---------------------------------------------------------------- Fig 2
# Static transfer curves.
h, T = load("transfer.csv")
fig, ax = plt.subplots(figsize=(6.5, 5))
for i, (lbl, c) in enumerate([("silk 0.1", "#e07a5f"), ("silk 0.5", "#8ac6ea"),
                              ("silk 0.9", "#7bd8a8")]):
    ax.plot(T[:, 0], T[:, i + 1], label=lbl, color=c, lw=1.8)
ax.plot(T[:, 0], T[:, 0], color="#555c68", lw=0.8, ls="--", label="unity")
ax.axvspan(-0.45, 0.45, color="#ffffff", alpha=0.04, label="quiet signals")
ax.set_xlabel("input (pre-drive)")
ax.set_ylabel("output")
ax.legend()
ax.set_title("Static transfer (drive 2.05 applied) — silk reshapes the CURVE, no filter involved")
fig.savefig(f"{D}/fig2_transfer.png", dpi=110, bbox_inches="tight")
print("fig2 done")

# ---------------------------------------------------------------- Fig 3
# Gain staging: RMS + peak gain vs input level.
h, G = load("gain_vs_level.csv")
fig, axes = plt.subplots(1, 2, figsize=(11, 4.4), sharex=True)
colors = ["#e07a5f", "#8ac6ea", "#7bd8a8"]
for j, lbl in enumerate(["silk 0.1", "silk 0.5", "silk 0.9"]):
    axes[0].plot(G[:, 0], G[:, 1 + 2 * j], label=lbl, color=colors[j], lw=1.8)
    axes[1].plot(G[:, 0], G[:, 2 + 2 * j], label=lbl, color=colors[j], lw=1.8)
for ax, t in zip(axes, ["RMS gain (60 Hz)", "PEAK gain (60 Hz)"]):
    ax.axhline(0, color="#555c68", lw=0.8, ls="--")
    ax.set_xlabel("input level (dBFS)")
    ax.set_ylabel("gain (dB)")
    ax.set_title(t)
    ax.legend()
fig.suptitle("Gain staging decomposition: parallel lines = pure gain; converging at low level = normalised small-signal")
fig.savefig(f"{D}/fig3_gain_staging.png", dpi=110, bbox_inches="tight")
print("fig3 done")

# ---------------------------------------------------------------- Fig 4
# Harmonic budget: heatmap at 60 Hz + mud vs silk per fundamental.
h, H = load("harmonics.csv")
fig, axes = plt.subplots(1, 2, figsize=(12.5, 4.6))
sel = H[:, 0] == 60.0
M = H[sel][:, 2:11]  # h2..h10 dB, rows = silk 0..1
pc = axes[0].pcolormesh(np.arange(2, 11), H[sel][:, 1], M, vmin=-80, vmax=-10,
                        cmap="magma", shading="auto")
axes[0].set_xlabel("harmonic # (60 Hz fundamental → 120..600 Hz)")
axes[0].set_ylabel("silk (knob)")
axes[0].set_title("Harmonic levels from a 60 Hz kick tone (dB rel fundamental)")
fig.colorbar(pc, ax=axes[0], fraction=0.04)
for f0, c in [(40, "#e07a5f"), (60, "#f2cc8f"), (100, "#8ac6ea"),
              (200, "#7bd8a8"), (400, "#b78ae8")]:
    sel = H[:, 0] == f0
    axes[1].plot(H[sel][:, 1], H[sel][:, 11], label=f"{f0} Hz", color=c, lw=1.8)
axes[1].set_xlabel("silk (knob)")
axes[1].set_ylabel("Σ harmonics ≤ 800 Hz (dB rel fund)")
axes[1].set_title("The 'mud budget': total ≤ 800 Hz content generated, per fundamental")
axes[1].legend()
fig.savefig(f"{D}/fig4_harmonics.png", dpi=110, bbox_inches="tight")
print("fig4 done")

# ---------------------------------------------------------------- Fig 5
# Companding non-involvement.
h, C = load("companding.csv")
fig, ax = plt.subplots(figsize=(5, 3.6))
ax.bar([str(int(v)) for v in C[:, 0]], C[:, 1], color="#8ac6ea")
ax.set_xlabel("smooth highs (dB)")
ax.set_ylabel("mud ≤ 800 Hz (dB rel fund)")
ax.set_title("HF companding does not touch the low-end budget\n(60 Hz tone, silk 0.5)")
fig.savefig(f"{D}/fig5_companding.png", dpi=110, bbox_inches="tight")
print("fig5 done")

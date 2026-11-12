#!/usr/bin/env python3
"""Closed-loop check of the GENERATIVE ROOM: does the plugin, set to a
real space's dot on the map, actually sound like that space?

Renders the whole plugin's impulse response (tests/space_renders.rs) at
every Nth real space's map position — with SPACE 1 (generated) and SPACE
0 (the knobs' room, the baseline) — then measures the renders with
ir_measure.py, the same tool that measured the real IRs, and compares.

    space_check.py [--every 12]
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ir_measure as irm  # noqa: E402

ROOT = Path(__file__).resolve().parents[3]
CACHE = Path.home() / ".cache/engram-train/irs"
MODEL = Path(__file__).resolve().parent.parent / "src/engine/space_model.rs"


def model_map():
    """MAP_XY from the generated Rust (what the plugin uses)."""
    src = MODEL.read_text()
    body = src.split("pub(crate) const MAP_XY: [f32; ")[1].split("= [", 1)[1].split("];", 1)[0]
    return np.array([float(v) for v in body.split(",")]).reshape(-1, 2)


def render(listing, out, amount):
    env = dict(os.environ, ENGRAM_SPACE_LIST=str(listing), ENGRAM_SPACE_OUT=str(out), ENGRAM_SPACE_AMOUNT=str(amount))
    r = subprocess.run(["cargo", "test", "-p", "stave-engram-vst3", "--release", "--test", "space_renders", "--",
                        "--ignored", "--nocapture"], cwd=ROOT, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stdout[-3000:] + r.stderr[-3000:])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--every", type=int, default=12)
    a = ap.parse_args()
    d = np.load(CACHE / "features.npz", allow_pickle=True)
    X, names = d["X"], d["names"]
    lay = {r[0]: (int(r[1]), int(r[2])) for r in d["layout"]}
    xy = model_map()
    pick = list(range(0, len(X), a.every))
    listing = CACHE / "check" / "list.tsv"
    listing.parent.mkdir(parents=True, exist_ok=True)
    listing.write_text("".join(f"s{i}\t{xy[i, 0]:.6f}\t{xy[i, 1]:.6f}\n" for i in pick))

    got = {}
    for label, amount in (("generated", 1.0), ("knobs", 0.0)):
        out = CACHE / "check" / label
        render(listing, out, amount)
        rows = []
        for i in pick:
            _, f, err = irm.measure((f"s{i}", {None: out / f"s{i}.wav"}))
            if f is None:
                sys.exit(f"measure failed: {err}")
            rows.append(f)
        got[label] = np.array(rows)
    real = X[pick]

    d0 = lay["decay"][0]
    e0, e1 = lay["early"]
    def t60(F, k):
        return 10 ** F[:, d0 + k]
    def early_share(F):
        # The reflections' share of each band's reverb energy (slot 0).
        return 1 / (1 + np.exp(-F[:, e0:e1].reshape(len(F), 3, 24)[:, :, 0]))
    def pattern_corr(F, R):
        # Shape of the early pattern (log bins), per space, per band.
        f = F[:, e0:e1].reshape(len(F), 3, 24)[:, :, 1:]
        r = R[:, e0:e1].reshape(len(R), 3, 24)[:, :, 1:]
        out = []
        for i in range(len(F)):
            for b in range(3):
                out.append(np.corrcoef(f[i, b], r[i, b])[0, 1])
        return np.nanmedian(out)

    # What the engine was TOLD: the decoder at each dot's map latent.
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import room_vae as rv  # noqa: E402
    v = np.load(CACHE / "room_vae.npz", allow_pickle=True)
    p = {k: v[k] for k in ("W1", "b1", "W2", "b2", "W3", "b3", "W4", "b4", "W5", "b5")}
    lat = v["latent"]
    # The engine's residual MEMORY (space.rs `remembered`).
    Xall = d["X"]
    dec_all = rv.decode(p, lat)[1] * v["std"] + v["mean"]
    resid = Xall - dec_all
    D = ((lat[:, None, :] - lat[None, :, :]) ** 2).sum(-1)
    np.fill_diagonal(D, np.inf)
    s_mem = 0.35 * float(np.median(np.sqrt(D.min(1))))
    def remembered(z):
        d2 = ((lat - z) ** 2).sum(1)
        w = np.exp(-0.5 * (d2 - d2.min()) / s_mem ** 2)
        return (w[:, None] * resid).sum(0) / w.sum() * np.exp(-0.5 * d2.min() / s_mem ** 2)
    K = -0.5 / 0.010 ** 2
    told = []
    for i in pick:
        d2 = ((xy - xy[i]) ** 2).sum(1)
        w = np.exp(K * (d2 - d2.min()))
        z = (w[:, None] * lat).sum(0) / w.sum()
        y = rv.decode(p, z[None])[1][0] * v["std"] + v["mean"]
        told.append(y + remembered(z))
    told = np.array(told)
    print(f"engine fidelity — plugin vs what it was TOLD: early pattern corr {pattern_corr(got['generated'], told):.2f}, "
          f"early share {np.median(early_share(got['generated'])):.2f} vs {np.median(early_share(told)):.2f} "
          f"(corr {np.corrcoef(early_share(got['generated']).ravel(), early_share(told).ravel())[0, 1]:.2f})")
    print(f"model fidelity  — TOLD vs real: early pattern corr {pattern_corr(told, real):.2f}, "
          f"T60@1k |log2 err| {np.median(np.abs(np.log2(t60(told, 3) / t60(real, 3)))):.2f}\n")
    print(f"{len(pick)} real spaces; the plugin at each one's map dot vs the real IR\n")
    print(f"{'':28}{'generated':>12}{'knobs':>12}")
    for k, hz in ((0, 125), (3, 1000), (6, 8000)):
        row = [np.median(np.abs(np.log2(t60(got[l], k) / t60(real, k)))) for l in ("generated", "knobs")]
        print(f"T60 @{hz:>5} Hz |log2 err|  {row[0]:>12.2f}{row[1]:>12.2f}   (1.0 = off by 2x)")
    for l in ("generated", "knobs"):
        c = np.corrcoef(np.log(t60(got[l], 3)), np.log(t60(real, 3)))[0, 1]
        print(f"T60 @1 kHz corr with real ({l}) {c:.2f}")
    for l in ("generated", "knobs"):
        es_g, es_r = early_share(got[l]), early_share(real)
        print(f"early share, median ({l}): plugin {np.median(es_g):.2f} vs real {np.median(es_r):.2f};"
              f" corr {np.corrcoef(es_g.ravel(), es_r.ravel())[0, 1]:.2f}; pattern corr {pattern_corr(got[l], real):.2f}")
    t0 = lay["tone"][0]
    for l in ("generated", "knobs"):
        err = np.abs(got[l][:, t0:t0 + 7] - real[:, t0:t0 + 7]) * 10
        c = np.nanmedian([np.corrcoef(got[l][i, t0:t0 + 7], real[i, t0:t0 + 7])[0, 1] for i in range(len(real))])
        print(f"tone per octave ({l}): median |err| {np.median(err):.1f} dB, low {np.median(err[:, :2]):.1f}, high {np.median(err[:, 5:]):.1f}; shape corr {c:.2f}")
    s0 = lay["stereo"][0]
    for l in ("generated", "knobs"):
        print(f"late L/R corr: plugin median {np.median(got[l][:, s0 + 1]):.2f} vs real {np.median(real[:, s0 + 1]):.2f}; "
              f"corr {np.corrcoef(got[l][:, s0 + 1], real[:, s0 + 1])[0, 1]:.2f}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Engram generative room — a VAE over the library's spaces.

Learns a small LATENT SPACE OF ROOMS from ir_measure.py's rows: every real
IR is a point, and any point decodes to a plausible room — in between two
real spaces, or past them. The engine decodes the latent into its early
reflections and its late network's decay, tone, density and shape.

Pure numpy (tiny: 104 → 64 → L → 64 → 104), hand-written backprop with a
gradient check. Features are z-scored; each measurement GROUP gets a total
weight (so 72 early bins don't drown out 7 decay bands).

    room_vae.py --sweep          held-out reconstruction for L = 2, 3, 4, 8
    room_vae.py --latent 4       train on all spaces → room_vae.npz
    room_vae.py --gradcheck
"""

import argparse
import sys
from pathlib import Path

import numpy as np

DATA = Path.home() / ".cache/engram-train/irs/features.npz"
OUT = Path.home() / ".cache/engram-train/irs"
GROUP_WEIGHT = {"early": 2.0, "decay": 2.0, "tone": 1.0, "density": 1.0, "shape": 1.0, "stereo": 0.5, "predelay": 0.5}
HIDDEN = 64


def load():
    d = np.load(DATA, allow_pickle=True)
    X = d["X"].astype(np.float64)
    lay = {r[0]: (int(r[1]), int(r[2])) for r in d["layout"]}
    mean, std = X.mean(0), np.maximum(X.std(0), 1e-3)
    w = np.zeros(X.shape[1])
    total = sum(GROUP_WEIGHT.values())
    for k, (a, b) in lay.items():
        w[a:b] = GROUP_WEIGHT[k] / total * X.shape[1] / (b - a)
    return X, (X - mean) / std, mean, std, w, lay, d["names"], d["categories"]


def init(n, L, rng):
    def lin(i, o, s=1.0):
        return rng.normal(0, s / np.sqrt(i), (i, o)), np.zeros(o)
    p = {}
    p["W1"], p["b1"] = lin(n, HIDDEN)
    p["W2"], p["b2"] = lin(HIDDEN, L)
    p["W3"], p["b3"] = lin(HIDDEN, L, 0.1)
    p["b3"] -= 2.0
    p["W4"], p["b4"] = lin(L, HIDDEN)
    p["W5"], p["b5"] = lin(HIDDEN, n)
    return p


def encode(p, x):
    h = np.tanh(x @ p["W1"] + p["b1"])
    return h, h @ p["W2"] + p["b2"], h @ p["W3"] + p["b3"]


def decode(p, z):
    g = np.tanh(z @ p["W4"] + p["b4"])
    return g, g @ p["W5"] + p["b5"]


def step(p, x, w, beta, eps):
    """Loss and gradients (batch mean)."""
    m = len(x)
    h, mu, lv = encode(p, x)
    s = np.exp(0.5 * lv)
    z = mu + s * eps
    g, y = decode(p, z)
    r = y - x
    rec = 0.5 * (w * r * r).sum() / m
    kl = -0.5 * (1 + lv - mu * mu - np.exp(lv)).sum() / m
    dy = w * r / m
    gr = {"W5": g.T @ dy, "b5": dy.sum(0)}
    dg = (dy @ p["W5"].T) * (1 - g * g)
    gr["W4"], gr["b4"] = z.T @ dg, dg.sum(0)
    dz = dg @ p["W4"].T
    dmu = dz + beta * mu / m
    dlv = dz * eps * 0.5 * s + beta * 0.5 * (np.exp(lv) - 1) / m
    gr["W2"], gr["b2"] = h.T @ dmu, dmu.sum(0)
    gr["W3"], gr["b3"] = h.T @ dlv, dlv.sum(0)
    dh = (dmu @ p["W2"].T + dlv @ p["W3"].T) * (1 - h * h)
    gr["W1"], gr["b1"] = x.T @ dh, dh.sum(0)
    return rec + beta * kl, rec, kl, gr


def train(xs, w, L, epochs=3000, lr=3e-3, beta_max=1.0, decay=1e-4, seed=0, log=False):
    rng = np.random.default_rng(seed)
    p = init(xs.shape[1], L, rng)
    mo = {k: np.zeros_like(v) for k, v in p.items()}
    ve = {k: np.zeros_like(v) for k, v in p.items()}
    batch = 64
    t = 0
    for ep in range(epochs):
        beta = beta_max * min(1.0, ep / (epochs * 0.3))  # KL warm-up
        order = rng.permutation(len(xs))
        for at in range(0, len(xs), batch):
            xb = xs[order[at:at + batch]]
            eps = rng.normal(size=(len(xb), L))
            _, rec, kl, gr = step(p, xb, w, beta, eps)
            t += 1
            for k in p:
                gk = gr[k] + (decay * p[k] if k.startswith("W") else 0)
                mo[k] = 0.9 * mo[k] + 0.1 * gk
                ve[k] = 0.999 * ve[k] + 0.001 * gk * gk
                p[k] -= lr * (mo[k] / (1 - 0.9 ** t)) / (np.sqrt(ve[k] / (1 - 0.999 ** t)) + 1e-8)
        if log and ep % 500 == 0:
            print(f"  epoch {ep:>5}  rec {rec:7.2f}  kl {kl:6.2f}  beta {beta:.2f}", flush=True)
    return p


def r2_by_group(p, xs, lay):
    _, mu, _ = encode(p, xs)
    _, y = decode(p, mu)
    out = {}
    for k, (a, b) in lay.items():
        out[k] = 1 - ((y[:, a:b] - xs[:, a:b]) ** 2).sum() / ((xs[:, a:b] - 0) ** 2).sum()
    return out


def gradcheck():
    rng = np.random.default_rng(1)
    x = rng.normal(size=(5, 7))
    w = rng.uniform(0.5, 2, 7)
    global HIDDEN
    HIDDEN = 6
    p = init(7, 3, rng)
    eps = rng.normal(size=(5, 3))
    _, _, _, gr = step(p, x, w, 0.7, eps)
    worst = 0
    for k in p:
        for idx in np.ndindex(p[k].shape):
            old = p[k][idx]
            p[k][idx] = old + 1e-6
            lp = step(p, x, w, 0.7, eps)[0]
            p[k][idx] = old - 1e-6
            lm = step(p, x, w, 0.7, eps)[0]
            p[k][idx] = old
            num = (lp - lm) / 2e-6
            worst = max(worst, abs(num - gr[k][idx]) / max(1e-6, abs(num) + abs(gr[k][idx])))
    print(f"gradcheck worst relative error {worst:.2e}")
    return worst < 1e-5


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sweep", action="store_true")
    ap.add_argument("--gradcheck", action="store_true")
    ap.add_argument("--latent", type=int, default=4)
    ap.add_argument("--epochs", type=int, default=3000)
    ap.add_argument("--beta", type=float, default=1.0)
    a = ap.parse_args()
    if a.gradcheck:
        return 0 if gradcheck() else 1
    X, xs, mean, std, w, lay, names, cats = load()
    if a.sweep:
        rng = np.random.default_rng(7)
        test = rng.random(len(xs)) < 0.15
        print(f"{(~test).sum()} train / {test.sum()} held-out spaces; R² on HELD-OUT, by group")
        print("  L  beta  " + "  ".join(f"{k:>8}" for k in lay))
        for L in (2, 3, 4, 8):
            for beta in (a.beta,):
                p = train(xs[~test], w, L, a.epochs, beta_max=beta)
                r = r2_by_group(p, xs[test], lay)
                print(f"  {L}  {beta:4.1f}  " + "  ".join(f"{r[k]:8.2f}" for k in lay), flush=True)
        return 0
    p = train(xs, w, a.latent, a.epochs, beta_max=a.beta, log=True)
    _, mu, lv = encode(p, xs)
    np.savez(OUT / "room_vae.npz", mean=mean, std=std, latent=mu, latent_sd=np.exp(0.5 * lv),
             names=names, categories=cats, **p)
    r = r2_by_group(p, xs, lay)
    print("train R² " + "  ".join(f"{k} {v:.2f}" for k, v in r.items()))
    print(f"-> {OUT / 'room_vae.npz'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

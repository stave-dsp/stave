#!/usr/bin/env python3
"""Export the room VAE's DECODER and the MAP of real spaces to Rust.

Writes `src/engine/space_model.rs` (generated — do not edit):
* MEAN/STD of the anatomy features, the decoder's W4/b4/W5/b5;
* the MAP: every real space's 2D position (the latent's first two
  principal components, each spread through its own rank so the dots
  fill the square) with its 8D latent and category index — a cursor on
  the map becomes a kernel blend of nearby spaces' latents;
* GOLDEN decodes for the Rust test.

Derived numbers only (weights, coordinates) — no audio leaves the library.
"""

from pathlib import Path

import numpy as np

SRC = Path.home() / ".cache/engram-train/irs/room_vae.npz"
OUT = Path(__file__).resolve().parent.parent / "src/engine/space_model.rs"


def arr(name, a, ty="f32"):
    a = np.asarray(a).ravel()
    body = ", ".join(f"{float(v):.7e}" if ty == "f32" else str(int(v)) for v in a)
    return f"pub(crate) const {name}: [{ty}; {len(a)}] = [{body}];\n"


KERNEL = 0.010


def tsne(x, perplexity=25.0, iters=1500, seed=3):
    """A plain t-SNE (van der Maaten): 2-D positions that keep each
    room's latent NEIGHBOURS near it (PCA kept only a third of the
    latent's spread, and piled unrelated rooms together)."""
    n = len(x)
    d = ((x[:, None, :] - x[None, :, :]) ** 2).sum(-1)
    P = np.zeros((n, n))
    target = np.log(perplexity)
    for i in range(n):
        lo, hi, beta = 1e-6, 1e6, 1.0
        di = np.delete(d[i], i)
        for _ in range(60):
            p = np.exp(-di * beta)
            sp = p.sum() + 1e-12
            h = np.log(sp) + beta * (di * p).sum() / sp
            if abs(h - target) < 1e-4:
                break
            if h > target:
                lo = beta
                beta = beta * 2 if hi == 1e6 else (beta + hi) / 2
            else:
                hi = beta
                beta = (beta + lo) / 2
        P[i, np.arange(n) != i] = p / sp
    P = (P + P.T) / (2 * n)
    rng = np.random.default_rng(seed)
    y = rng.normal(0, 1e-4, (n, 2))
    vel = np.zeros_like(y)
    gains = np.ones_like(y)
    for it in range(iters):
        exag = 12.0 if it < 250 else 1.0
        sq = ((y[:, None, :] - y[None, :, :]) ** 2).sum(-1)
        num = 1.0 / (1.0 + sq)
        np.fill_diagonal(num, 0.0)
        Q = num / num.sum()
        pq = (exag * P - np.maximum(Q, 1e-12)) * num
        grad = 4.0 * ((np.diag(pq.sum(1)) - pq) @ y)
        gains = (gains + 0.2) * ((grad > 0) != (vel > 0)) + gains * 0.8 * ((grad > 0) == (vel > 0))
        gains = np.maximum(gains, 0.01)
        mom = 0.5 if it < 250 else 0.8
        vel = mom * vel - 200.0 * gains * grad
        y = y + vel
        y -= y.mean(0)
    return y


def layout(mu):
    """The MAP: t-SNE positions, then each room snapped to its own cell of
    a square grid (optimal assignment) — no two rooms collide, and
    neighbouring cells hold neighbouring rooms."""
    from scipy.optimize import linear_sum_assignment
    y = tsne(mu)
    y = (y - y.min(0)) / (y.max(0) - y.min(0))
    side = int(np.ceil(np.sqrt(len(mu))))
    cells = np.array([((c + 0.5) / side, (r + 0.5) / side) for r in range(side) for c in range(side)])
    cost = ((y[:, None, :] - cells[None, :, :]) ** 2).sum(-1)
    rows, cols = linear_sum_assignment(cost)
    xy = np.zeros_like(y)
    xy[rows] = cells[cols]
    xy = 0.04 + 0.92 * xy
    # How well the map keeps neighbours: latent distance to each room's
    # nearest room ON THE MAP, vs to its nearest room in the latent.
    D = np.sqrt(((xy[:, None] - xy[None]) ** 2).sum(-1))
    np.fill_diagonal(D, 9)
    L = np.sqrt(((mu[:, None] - mu[None]) ** 2).sum(-1))
    np.fill_diagonal(L, 9)
    print(f"map: {side}×{side} grid; latent distance to map-nearest room median {np.median(L[np.arange(len(mu)), D.argmin(1)]):.2f} (latent NN {np.median(L.min(1)):.2f})")
    return xy


def main():
    d = np.load(SRC, allow_pickle=True)
    mu = d["latent"]
    L = mu.shape[1]
    xy = layout(mu)
    cats = [str(x) for x in d["categories"]]

    # FAMILIES for the face's map labels (category keywords → a word).
    fam_rules = [("early", "EARLY"), ("drum", "DRUM ROOMS"), ("small", "ROOMS"), ("ambience", "ROOMS"),
                 ("plate", "PLATES"), ("spring", "SPRINGS"), ("hall", "HALLS"), ("chamber", "CHAMBERS"),
                 ("bigger", "VAST"), ("cathedral", "VAST"), ("real places", "PLACES"), ("strange", "STRANGE"),
                 ("experimental", "STRANGE"), ("texture", "STRANGE"), ("mirror", "STRANGE"), ("airwindows", "SYNTHETIC")]
    fam_names = sorted(set(f for _, f in fam_rules))
    def family(cat):
        return next((fam_names.index(f) for k, f in fam_rules if k in cat), fam_names.index("PLACES"))
    fam = np.array([family(c) for c in cats])
    # Display names: family + ordinal. Source file names and library
    # labels stay out of the generated code (the IRs are third-party).
    short, seen = [], {}
    for f in fam:
        seen[f] = seen.get(f, 0) + 1
        short.append(f"{fam_names[f]} {seen[f]}")
    # The latent → map projection's kernel: the median distance to the
    # nearest other space in the latent.
    D = ((mu[:, None, :] - mu[None, :, :]) ** 2).sum(-1)
    np.fill_diagonal(D, np.inf)
    sigma = float(np.median(np.sqrt(D.min(1))))
    print(f"latent nearest-neighbour median {sigma:.3f}")

    W4, b4, W5, b5 = d["W4"], d["b4"], d["W5"], d["b5"]
    # The decoder's RESIDUAL at every real room: what it really measures
    # minus what the decoder makes of its latent (raw feature units).
    # The engine adds nearby rooms' residuals back, so on a real room's
    # dot it gets that room exactly; between rooms, a blend; far from all
    # rooms, the pure decoder.
    X = np.load(Path.home() / ".cache/engram-train/irs/features.npz", allow_pickle=True)["X"]
    assert len(X) == len(mu)
    dec = (np.tanh(mu @ W4 + b4) @ W5 + b5) * d["std"] + d["mean"]
    resid = X - dec
    print(f"residual rms (std units) {np.sqrt(np.mean((resid / d['std']) ** 2)):.2f}")
    rng = np.random.default_rng(5)
    gz = rng.normal(size=(3, L))
    gy = (np.tanh(gz @ W4 + b4) @ W5 + b5) * d["std"] + d["mean"]

    src = [
        "//! GENERATED by train/export_space.py — do not edit.\n",
        "//! The room VAE's decoder and the map of the library's real spaces.\n\n",
        "#![allow(clippy::excessive_precision, clippy::unreadable_literal, clippy::large_const_arrays, clippy::approx_constant)]\n\n",
        f"pub(crate) const LATENT: usize = {L};\n",
        f"pub(crate) const HIDDEN: usize = {W4.shape[1]};\n",
        f"pub(crate) const FEATURES: usize = {W5.shape[1]};\n",
        f"pub(crate) const SPACES: usize = {len(mu)};\n",
        arr("MEAN", d["mean"]), arr("STD", d["std"]),
        arr("W4", W4), arr("B4", b4), arr("W5", W5), arr("B5", b5),
        arr("MAP_XY", xy), arr("MAP_LATENT", mu),
        "/// Display families (the face's map labels), indexed by MAP_FAMILY.\n",
        "pub(crate) const FAMILIES: [&str; %d] = [%s];\n" % (len(fam_names), ", ".join('"%s"' % n for n in fam_names)),
        arr("MAP_FAMILY", fam, "u8"),
        "/// Each space's display name.\n",
        "pub(crate) const NAMES: [&str; %d] = [%s];\n" % (len(short), ", ".join('"%s"' % n for n in short)),
        f"/// The latent → map projection's kernel width (latent units).\npub(crate) const LATENT_SIGMA: f32 = {sigma:.5f};\n",
        "#[cfg(test)]\n" + arr("GOLDEN_Z", gz), "#[cfg(test)]\n" + arr("GOLDEN_FEATURES", gy),
        arr("RESIDUAL", resid),
    ]
    OUT.write_text("".join(src))
    print(f"{len(mu)} spaces, {len(fam_names)} families -> {OUT} ({OUT.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()

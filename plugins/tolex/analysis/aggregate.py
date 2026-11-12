#!/usr/bin/env python3
"""Stage 2: group analyses over corpus.json -> the modeling laws.

Names are re-parsed here via naming.py (full-corpus coverage: letter
positions, numbered-ordinal positions, ambient captures), so stage 1
does not need re-running after parser fixes.

Questions answered, in order of architectural weight:
 1. Mode-frequency stability: same speaker, different mic position —
    what fraction of spectral peaks match within 1/24 oct? Baseline:
    cross-speaker match rate. Decides M2's shape (fixed modal set +
    position weights vs per-position sets).
 2. Fine-structure correlation same-speaker vs cross-speaker.
 3. The ring law: T20 decay time vs frequency, population curve.
 4. Mode density and Q distributions per octave.
 5. Reflection-delay histogram (box geometry).
 6. Position macro effects: letter classes AND the numbered-ordinal
    rungs (1 = cap ... 5 = edge; monotonicity is the check that the
    ordinal reading is right).
 7. Excess group delay (min-phase-ness) + energy timeline.
 8. Ambient captures (ROOM/REAR/SIDE/SUM) as their own population —
    the M4 room/rear-mic targets. Never pooled with close-mic stats.
"""

import json
import random
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

sys.path.insert(0, str(Path(__file__).resolve().parent))
import naming

OUT = Path(__file__).resolve().parent.parent / "refs/analysis/out"
results = json.load(open(OUT / "corpus.json"))
random.seed(7)

GRID_N = 1600
GRID = 40.0 * (20000.0 / 40.0) ** (np.arange(GRID_N) / (GRID_N - 1))
RES_BAND = GRID[(GRID >= 500) & (GRID <= 8000)]

for r in results:
    r.update(naming.parse(r["file"]))

close = [r for r in results if r["klass"] == "close"]
ambient = [r for r in results if r["klass"] == "ambient"]
unknown = [r for r in results if r["klass"] == "unknown"]
print(
    f"corpus: {len(results)} IRs -> close {len(close)}, ambient {len(ambient)}, unknown {len(unknown)}"
)
if unknown:
    for r in unknown[:10]:
        print("  UNPARSED:", r["file"])


def pctl(v, p):
    return float(np.percentile(np.asarray(v, dtype=float), p))


def fmt3(v):
    return f"{pctl(v,10):.2f}/{pctl(v,50):.2f}/{pctl(v,90):.2f}"


# ------------------------------------------------------------------ 1+2
def match_rate(a, b, tol_oct=1 / 24):
    fa, fb = np.asarray(a["peak_freqs"]), np.asarray(b["peak_freqs"])
    if len(fa) == 0 or len(fb) == 0:
        return None
    lo, hi = fb * 2 ** (-tol_oct), fb * 2 ** (tol_oct)
    hits = sum(1 for f in fa if np.any((f >= lo) & (f <= hi)))
    return hits / len(fa)


def resid_corr(a, b):
    ra, rb = np.asarray(a["resid_sig"]), np.asarray(b["resid_sig"])
    n = min(len(ra), len(rb))
    return float(np.corrcoef(ra[:n], rb[:n])[0, 1])


by_speaker = defaultdict(list)
for r in close:
    by_speaker[r["speaker"]].append(r)
speakers = sorted(by_speaker)
print(f"speakers ({len(speakers)}):", ", ".join(speakers))

same_match, same_corr, same_mic_match = [], [], []
for spk, rs in by_speaker.items():
    pool = random.sample(rs, min(len(rs), 40))
    for i in range(len(pool)):
        for j in range(i + 1, len(pool)):
            a, b = pool[i], pool[j]
            if a["pos"] == b["pos"] and a["idx"] == b["idx"]:
                continue
            m = match_rate(a, b)
            if m is not None:
                same_match.append(m)
                same_corr.append(resid_corr(a, b))
                if a["mic"] == b["mic"]:
                    same_mic_match.append(m)

cross_match, cross_corr = [], []
flat = [r for rs in by_speaker.values() for r in random.sample(rs, min(len(rs), 15))]
for _ in range(4000):
    a, b = random.sample(flat, 2)
    if a["speaker"] == b["speaker"]:
        continue
    m = match_rate(a, b)
    if m is not None:
        cross_match.append(m)
        cross_corr.append(resid_corr(a, b))

print("== 1. mode-frequency stability (peak match within 1/24 oct) ==")
print(f"same speaker, diff position   p10/med/p90: {fmt3(same_match)}")
print(f"same speaker + same mic       p10/med/p90: {fmt3(same_mic_match)}")
print(f"cross speaker (baseline)      p10/med/p90: {fmt3(cross_match)}")
print("== 2. fine-structure residual correlation ==")
print(f"same speaker {fmt3(same_corr)}   cross speaker {fmt3(cross_corr)}")

# ------------------------------------------------------------------ 3
bands = sorted({float(k) for r in close for k in r["t20"]})
t_by_band = {b: [] for b in bands}
for r in close:
    for k, v in r["t20"].items():
        t_by_band[float(k)].append(v * 1000.0)

# The instrument's own floor: a 1/3-octave filter rings by itself
# (T60 and bandwidth are Fourier-dual). Any band whose median is not
# comfortably above the delta-impulse floor is measuring the FILTER.
from analyze_corpus import band_t20  # noqa: E402
delta = np.zeros(24000)
delta[100] = 1.0
floors = {}
for b in bands:
    fl = band_t20(delta, b)
    floors[b] = (fl or 0.0) * 1000.0
print("== 3. ring law: decay ms per 1/3 octave (close mics) — with instrument floor ==")
for b in bands:
    v = t_by_band[b]
    fl = floors[b]
    med = pctl(v, 50)
    flag = "  FLOOR-LIMITED" if med < 2.0 * fl else ""
    print(f"  {b:7.0f} Hz  {fmt3(v)}  floor {fl:6.1f}{flag}")
print("  (trust bands where median >= 2x floor; LF ring targets come")
print("   from T/S alignment physics, not this instrument)")

# ------------------------------------------------------------------ 4
oct_edges = [(500, 1000), (1000, 2000), (2000, 4000), (4000, 8000)]
dens = {e: [] for e in oct_edges}
qs = {e: [] for e in oct_edges}
proms = {e: [] for e in oct_edges}
for r in close:
    f = np.asarray(r["peak_freqs"])
    q = np.asarray(r["peak_qs"])
    p = np.asarray(r["peak_proms"])
    for lo, hi in oct_edges:
        sel = (f >= lo) & (f < hi)
        dens[(lo, hi)].append(int(np.sum(sel)))
        qs[(lo, hi)].extend(q[sel].tolist())
        proms[(lo, hi)].extend(p[sel].tolist())
print("== 4. modes per octave / Q / prominence dB (close mics) ==")
for e in oct_edges:
    print(f"  {e[0]}-{e[1]} Hz: count {fmt3(dens[e])}  Q {fmt3(qs[e])}  prom {fmt3(proms[e])}")

# ------------------------------------------------------------------ 5
refl_t = [t for r in close for (t, a) in r["reflections_ms"]]
refl_top = [r["reflections_ms"][0][0] for r in close if r["reflections_ms"]]
print("== 5. cepstral structure, window 0.05-6 ms (close mics) ==")
print(f"all detected lags ms    {fmt3(refl_t)}")
print(f"strongest per IR ms     {fmt3(refl_top)}")
edges = [(0.05, 0.1, "cone envelope (<3 cm)"), (0.1, 0.3, "cap/cavity (3-10 cm)"),
         (0.3, 1.0, "baffle/box (10-34 cm)"), (1.0, 6.0, "beyond-box")]
n = max(len(refl_t), 1)
for lo, hi, label in edges:
    frac = sum(1 for t in refl_t if lo <= t < hi) / n
    print(f"  {lo:4.2f}-{hi:4.2f} ms {label:22} {100*frac:5.1f}% of lags")
print("  (sub-0.1 ms lags ride the spectral-envelope boundary of the")
print("   cepstrum — geometry reads there are indicative, not literal)")

# ------------------------------------------------------------------ 6
print("== 6a. position macro, letter classes (tilt dB / hf corner Hz) ==")
by_pos = defaultdict(list)
for r in close:
    if r["pos"] != "P":
        by_pos[r["pos"]].append(r)
for pos in sorted(by_pos, key=lambda p: -len(by_pos[p])):
    rs = by_pos[pos]
    if len(rs) < 30:
        continue
    print(
        f"  {pos:4} (n={len(rs):4}) tilt {fmt3([x['tilt_db'] for x in rs])}  "
        f"corner {fmt3([x['hf_corner'] for x in rs])}"
    )
print("== 6b. numbered-ordinal rungs (1 = cap ... 5 = edge?) ==")
by_idx = defaultdict(list)
for r in close:
    if r["pos"] == "P" and r["idx"]:
        by_idx[int(r["idx"])].append(r)
med_tilts = {}
for idx in sorted(by_idx):
    rs = by_idx[idx]
    med_tilts[idx] = pctl([x["tilt_db"] for x in rs], 50)
    print(
        f"  P{idx}  (n={len(rs):4}) tilt {fmt3([x['tilt_db'] for x in rs])}  "
        f"corner {fmt3([x['hf_corner'] for x in rs])}"
    )
if len(med_tilts) >= 3:
    ks = sorted(med_tilts)
    mono = all(med_tilts[ks[i]] >= med_tilts[ks[i + 1]] for i in range(len(ks) - 1))
    print(f"  ordinal tilt monotonic cap->edge: {mono}")

# ------------------------------------------------------------------ 7
print("== 7. excess group delay rms ms (200-5k, bulk removed, close) ==")
print(f"  {fmt3([r['gd_dev_ms'] for r in close])}")
print("== energy timeline (close mics) ==")
for k in ["t10_ms", "t50_ms", "t90_ms", "direct_frac", "early_frac", "late_frac"]:
    print(f"  {k:12} {fmt3([r[k] for r in close])}")

# ------------------------------------------------------------------ 8
print("== 8. ambient captures — the M4 room/rear targets ==")
by_kind = defaultdict(list)
for r in ambient:
    key = "RML/RMS" if r["kind"] in ("RML", "RMS") else r["kind"]
    by_kind[key].append(r)
for kind in sorted(by_kind, key=lambda k: -len(by_kind[k])):
    rs = by_kind[kind]
    print(
        f"  {kind:8} (n={len(rs):3}) t90 {fmt3([x['t90_ms'] for x in rs])} ms  "
        f"late {fmt3([x['late_frac'] for x in rs])}  "
        f"gd_dev {fmt3([x['gd_dev_ms'] for x in rs])} ms  "
        f"tilt {fmt3([x['tilt_db'] for x in rs])}"
    )

# ------------------------------------------------------------------ tolex, same instrument
tolex_path = OUT / "tolex_ir.f32"
if tolex_path.exists():
    from analyze_corpus import analyze  # noqa: E402
    tx = np.fromfile(tolex_path, dtype="<f4").astype(np.float64)
    t = analyze(tx)
    print("== 9. tolex M1 through the SAME instrument (refs comparisons above) ==")
    f = np.asarray(t["peak_freqs"])
    octs = np.log2(6000 / 700)
    print(f"  modes 0.7-6k /oct: {np.sum((f>=700)&(f<=6000))/octs:.2f}   "
          f"(population {fmt3([np.sum((np.asarray(r['peak_freqs'])>=700)&(np.asarray(r['peak_freqs'])<=6000))/octs for r in close])})")
    for fc in (1000.0, 1260.1):
        key = [b for b in t["t20"] if abs(float(b) - fc) < 2]
        tv = t["t20"][key[0]] * 1000.0 if key else float("nan")
        print(f"  decay @{fc:.0f} Hz: {tv:.1f} ms   (population {fmt3(t_by_band[min(bands, key=lambda b: abs(b-fc))])})")
    print(f"  strongest cepstral lag: "
          f"{t['reflections_ms'][0][0]:.3f} ms" if t["reflections_ms"] else "  no cepstral peak")
    print(f"  excess GD dev: {t['gd_dev_ms']:.3f} ms   (population {fmt3([r['gd_dev_ms'] for r in close])})")
else:
    print("== 9. tolex row skipped (run the release fidelity test to export tolex_ir.f32) ==")

# ------------------------------------------------------------------ plots
fig, ax = plt.subplots(figsize=(8, 4.5))
med = [pctl(t_by_band[b], 50) for b in bands]
p10 = [pctl(t_by_band[b], 10) for b in bands]
p90 = [pctl(t_by_band[b], 90) for b in bands]
ax.fill_between(bands, p10, p90, alpha=0.3, label="p10-p90")
ax.plot(bands, med, "o-", label="median")
ax.set(xscale="log", xlabel="Hz", ylabel="decay ms (T20→T60)",
       title=f"The ring law: per-band decay ({len(close)} close-mic IRs)")
ax.grid(True, which="both", alpha=0.3)
ax.legend()
fig.tight_layout()
fig.savefig(OUT / "ring_law.png", dpi=130)

fig, ax = plt.subplots(figsize=(8, 4.5))
ax.hist(same_match, bins=40, alpha=0.6, density=True, label="same speaker, diff position")
ax.hist(cross_match, bins=40, alpha=0.6, density=True, label="different speaker")
ax.set(xlabel="fraction of peaks matched within 1/24 oct", ylabel="density",
       title="Mode frequencies are the speaker's identity")
ax.legend()
fig.tight_layout()
fig.savefig(OUT / "mode_stability.png", dpi=130)

fig, ax = plt.subplots(figsize=(8, 4.5))
ax.hist(refl_t, bins=60, range=(0.3, 6.0))
ax.set(xlabel="reflection delay ms", ylabel="count",
       title="Cepstral reflections, close mics (box geometry)")
fig.tight_layout()
fig.savefig(OUT / "reflections.png", dpi=130)

spk = speakers[0]
rs = [r for r in by_speaker[spk] if r["mic"] == by_speaker[spk][0]["mic"]][:6]
fig, ax = plt.subplots(figsize=(9, 4.5))
for r in rs:
    ax.plot(RES_BAND[: len(r["resid_sig"])], r["resid_sig"], lw=0.8,
            label=f"{r['pos']}{r['idx'] or ''}")
ax.set(xscale="log", xlabel="Hz", ylabel="fine structure dB (residual)",
       title=f"{spk}: same speaker, mic moved")
ax.legend(fontsize=7)
ax.grid(True, which="both", alpha=0.3)
fig.tight_layout()
fig.savefig(OUT / "position_overlay.png", dpi=130)

# Ordinal position sweep: median fine structure per rung.
fig, ax = plt.subplots(figsize=(9, 4.5))
for idx in sorted(by_idx):
    rs = by_idx[idx]
    sigs = np.array(
        [r["resid_sig"][: len(RES_BAND)] for r in rs if len(r["resid_sig"]) >= len(RES_BAND)]
    )
    if len(sigs):
        ax.plot(RES_BAND, np.median(sigs, axis=0), lw=1.0, label=f"rung {idx} (n={len(rs)})")
ax.set(xscale="log", xlabel="Hz", ylabel="median fine structure dB",
       title="Numbered positions: the cap→edge sweep, population median")
ax.legend(fontsize=8)
ax.grid(True, which="both", alpha=0.3)
fig.tight_layout()
fig.savefig(OUT / "ordinal_sweep.png", dpi=130)

print("plots ->", OUT)

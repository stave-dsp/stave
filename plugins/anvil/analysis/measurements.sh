#!/bin/zsh
# The anvil measurement battery. FRESHNESS-GATED: builds via
# fresh-bundle.sh and prints the render's self-reported build
# stamp first — a battery line without a fresh stamp is fiction
# (the poisoned-measurement day's law, 2026-07-31).
set -e
set -o pipefail
bash scripts/fresh-bundle.sh stave-anvil-vst3 plugins/anvil 2>/dev/null | tail -2
SETS=(--set gain=20 --set squash=12 --set tight=110 --set bass=1.5 --set mid=-3 --set treble=2 --set presence=2 --set depth=2 --set focus=0.5)
./target/release/stave-bench "$PWD/build/StaveAnvil.vst3" --rate 44100 --seconds 12 --signal wav:/tmp/rb_testing.wav "${SETS[@]}" --out /tmp/rbt_mig.wav >/dev/null 2>&1
./target/release/stave-bench "$PWD/build/StaveAnvil.vst3" --rate 48000 --seconds 5 --signal wav:/tmp/chug_probe.wav "${SETS[@]}" --out /tmp/chug_mig.wav >/dev/null 2>&1
python3 - <<'PYEOF'
import numpy as np, struct
def load(p):
    raw = open(p,'rb').read()
    i = 12
    while i < len(raw):
        cid = raw[i:i+4]; sz = struct.unpack('<I', raw[i+4:i+8])[0]
        if cid == b'fmt ': fmt = struct.unpack('<HHIIHH', raw[i+8:i+24])
        elif cid == b'data': data = raw[i+8:i+8+sz]; break
        i += 8 + sz + (sz & 1)
    return np.frombuffer(data, dtype=np.float32).astype(np.float64)[::fmt[1]], fmt[2]
di, fs = load('/tmp/rb_testing.wav')
hop = int(0.005*fs)
edi = np.array([np.sqrt((di[i:i+hop]**2).mean()) for i in range(0, len(di)-hop, hop)])
edi = 20*np.log10(edi+1e-12)
onsets = []
for j in range(4, len(edi)-2):
    past = edi[max(0,j-16):j-1].max()
    if edi[j] > past + 8 and (not onsets or (j*hop/fs) - onsets[-1] > 0.12):
        onsets.append(j*hop/fs)
pm = [t for t in onsets if 3.4 <= t <= 10.5]
y, _ = load('/tmp/rbt_mig.wav')
feats = []
for t in pm:
    a = int(t*fs); n = int(0.15*fs)
    seg = y[a:a+n]
    r = 20*np.log10(np.sqrt((seg**2).mean())+1e-12)
    X = np.abs(np.fft.rfft(seg*np.hanning(n)))**2; f = np.fft.rfftfreq(n, 1/fs)
    m = (f>70)&(f<7000)
    cen = np.exp((np.log(f[m])*X[m]).sum()/max(X[m].sum(),1e-18))
    feats.append((r, cen))
feats = np.array(feats)
e = np.array([np.sqrt((y[i:i+hop]**2).mean()) for i in range(0, len(y)-hop, hop)])
e = 20*np.log10(e+1e-12)
d = [e[int(a/0.005):int(b/0.005)].max() - e[int(a/0.005)+4:int(b/0.005)+2].min() for a,b in zip(pm[:-1],pm[1:])]
print(f"MIG: rms spread {feats[:,0].std():.2f}  timbre {np.std(1200*np.log2(feats[:,1]/feats[:,1].mean())):.0f}c  troughs {np.median(d):.1f}  (plini .99/610/9.8, 051 .32/372/5.2)")
# crest+THD proxy on chug probe (the wall_stands dimensions)
x, fs2 = load('/tmp/chug_mig.wav')
t2 = x[fs2:]
crest = np.sqrt((t2**2).mean())/np.abs(t2).max()
def bands(p):
    xx, fs3 = load(p); xx = xx[int(0.1*fs3):]
    seg = 1<<14; acc = np.zeros(seg//2+1); w = np.hanning(seg); c = 0
    for s in range(0, len(xx)-seg, seg//2):
        acc += np.abs(np.fft.rfft(xx[s:s+seg]*w))**2; c += 1
    P = acc/c; f = np.fft.rfftfreq(seg, 1/fs3)
    def bd(lo, hi):
        m = (f>=lo)&(f<hi); return 10*np.log10(P[m].mean()+1e-24)
    ref = bd(100, 6000)
    return {n: bd(lo,hi)-ref for lo,hi,n in [(40,80,"sub"),(80,150,"depth"),(150,300,"fat"),(300,600,"body"),(600,1200,"mid"),(1200,2500,"himid"),(2500,5500,"bite"),(5500,9000,"pres"),(9000,14000,"fizz")]}
a = bands('/tmp/chug_mig.wav'); p = bands('/tmp/chug_plini_0.65.wav')
print(f"crest {crest:.2f} | bands vs plini: " + " ".join(f"{k}:{a[k]-p[k]:+.1f}" for k in a))
PYEOF

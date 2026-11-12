import numpy as np, struct, sys
fs = 48000
def load(p):
    raw = open(p,'rb').read()
    i = 12
    while i < len(raw):
        cid = raw[i:i+4]; sz = struct.unpack('<I', raw[i+4:i+8])[0]
        if cid == b'fmt ': fmt = struct.unpack('<HHIIHH', raw[i+8:i+24])
        elif cid == b'data': data = raw[i+8:i+8+sz]; break
        i += 8 + sz + (sz & 1)
    return np.frombuffer(data, dtype=np.float32).astype(np.float64)[::fmt[1]]
def centroid(x, t0, dur=0.06):
    n = int(dur*fs)
    s = x[int(t0*fs):int(t0*fs)+n] * np.hanning(n)
    X = np.abs(np.fft.rfft(s))**2; f = np.fft.rfftfreq(len(s), 1/fs)
    m = (f>80)&(f<9000)
    return np.exp((np.log(f[m])*X[m]).sum()/X[m].sum())
def srms(x, t0, dur=0.03):
    n = int(dur*fs)
    s = x[int(t0*fs):int(t0*fs)+n]
    return 20*np.log10(np.sqrt((s**2).mean())+1e-12)
def report(name, path):
    y = load(path)
    lat = 0.0015  # ~66-70 samples, negligible vs 20ms windows
    # PM note 3 at 1.05s; OPEN at 1.55s
    pm_on, op_on = 3*0.35, 4*0.35 + 0.15
    c_pm_a, c_pm_s = centroid(y, pm_on+0.012), centroid(y, pm_on+0.15)
    c_op_a, c_op_s, c_op_t = centroid(y, op_on+0.012), centroid(y, op_on+0.25), centroid(y, op_on+1.0)
    r_pm_a, r_pm_s = srms(y, pm_on+0.010), srms(y, pm_on+0.16)
    r_op_a, r_op_s, r_op_t = srms(y, op_on+0.010), srms(y, op_on+0.25), srms(y, op_on+1.0)
    print(f"--- {name}")
    print(f"  PM  : centroid {c_pm_a:5.0f} -> {c_pm_s:5.0f} Hz (drift {c_pm_s-c_pm_a:+5.0f})   env {r_pm_a:6.1f} -> {r_pm_s:6.1f} dB ({r_pm_s-r_pm_a:+5.1f})")
    print(f"  OPEN: centroid {c_op_a:5.0f} -> {c_op_s:5.0f} -> {c_op_t:5.0f} Hz               env {r_op_a:6.1f} -> {r_op_s:6.1f} -> {r_op_t:6.1f} dB")
    print(f"  INPUT DIFFERENTIATION: centroid(open-pm)@sustain {c_op_s-c_pm_s:+5.0f} Hz   level(open-pm)@attack {r_op_a-r_pm_a:+5.1f} dB")
    return (c_op_s-c_pm_s, r_op_a-r_pm_a)
import numpy as np
# input reference
x = load('/tmp/react_probe.wav') if False else None
for n, p in [("input", "/tmp/react_probe.wav"), ("anvil", "/tmp/react_anvil.wav"), ("plini", "/tmp/react_plini.wav")]:
    report(n, p)

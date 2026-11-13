#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["neural-amp-modeler==0.13.0", "torch", "numpy"]
# ///
"""Reference outputs for stave-dsp's learned networks (DSP.md M2).

Builds small Neural Amp Modeler networks with seeded random weights,
exports each with NAM's own exporter (the `.nam` format the trainer
writes), loads the file back with NAM's own loader (`init_from_nam`) and
runs its PyTorch forward pass (float32, `pad_start=True`: the history
before the first sample is silence) on a fixed input. The `.nam` file and
`input output` pairs are written to crates/stave-dsp/tests/reference/nam/;
`tests/nam_parity.rs` holds stave's streaming networks to them.

Weights are rounded to four significant digits and the file reloaded, so
the fixture stays a few KB and both sides read exactly the same values
(decimal -> f64 -> f32 on each side).

The GRU fixture is torch.nn.GRU itself (NAM has no GRU), weights in
stave's documented layout.

Slimmable networks (DSP.md M7): `slimmable_*.nam` are WaveNets trained
for channel slicing (Atkinson 2025), exported by NAM's exporter, and each
`slimmable_*_c<N>.txt` is NAM's forward pass slimmed to N channels
(`set_slimming`). `container.nam` is a `SlimmableContainer` holding
NAM-exported submodels in the layout the trainer's export hooks read
(`config.submodels[i].model`, a whole `.nam` dict each), with each
`container_<i>.txt` the trainer's forward pass of submodel i.

    uv run scripts/nam-reference.py

Offline only; tests read the files.
"""
import json
import math
import pathlib
import sys
import tempfile
import types

# nam.train imports tkinter for its GUI; nothing here uses it.
_tk = types.ModuleType("tkinter")
_tk.filedialog = types.ModuleType("tkinter.filedialog")
sys.modules.setdefault("tkinter", _tk)
sys.modules.setdefault("tkinter.filedialog", _tk.filedialog)

import numpy as np  # noqa: E402
import torch  # noqa: E402
from nam.models._from_nam import init_from_nam  # noqa: E402
from nam.models.recurrent import LSTM  # noqa: E402
from nam.models.wavenet import WaveNet  # noqa: E402

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/stave-dsp/tests/reference/nam"
RATE = 48_000.0
N = 256


def stimulus() -> np.ndarray:
    """Two sines, a hot decaying burst and a stretch of silence: drives
    the activations from linear to saturated and back, then shows the
    settle."""
    n = np.arange(N)
    x = 0.4 * np.sin(2 * math.pi * 0.013 * n) + 0.2 * np.sin(2 * math.pi * 0.071 * n)
    burst = (n >= 96) & (n < 160)
    x = x + burst * 1.2 * np.exp(-(n - 96) / 40.0) * np.sin(2 * math.pi * 0.031 * n)
    x[208:] = 0.0
    return np.array([float(f"{v:.4g}") for v in x], dtype=np.float64)


def seed_weights(model: torch.nn.Module, seed: int, std: float) -> None:
    g = torch.Generator().manual_seed(seed)
    with torch.no_grad():
        for p in model.parameters():
            p.copy_(torch.randn(p.shape, generator=g) * std)


def round_file(d: dict) -> dict:
    d = dict(d)
    d["weights"] = [float(f"{w:.4g}") for w in d["weights"]]
    # Keep the fixture stable: no export date; loudness/gain are not read.
    d["metadata"] = {}
    return d


def export(model) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        model.export(pathlib.Path(tmp), basename="m")
        return json.loads(pathlib.Path(tmp, "m.nam").read_text())


def write(name: str, nam: dict, x: np.ndarray) -> None:
    model = init_from_nam(json.loads(json.dumps(nam)))
    model.eval()
    with torch.no_grad():
        y = model(torch.tensor(x, dtype=torch.float32), pad_start=True).numpy()
    assert y.shape == (N,), y.shape
    (OUT / f"{name}.nam").write_text(json.dumps(nam, separators=(",", ":")) + "\n")
    lines = [f"{repr(float(xi))} {repr(float(np.float32(yi)))}" for xi, yi in zip(x, y)]
    (OUT / f"{name}.txt").write_text("\n".join(lines) + "\n")
    peak = float(np.max(np.abs(y)))
    print(f"{name}: {len(nam['weights'])} weights, output peak {peak:.3f}")


def wavenet(layers, head_scale: float) -> WaveNet:
    return WaveNet.init_from_config(
        {"layers_configs": layers, "head": None, "head_scale": head_scale, "sample_rate": RATE}
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    x = stimulus()

    # The standard NAM shape, shrunk: two layer arrays, the second gated,
    # the first's head feeding the second's.
    standard = wavenet(
        [
            {"input_size": 1, "condition_size": 1, "channels": 4, "kernel_size": 3,
             "dilations": [1, 2, 4], "activation": "Tanh", "gated": False,
             "head": {"out_channels": 3, "kernel_size": 1, "bias": False}},
            {"input_size": 4, "condition_size": 1, "channels": 3, "kernel_size": 3,
             "dilations": [1, 2], "activation": {"name": "PairMultiply", "primary": "Tanh",
                                                 "secondary": "Sigmoid"},
             "head": {"out_channels": 1, "kernel_size": 1, "bias": True}},
        ],
        head_scale=0.3,
    )
    seed_weights(standard, 1, 0.5)
    write("wavenet_standard", round_file(export(standard)), x)

    # The 0.13 trainer's shape: one array, per-layer kernel sizes,
    # LeakyReLU, a head with a kernel of its own.
    packed = wavenet(
        [
            {"input_size": 1, "condition_size": 1, "channels": 3, "kernel_sizes": [3, 5, 2],
             "dilations": [1, 3, 7], "activation": {"name": "LeakyReLU", "negative_slope": 0.1},
             "gated": False, "head": {"out_channels": 1, "kernel_size": 4, "bias": True}},
        ],
        head_scale=0.5,
    )
    seed_weights(packed, 2, 0.5)
    write("wavenet_packed", round_file(export(packed)), x)

    # The less common switches: a bottleneck narrower than the channels,
    # an active head 1x1, blended gating and assorted activations.
    mixed = wavenet(
        [
            {"input_size": 1, "condition_size": 1, "channels": 4, "bottleneck": 3,
             "kernel_size": 2, "dilations": [1, 2, 4, 8],
             "activation": [
                 {"name": "PairBlend", "primary": "Tanh", "secondary": "Sigmoid"},
                 "Softsign",
                 {"name": "Hardtanh", "min_val": -0.5, "max_val": 0.7},
                 "SiLU",
             ],
             "head_1x1_config": {"active": True, "out_channels": 2, "groups": 1},
             "head": {"out_channels": 1, "kernel_size": 2, "bias": True}},
        ],
        head_scale=0.7,
    )
    seed_weights(mixed, 3, 0.6)
    write("wavenet_mixed", round_file(export(mixed)), x)

    lstm = LSTM(hidden_size=5, num_layers=2, sample_rate=RATE)
    seed_weights(lstm, 4, 0.6)
    write("lstm", round_file(export(lstm)), x)

    write_gru(x)
    write_slimmable(x)
    write_container(x)


def forward(nam: dict, x: np.ndarray, slimming=None) -> np.ndarray:
    model = init_from_nam(json.loads(json.dumps(nam)))
    model.eval()
    if slimming is not None:
        model._net.set_slimming(slimming)
    with torch.no_grad():
        y = model(torch.tensor(x, dtype=torch.float32), pad_start=True).numpy()
    assert y.shape == (N,), y.shape
    return y


def write_pairs(name: str, x: np.ndarray, y: np.ndarray) -> None:
    lines = [f"{repr(float(xi))} {repr(float(np.float32(yi)))}" for xi, yi in zip(x, y)]
    (OUT / f"{name}.txt").write_text("\n".join(lines) + "\n")


def write_slimmable(x: np.ndarray) -> None:
    """Two slimmable WaveNets (NAM 0.13: one layer array, channel
    slicing): a gated one with the exporter's default list (every width
    1..4) and a plain one restricted to widths 2, 4, 6. Each is run by
    NAM at a slimming ratio per width (index floor(ratio * len))."""
    slim = {"method": "slice_channels_uniform", "kwargs": {}}
    gated = wavenet(
        [{"input_size": 1, "condition_size": 1, "channels": 4, "kernel_size": 3,
          "dilations": [1, 2, 4], "activation": {"name": "PairMultiply", "primary": "Tanh",
                                               "secondary": "Sigmoid"},
          "head": {"out_channels": 1, "kernel_size": 1, "bias": True}, "slimmable": slim}],
        head_scale=0.4,
    )
    seed_weights(gated, 7, 0.5)
    plain = wavenet(
        [{"input_size": 1, "condition_size": 1, "channels": 6, "kernel_size": 2,
          "dilations": [1, 2, 4, 8], "activation": {"name": "LeakyReLU", "negative_slope": 0.1},
          "gated": False, "head": {"out_channels": 1, "kernel_size": 1, "bias": False},
          "slimmable": {"method": "slice_channels_uniform",
                        "kwargs": {"allowed_channels": [2, 4, 6]}}}],
        head_scale=0.6,
    )
    seed_weights(plain, 8, 0.35)
    for name, model, widths in (("slimmable_gated", gated, [1, 2, 3, 4]),
                                ("slimmable_plain", plain, [2, 4, 6])):
        nam = round_file(export(model))
        (OUT / f"{name}.nam").write_text(json.dumps(nam, separators=(",", ":")) + "\n")
        for i, c in enumerate(widths):
            # The middle of the ratio interval that selects width i.
            y = forward(nam, x, slimming=(i + 0.5) / len(widths))
            write_pairs(f"{name}_c{c}", x, y)
            print(f"{name} at {c} channels: output peak {float(np.max(np.abs(y))):.3f}")
        full = forward(nam, x)
        assert np.array_equal(full, forward(nam, x, slimming=1.0))


def write_container(x: np.ndarray) -> None:
    """A SlimmableContainer of three NAM-exported submodels, smallest
    first: a 2-channel WaveNet, a 5-channel gated WaveNet and a 2 x 8
    LSTM."""
    small = wavenet(
        [{"input_size": 1, "condition_size": 1, "channels": 2, "kernel_size": 3,
          "dilations": [1, 2], "activation": "Tanh", "gated": False,
          "head": {"out_channels": 1, "kernel_size": 1, "bias": True}}],
        head_scale=0.5,
    )
    seed_weights(small, 9, 0.5)
    medium = wavenet(
        [{"input_size": 1, "condition_size": 1, "channels": 5, "kernel_size": 3,
          "dilations": [1, 2, 4, 8], "activation": {"name": "PairMultiply", "primary": "Tanh",
                                                  "secondary": "Sigmoid"},
          "head": {"out_channels": 1, "kernel_size": 1, "bias": True}}],
        head_scale=0.3,
    )
    seed_weights(medium, 10, 0.4)
    lstm = LSTM(hidden_size=8, num_layers=2, sample_rate=RATE)
    seed_weights(lstm, 11, 0.5)
    submodels = [round_file(export(m)) for m in (small, medium, lstm)]
    container = {
        "version": submodels[0]["version"],
        "metadata": {},
        "architecture": "SlimmableContainer",
        "config": {"submodels": [{"model": m} for m in submodels]},
        "sample_rate": RATE,
    }
    (OUT / "container.nam").write_text(json.dumps(container, separators=(",", ":")) + "\n")
    for i, m in enumerate(submodels):
        y = forward(m, x)
        write_pairs(f"container_{i}", x, y)
        print(f"container submodel {i}: output peak {float(np.max(np.abs(y))):.3f}")


def write_gru(x: np.ndarray) -> None:
    """torch.nn.GRU (2 layers, hidden 4) and a linear head. Weights flat,
    per layer: w_ih (3H x I), w_hh (3H x H), b_ih (3H), b_hh (3H), gates
    r, z, n (PyTorch order); then head weight (H) and bias (1)."""
    hidden, layers = 4, 2
    gru = torch.nn.GRU(1, hidden, num_layers=layers, batch_first=True)
    head = torch.nn.Linear(hidden, 1)
    seed_weights(gru, 5, 0.6)
    seed_weights(head, 6, 0.6)
    flat = []
    for k in range(layers):
        for name in ("weight_ih", "weight_hh", "bias_ih", "bias_hh"):
            flat += getattr(gru, f"{name}_l{k}").detach().flatten().tolist()
    flat += head.weight.detach().flatten().tolist() + head.bias.detach().tolist()
    flat = [float(f"{w:.4g}") for w in flat]
    # Reload the rounded weights so the reference runs exactly them.
    i = 0
    with torch.no_grad():
        for k in range(layers):
            for name in ("weight_ih", "weight_hh", "bias_ih", "bias_hh"):
                p = getattr(gru, f"{name}_l{k}")
                p.copy_(torch.tensor(flat[i:i + p.numel()]).reshape(p.shape))
                i += p.numel()
        head.weight.copy_(torch.tensor(flat[i:i + hidden]).reshape(1, hidden))
        head.bias.copy_(torch.tensor(flat[i + hidden:i + hidden + 1]))
        y = head(gru(torch.tensor(x, dtype=torch.float32)[None, :, None])[0])[0, :, 0].numpy()
    spec = {"input_size": 1, "hidden_size": hidden, "num_layers": layers, "sample_rate": RATE,
            "weights": flat}
    (OUT / "gru.json").write_text(json.dumps(spec, separators=(",", ":")) + "\n")
    lines = [f"{repr(float(xi))} {repr(float(np.float32(yi)))}" for xi, yi in zip(x, y)]
    (OUT / "gru.txt").write_text("\n".join(lines) + "\n")
    print(f"gru: {len(flat)} weights, output peak {float(np.max(np.abs(y))):.3f}")


if __name__ == "__main__":
    main()

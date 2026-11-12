#!/usr/bin/env python3
"""Run every netlist in crates/stave-test/spice/ through ngspice into a
waveform next to it (<name>.ngspice.txt: time and value columns at 48 kHz).

Drives the shared library (brew install libngspice: a prebuilt bottle with
no dependencies) rather than the ngspice CLI. Offline only: waveforms are
committed, ngspice never runs in a test.
"""

import ctypes
import pathlib
import sys
import tempfile

LIBRARY = "/opt/homebrew/lib/libngspice.dylib"
SPICE_DIR = pathlib.Path(__file__).resolve().parent.parent / "crates/stave-test/spice"

PRINT = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_void_p)
EXIT = ctypes.CFUNCTYPE(
    ctypes.c_int, ctypes.c_int, ctypes.c_bool, ctypes.c_bool, ctypes.c_int, ctypes.c_void_p
)

errors = []


@PRINT
def on_print(text, _id, _user):
    line = text.decode(errors="replace")
    if line.startswith("stderr") and "warning" not in line.lower():
        errors.append(line)
    return 0


@EXIT
def on_exit(status, _unload, _quit, _id, _user):
    return 0


def main():
    try:
        spice = ctypes.CDLL(LIBRARY)
    except OSError:
        sys.exit(f"{LIBRARY} not found (brew install libngspice)")
    spice.ngSpice_Init(on_print, None, on_exit, None, None, None, None)
    spice.ngSpice_Command.argtypes = [ctypes.c_char_p]
    for cir in sorted(SPICE_DIR.glob("*.cir")):
        waveform = cir.with_suffix(".ngspice.txt")
        netlist = cir.read_text().replace("OUTFILE", str(waveform))
        with tempfile.NamedTemporaryFile("w", suffix=".cir", delete=False) as tmp:
            tmp.write(netlist)
        errors.clear()
        spice.ngSpice_Command(f"source {tmp.name}".encode())
        spice.ngSpice_Command(b"remcirc")
        pathlib.Path(tmp.name).unlink()
        if errors or not waveform.exists():
            sys.exit(f"{cir.name}: " + "".join(errors or ["no output written"]))
        points = sum(1 for _ in waveform.open())
        print(f"wrote {waveform.name} ({points} points)")


if __name__ == "__main__":
    main()

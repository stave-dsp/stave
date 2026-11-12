#!/usr/bin/env python3
"""Heirloom — the enclosure's PRINT for the Blender render: albedo,
roughness and a groove height map over the pedal's top face, laid out
from the same geometry as src/pedal/mod.rs (points; the pedal is
x 50…470, y 50…650).

    python3 face_print.py OUTDIR
"""

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

PX = 5  # pixels per point
PEDAL = (50.0, 50.0, 420.0, 600.0)
W, H = int(PEDAL[2] * PX), int(PEDAL[3] * PX)
KNOBS = [(135, 178), (260, 178), (385, 178), (135, 400), (260, 400), (385, 400)]
KNOB_R, SCALE_R = 27.0, 37.0
KNOB_LABELS = ["TIME", "REPEATS", "MIX", "WOBBLE", "RPM", "AGE"]
TOGGLES = [(135, 292), (260, 292), (385, 292)]
TOGGLE_NAMES = ["MACHINE", "WARP", "MEDIA"]
TOGGLE_WORDS = [["REVERSE", "OCTAVE", "BLOOM"], ["SINE", "WARP", "DRIFT"], ["VINYL", "TAPE", "VHS"]]
TOGGLE_POS = [1, 0, 2]
FOOT = [((150, 588), (150, 528)), ((370, 588), (370, 528))]
FOOT_LABELS = ["BYPASS", "HOLD"]
RECORD = (260.0, 680.0, 215.0, 80.0)

CREAM = np.array([0xEA, 0xDF, 0xC6]) / 255
RUST = np.array([0xA9, 0x54, 0x2E]) / 255
INK = (41, 36, 31)
CREAM_INK = (237, 227, 204)
LIT = (158, 69, 33)
DIM = (126, 116, 100)


def grid():
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    return PEDAL[0] + (x + 0.5) / PX, PEDAL[1] + (y + 0.5) / PX


def cover(d):
    return np.clip(0.5 - d * PX, 0.0, 1.0)


def vnoise(shape, scale, seed):
    rng = np.random.default_rng(seed)
    small = rng.random((max(2, int(shape[0] / scale)), max(2, int(shape[1] / scale))))
    return np.asarray(Image.fromarray((small * 255).astype(np.uint8)).resize((shape[1], shape[0]), Image.BICUBIC)) / 255


def sd_rrect(x, y, cx, cy, hw, hh, r):
    qx, qy = np.abs(x - cx) - hw + r, np.abs(y - cy) - hh + r
    return np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - r


def main(out):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    x, y = grid()
    d = sd_rrect(x, y, 260, 350, 210, 300, 20)

    # Powder coat: cream with a low mottle.
    mottle = 1 + 0.05 * (vnoise(x.shape, 300, 11) - 0.5) + 0.03 * (vnoise(x.shape, 30, 12) - 0.5)
    c = CREAM[None, None, :] * mottle[..., None]
    rough = np.full(x.shape, 0.42)
    height = np.zeros(x.shape)

    # Rust pinstripe.
    s = cover(np.abs(d + 12) - 0.8)[..., None] * 0.9
    c = c * (1 - s) + RUST * s

    # The warped record.
    rx, ry, rr, lab = RECORD
    dx, dy = x - rx, y - ry
    r = np.hypot(dx, dy)
    th = np.arctan2(dy, dx)
    edge = rr + 2.6 * np.sin(3 * th + 0.7) + 1.1 * np.sin(5 * th - 1.1)
    on = cover(r - edge)
    gap = np.ones_like(r)
    for g in (118, 146, 172):
        gap = np.minimum(gap, 0.35 + 0.65 * np.clip((np.abs(r - g) - 0.6) / 1.2, 0, 1))
    vinyl = np.stack([np.full_like(r, 0.018), np.full_like(r, 0.018), np.full_like(r, 0.021)], -1)
    # Label: rust → amber halves, cream rim, ring of dots, a spoke.
    split = np.clip(0.5 + 0.5 * np.clip(np.sin(th) * r / 0.6, -1, 1), 0, 1)[..., None]
    paper = (1 + 0.06 * (vnoise(x.shape, 60, 21) - 0.5))[..., None]
    a1, a2 = np.array([0xB4, 0x53, 0x2A]) / 255, np.array([0xD9, 0xA2, 0x3A]) / 255
    label = (a1 * (1 - split) + a2 * split) * paper
    rim = cover(np.abs(r - (lab - 6)) - 1.1)[..., None]
    label = label * (1 - rim) + CREAM * rim
    step = 2 * math.pi / 24
    da = np.round(th / step) * step
    dd = np.hypot(r * np.cos(th) - (lab - 15) * np.cos(da), r * np.sin(th) - (lab - 15) * np.sin(da))
    dots = cover(dd - 1.6)[..., None]
    label = label * (1 - dots) + np.array([0x3A, 0x1C, 0x10]) / 255 * dots
    spoke = ((np.cos(th - 0.9) > 0) & (r > 18) & (r < lab - 22)) * cover(np.abs(r * np.sin(th - 0.9)) - 1.4)
    label = label * (1 - spoke[..., None]) + CREAM * spoke[..., None]
    on_label = cover(r - lab)[..., None]
    rec = vinyl * (1 - on_label) + label * on_label
    hole = cover(r - 3.5)[..., None]
    rec = rec * (1 - hole) + 0.01 * hole
    c = c * (1 - on[..., None]) + rec * on[..., None]
    # Vinyl is glossy with grooves (height), the label is matte paper.
    lead = np.clip((r - lab - 6) / 8, 0, 1)
    grooves = 0.5 + 0.5 * np.sin(r * 2 * math.pi * 1.6)
    rough = np.where(on > 0.5, np.where(r < lab, 0.55, 0.30 + 0.12 * (1 - gap)), rough)
    height = np.where(on > 0.5, lead * gap * grooves * 0.6 + (1 - gap) * 0.0 + (r < lab) * 0.25, height)

    # Knob scales: eleven dots, the ends larger.
    for kx, ky in KNOBS:
        for k in range(11):
            a = math.radians(135 + 27 * k)
            tx, ty = kx + math.cos(a) * SCALE_R, ky + math.sin(a) * SCALE_R
            rad = 1.9 if k in (0, 10) else 1.25
            m = cover(np.hypot(x - tx, y - ty) - rad)[..., None]
            c = c * (1 - m) + np.array([0x2B, 0x26, 0x22]) / 255 * m

    img = Image.fromarray((np.clip(c, 0, 1) ** (1 / 1.0) * 255).astype(np.uint8))
    # Text, silkscreened (drawn in sRGB; the texture is sRGB).
    dr = ImageDraw.Draw(img, "RGBA")
    fut = "/System/Library/Fonts/Supplemental/Futura.ttc"

    def font(size, index=0):
        return ImageFont.truetype(fut, int(size * PX), index=index)

    def put(at, s, size, fill, index=4, track=0.0):
        f = font(size, index)
        px, py = (at[0] - PEDAL[0]) * PX, (at[1] - PEDAL[1]) * PX
        if track:
            widths = [dr.textlength(ch, font=f) for ch in s]
            total = sum(widths) + track * PX * (len(s) - 1)
            cx = px - total / 2
            for ch, w in zip(s, widths):
                dr.text((cx, py), ch, font=f, fill=fill, anchor="lm")
                cx += w + track * PX
        else:
            dr.text((px, py), s, font=f, fill=fill, anchor="mm")

    # Futura.ttc faces: 0 Medium, 1 Medium Italic, 2 Bold, 3 Condensed Medium, 4 Condensed ExtraBold
    put((260, 98), "HEIRLOOM", 25, INK, index=2, track=6.0)
    put((260, 119), "reverse machines  ·  played off worn media", 7.5, (107, 61, 38), index=0)
    for (kx, ky), name in zip(KNOBS, KNOB_LABELS):
        put((kx, ky + KNOB_R + 22), name, 10, INK, index=2, track=1.2)
    for t, (tx, ty) in enumerate(TOGGLES):
        put((tx, ty - 34), TOGGLE_NAMES[t], 8.5, INK, index=2, track=1.0)
        for k, word in enumerate(TOGGLE_WORDS[t]):
            put((tx + 17 + 2.45 * len(word), ty - 13 + 13 * k), word, 6.5, LIT if k == TOGGLE_POS[t] else DIM, index=2)
    for ((fx, _), (lx, ly)), name in zip(FOOT, FOOT_LABELS):
        put((fx, ly + 23), name, 7.5, CREAM_INK, index=2, track=1.0)

    img.save(out / "face_albedo.png")
    # Ink is a touch glossier than the powder coat and very slightly raised.
    ink = np.asarray(img.convert("L"), dtype=np.float64) / 255
    base = np.asarray(Image.fromarray((np.clip(c, 0, 1) * 255).astype(np.uint8)).convert("L"), dtype=np.float64) / 255
    printed = np.clip(np.abs(ink - base) * 4, 0, 1)
    rough = rough * (1 - 0.35 * printed)
    height = height + 0.15 * printed
    # Orange-peel grain on the powder coat.
    peel = vnoise(x.shape, 6, 31) - 0.5
    height = height + np.where(on > 0.5, 0.0, 0.25 * peel)
    vin = on * (1 - cover(r - lab - 4)) * np.clip(1 - (1 - gap) * 1.2, 0, 1)
    Image.fromarray((np.clip(vin, 0, 1) * 255).astype(np.uint8)).save(out / "face_vinyl.png")
    Image.fromarray((np.clip(rough, 0, 1) * 255).astype(np.uint8)).save(out / "face_rough.png")
    hb = Image.fromarray((np.clip(height * 0.5 + 0.5, 0, 1) * 65535).astype(np.uint16))
    hb.save(out / "face_height.png")
    print("wrote", out, W, H)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "build/heirloom-blender")

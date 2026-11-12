#!/usr/bin/env python3
"""Engram embedding — training set prep.

Builds the list of library recordings to learn from (`list.tsv`: wav path,
coarse label, fine label, seconds), from the sample library's manifest:

* everything playable except impulse responses — and NOTHING from the Test
  Corpus (nor any recording with the same name): that is the held-out set
  the embedding is judged on;
* COARSE label = the type-first category (Drums/One Shots/Kick,
  Instruments/Multisamples/Grand Piano …); none for kits (a kit folder
  mixes kicks and hats), renders, stems and archive audio;
* FINE label = the multisample's instrument folder (one instrument at many
  pitches — what teaches pitch invariance), else the recording itself;
* AIFF is converted to WAV once (afconvert) into the cache; Ableton's
  protected AIFC is skipped (never decoded).

Usage: prep.py [--library PATH] [--cache DIR]
"""
import argparse, csv, hashlib, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

ap = argparse.ArgumentParser()
ap.add_argument("--library", default=os.environ.get("ENGRAM_LIBRARY"), required="ENGRAM_LIBRARY" not in os.environ)
ap.add_argument("--cache", default=os.path.expanduser("~/.cache/engram-train"))
a = ap.parse_args()

LIB = a.library
ROOTS = ("Drums", "Instruments", "Loops", "Vocals", "FX", "Foley & Hits", "Source Material", "Stems", "Halflife")
LABELLED = ("Drums", "Instruments", "Loops", "Vocals", "FX")
os.makedirs(os.path.join(a.cache, "wav"), exist_ok=True)

held = set()
for root, _, files in os.walk(os.path.join(LIB, "Test Corpus")):
    held.update(f for f in files if not f.startswith("._"))

rows = [r for r in csv.DictReader(open(os.path.join(LIB, "_manifest.csv"))) if r["path"].startswith(ROOTS)]
out, skipped_held = [], 0


def protected(path):
    try:
        with open(path, "rb") as f:
            head = f.read(64)
        return head[:4] == b"FORM" and head[8:12] == b"AIFC" and b"able" in head
    except OSError:
        return True


def item(r):
    name = os.path.basename(r["path"])
    if name in held:
        return "held"
    src = os.path.join(LIB, r["path"])
    if not os.path.exists(src):
        return None
    if r["format"] != "wav":
        if protected(src):
            return None
        dst = os.path.join(a.cache, "wav", hashlib.sha1(r["path"].encode()).hexdigest()[:16] + ".wav")
        if not os.path.exists(dst):
            res = subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI24", src, dst], capture_output=True)
            if res.returncode != 0:
                return None
        src = dst
    parts = r["category"].split("/")
    coarse = ""
    if r["path"].startswith(LABELLED) and not r["path"].startswith("Drums/Kits"):
        coarse = "/".join(parts[:3])
    fine = os.path.dirname(r["path"]) if "/Multisamples/" in r["path"] else r["path"]
    return (src, coarse, fine, r["seconds"] or "0")


with ThreadPoolExecutor(8) as ex:
    for res in ex.map(item, rows):
        if res == "held":
            skipped_held += 1
        elif res:
            out.append(res)

path = os.path.join(a.cache, "list.tsv")
with open(path, "w") as f:
    for r in out:
        f.write("\t".join(r) + "\n")
coarse = {r[1] for r in out if r[1]}
print(f"{len(out)} recordings ({skipped_held} held out as Test Corpus), {len(coarse)} coarse classes -> {path}")

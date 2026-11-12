"""York filename taxonomy — one parser, used by both stages.

Observed schemes across the 12 packs (48k Natural Phase Singles):
  letter positions:  "<MIC>-<POS>[ <idx>]"     e.g. 57v-OA 2, 58-CNE,
                     idx may be 1-9 or A/B/C (KW's OA variants)
  numbered positions: "<MIC>-<n>[L|R]"         e.g. 121-3, U47-5 —
                     ordinal across the cone (1 = cap ... 5 = edge)
  ambient captures:  ROOM/REAR/RML/RMS/SIDE/SUM with -n, " n", L/R —
                     room pairs, rear-of-cab, side, and mix-sum shots.
                     NEVER pooled with close-mic population stats.

Sub-cab variants (MRSH MV30a/b, MTCH ESD H/M) stay part of the stem
and are treated as distinct speakers by keeping them in `speaker`.
"""

import re

AMBIENT_RE = re.compile(r"\b(?P<kind>ROOM|REAR|RML|RMS|SIDE|SUM)(?:[- ]?(?P<sub>[0-9]+[LR]?|[LR]))?$")
POS_TOKENS = "OA|CNE|CNT|CN|CE|FOA|FCN|DRK"
LETTER_RE = re.compile(
    rf"(?P<mic>[A-Za-z0-9+]+)-(?P<pos>{POS_TOKENS})(?: (?P<idx>[0-9ABC]))?$"
)
NUM_RE = re.compile(r"(?P<mic>[A-Za-z0-9+]+)-(?P<idx>\d)(?P<lr>[LR])?$")


def parse(name):
    """name: WAV basename or stem. Returns a dict:
    klass: 'close' | 'ambient' | 'unknown'
    mic, pos, idx: close-mic fields (pos 'P' = numbered-ordinal)
    kind: ambient subtype when klass == 'ambient'
    speaker: the stem minus the mic/position tail (identity incl.
             sub-cab variants like MV30a / ESD H).
    """
    stem = re.sub(r"\.wav$", "", name).replace("YA ", "").strip()

    m = AMBIENT_RE.search(stem)
    if m:
        return {
            "klass": "ambient",
            "kind": m.group("kind"),
            "mic": None,
            "pos": None,
            "idx": m.group("sub"),
            "speaker": stem[: m.start()].strip(),
        }
    m = LETTER_RE.search(stem)
    if m:
        return {
            "klass": "close",
            "kind": None,
            "mic": m.group("mic"),
            "pos": m.group("pos"),
            "idx": m.group("idx"),
            "speaker": stem[: m.start()].strip(),
        }
    m = NUM_RE.search(stem)
    if m:
        return {
            "klass": "close",
            "kind": None,
            "mic": m.group("mic"),
            "pos": "P",  # numbered-ordinal; idx carries the rung
            "idx": m.group("idx"),
            "speaker": stem[: m.start()].strip(),
        }
    return {"klass": "unknown", "kind": None, "mic": None, "pos": None, "idx": None, "speaker": stem}

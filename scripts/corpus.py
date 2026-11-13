#!/usr/bin/env python3
"""The reference corpus against the sample library (DSP.md R22).

Reads crates/stave-test/corpus.spec (the committed roles and selection
rules) and resolves them against the library STAVE_SAMPLE_LIBRARY names
exactly as stave_test::corpus does: _corpus.json sets, _manifest.csv
categories, directories; WAV only, never `._*` AppleDouble sidecars,
never AIFF (Ableton's protected AIFF-C is never decoded). Nothing is
copied and no audio leaves the library.

  corpus.py list [ROLE...]   each role's picks
  corpus.py check            missing roles, lock drift; exit 1 on a problem
  corpus.py lock             record each pick's SHA-256, size and time in
                             the local lock (STAVE_CORPUS_LOCK, default
                             ~/.config/stave/corpus.lock; never committed)

Run it through scripts/corpus.sh, which loads the local configuration.
Standard library only.
"""

import hashlib
import json
import os
import re
import struct
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = os.path.join(ROOT, "crates", "stave-test", "corpus.spec")


def glob_re(pattern):
    return re.compile("".join(".*" if c == "*" else "." if c == "?" else re.escape(c) for c in pattern) + r"\Z", re.S)


def glob(pattern, text):
    return glob_re(pattern).match(text) is not None


# ---- the spec ---------------------------------------------------------------

def parse_spec(text):
    blocks = []
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("role "):
            blocks.append([line])
        elif raw[:1].isspace() and blocks:
            blocks[-1].append(line)
        else:
            raise SystemExit(f"corpus.spec line {n}: expected `role NAME` or an indented key")
    return [parse_role(b) for b in blocks]


def parse_role(lines):
    role = {
        "name": lines[0][5:].strip(), "about": "", "instrument": "", "technique": "", "used_by": [],
        "required": False, "selects": [], "combine": None, "count": 0, "excerpt": None, "fill": None,
        "digest": hashlib.sha256("\n".join(lines).encode()).hexdigest(),
    }
    for line in lines[1:]:
        key, _, value = line.partition(" ")
        value = value.strip()
        if key in ("about", "instrument", "technique", "fill"):
            role[key] = value
        elif key == "used-by":
            role["used_by"] = value.split()
        elif key == "required":
            role["required"] = value == "yes"
        elif key == "select":
            parts = [p.strip() for p in value.split(";")]
            kind, _, arg = parts[0].partition(" ")
            filters = dict(p.split("=", 1) for p in parts[1:] if p)
            role["selects"].append((kind, arg.strip(), {k.strip(): v for k, v in filters.items()}))
        elif key == "combine":
            role["combine"] = int(value.split()[1])
        elif key == "count":
            role["count"] = int(value)
        elif key == "excerpt":
            role["excerpt"] = tuple(float(v) for v in value.split())
        else:
            raise SystemExit(f"corpus.spec: role {role['name']}: unknown key {key}")
    return role


# ---- the library ------------------------------------------------------------

def readable(path):
    name = path.rsplit("/", 1)[-1]
    return not name.startswith("._") and name.lower().endswith(".wav")


def file_name(path):
    return path.rsplit("/", 1)[-1]


def absolute(root, rel):
    return os.path.join(root, *rel.split("/"))


def wav_info(path):
    """(rate, channels, seconds) from a WAV's header, or None."""
    try:
        with open(path, "rb") as f:
            head = f.read(12)
            if head[:4] != b"RIFF" or head[8:12] != b"WAVE":
                return None
            fmt = None
            while True:
                chunk = f.read(8)
                if len(chunk) < 8:
                    return None
                cid, size = chunk[:4], struct.unpack("<I", chunk[4:])[0]
                if cid == b"fmt ":
                    fmt = struct.unpack("<HHIIH", f.read(14))
                    f.seek(size - 14 + (size & 1), 1)
                elif cid == b"data" and fmt:
                    _, channels, rate, _, align = fmt
                    return rate, channels, size / max(align, 1) / max(rate, 1)
                else:
                    f.seek(size + (size & 1), 1)
    except OSError:
        return None


def admit(filters, name, seconds, rate, channels):
    f = filters
    return (("name" not in f or glob(f["name"], name))
            and ("min-seconds" not in f or seconds >= float(f["min-seconds"]))
            and ("max-seconds" not in f or seconds <= float(f["max-seconds"]))
            and ("min-rate" not in f or rate >= float(f["min-rate"]))
            and ("channels" not in f or channels == int(f["channels"])))


class Library:
    def __init__(self, root):
        self.root = root
        self.manifest = self._manifest()
        self.sets = self._sets()

    def _manifest(self):
        import csv
        path = os.path.join(self.root, "_manifest.csv")
        if not os.path.isfile(path):
            return None
        with open(path, newline="", encoding="utf-8") as f:
            rows = [r for r in csv.DictReader(f) if r.get("format", "").lower() == "wav" and readable(r["path"])]
        return sorted(rows, key=lambda r: r["path"].encode())

    def _sets(self):
        path = os.path.join(self.root, "_corpus.json")
        if not os.path.isfile(path):
            return None
        with open(path, encoding="utf-8") as f:
            return {k: [e["file"] for e in v if "file" in e] for k, v in json.load(f).items()}

    def walk(self, d):
        out = []
        for dirpath, _, names in os.walk(absolute(self.root, d)):
            for n in names:
                rel = os.path.relpath(os.path.join(dirpath, n), self.root).replace(os.sep, "/")
                if readable(rel):
                    out.append(rel)
        return sorted(out, key=str.encode)

    def admit_file(self, rel, filters):
        if not readable(rel):
            return False
        path = absolute(self.root, rel)
        if not filters:
            return os.path.isfile(path)
        info = wav_info(path)
        return info is not None and admit(filters, file_name(rel), info[2], info[0], info[1])

    def resolve(self, role):
        files = []
        for kind, arg, filters in role["selects"]:
            if kind == "file":
                found = [arg]
            elif kind == "set":
                found = self.sets[arg] if self.sets is not None and arg in self.sets else self.walk(f"Test Corpus/{arg}")
            elif kind == "dir":
                found = self.walk(arg)
            elif kind == "manifest":
                rx = glob_re(arg)
                found = [r["path"] for r in (self.manifest or [])
                         if rx.match(r["category"]) and ("kind" not in filters or r["kind"] == filters["kind"])
                         and admit(filters, file_name(r["path"]), float(r["seconds"] or 0), float(r["rate"] or 0), int(r["channels"] or 0))]
            else:
                raise SystemExit(f"corpus.spec: role {role['name']}: select kind {kind}")
            for f in found:
                ok = (readable(f) and os.path.isfile(absolute(self.root, f))) if kind == "manifest" else self.admit_file(f, filters)
                if ok and f not in files:
                    files.append(f)
        if role["combine"]:
            groups = {}
            for f in files:
                groups.setdefault("/".join(f.split("/")[:role["combine"]]), []).append(f)
            items = list(groups.values())
        else:
            items = [[f] for f in files]
        return items[:role["count"]]


# ---- the lock ---------------------------------------------------------------

def lock_path():
    if os.environ.get("STAVE_CORPUS_LOCK"):
        return os.environ["STAVE_CORPUS_LOCK"]
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return os.path.join(base, "stave", "corpus.lock")


def read_lock(path):
    roles, items = {}, {}
    if not os.path.isfile(path):
        return None
    with open(path, encoding="utf-8") as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if p[0] == "role" and len(p) == 3:
                roles[p[1]] = p[2]
            elif p[0] == "item" and len(p) == 7:
                items.setdefault(p[1], {}).setdefault(int(p[2]), []).append((p[3], int(p[4]), int(p[5]), p[6]))
    return roles, items


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


# ---- commands ---------------------------------------------------------------

def seconds_of(lib, item):
    info = [wav_info(absolute(lib.root, f)) for f in item]
    return max((i[2] for i in info if i), default=0.0), sorted({i[0] for i in info if i})


def cmd_list(lib, roles, names):
    for role in roles:
        if names and role["name"] not in names:
            continue
        items = lib.resolve(role)
        need = "required" if role["required"] else "optional"
        print(f"{role['name']}  ({need}; {len(items)}/{role['count']}; {role['instrument']}: {role['technique']}; used by {' '.join(role['used_by'])})")
        if not items and role["fill"]:
            print(f"    none — fill: {role['fill']}")
        for item in items:
            secs, rates = seconds_of(lib, item)
            what = item[0] if len(item) == 1 else f"{'/'.join(item[0].split('/')[:role['combine']])}  ({len(item)} files summed)"
            print(f"    {what}  [{secs:.1f} s, {'/'.join(str(r) for r in rates)} Hz]")


def cmd_check(lib, roles):
    problems, notes = [], []
    if lib.manifest is None:
        notes.append("no _manifest.csv: manifest selections find nothing")
    if lib.sets is None:
        notes.append("no _corpus.json: sets read from Test Corpus/<set>/")
    else:
        missing = [f for files in lib.sets.values() for f in files if not os.path.isfile(absolute(lib.root, f))]
        if missing:
            problems.append(f"_corpus.json names {len(missing)} missing files, e.g. {missing[0]}")
    path = lock_path()
    lock = read_lock(path)
    if lock is None:
        notes.append("unlocked: scripts/corpus.sh lock records the picks' checksums so runs are reproducible")
    for role in roles:
        items = lib.resolve(role)
        name = role["name"]
        if not items:
            line = f"{name}: no material" + (f" — fill: {role['fill']}" if role["fill"] else "")
            (problems if role["required"] else notes).append(("MISSING " if role["required"] else "GAP ") + line)
            continue
        status = f"{name}: {len(items)}/{role['count']}"
        if lock is not None:
            roles_l, items_l = lock
            if roles_l.get(name) != role["digest"]:
                problems.append(f"{name}: the lock was taken under other rules (corpus.spec changed): run scripts/corpus.sh lock")
            else:
                locked = items_l.get(name, {})
                locked_paths = [[e[3] for e in locked[i]] for i in sorted(locked)]
                if locked_paths != items:
                    problems.append(f"{name}: the library now resolves to other files than the lock names (runs play the locked ones)")
                for i in sorted(locked):
                    for sha, size, mtime, rel in locked[i]:
                        p = absolute(lib.root, rel)
                        if not os.path.isfile(p):
                            problems.append(f"{name}: locked file missing: {rel}")
                        elif sha256(p) != sha:
                            problems.append(f"{name}: checksum drift: {rel}")
                status += ", locked, checksums verified"
        print("ok      " + status)
    for n in notes:
        print("note    " + n)
    for p in problems:
        print("PROBLEM " + p)
    print(f"{len(roles)} roles, {len(problems)} problems")
    return 1 if problems else 0


def cmd_lock(lib, roles):
    path = lock_path()
    lines = ["# stave corpus lock v1 (scripts/corpus.sh lock): local to this machine, never committed",
             "# role NAME DIGEST / item NAME INDEX SHA256 BYTES MTIME PATH (tab-separated)"]
    total = 0
    for role in roles:
        lines.append(f"role\t{role['name']}\t{role['digest']}")
        for i, item in enumerate(lib.resolve(role)):
            for rel in item:
                p = absolute(lib.root, rel)
                st = os.stat(p)
                lines.append(f"item\t{role['name']}\t{i}\t{sha256(p)}\t{st.st_size}\t{int(st.st_mtime)}\t{rel}")
                total += 1
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path) or ".", prefix=".corpus.lock.")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    os.replace(tmp, path)
    print(f"locked {total} files over {len(roles)} roles in {path}")
    return 0


def main(argv):
    if not argv or argv[0] in ("-h", "--help") or argv[0] not in ("list", "check", "lock"):
        print(__doc__.strip())
        return 0 if argv and argv[0] in ("-h", "--help") else 2
    with open(SPEC, encoding="utf-8") as f:
        roles = parse_spec(f.read())
    root = os.environ.get("STAVE_SAMPLE_LIBRARY", "")
    if not root:
        print("skipped: library absent (STAVE_SAMPLE_LIBRARY is not set; see stave.env.example)")
        return 0
    if not os.path.isdir(root):
        print("skipped: library absent (STAVE_SAMPLE_LIBRARY is not mounted)")
        return 0
    lib = Library(root)
    if argv[0] == "list":
        cmd_list(lib, roles, set(argv[1:]))
        return 0
    if argv[0] == "check":
        return cmd_check(lib, roles)
    return cmd_lock(lib, roles)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

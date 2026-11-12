#!/usr/bin/env python3
"""Test only what a change can affect: the workspace packages owning the
changed files, plus every workspace package that depends on them.

    scripts/test-affected.py              # vs HEAD (uncommitted changes)
    scripts/test-affected.py main         # vs a ref (the branch's changes)
    scripts/test-affected.py --list main  # print the packages, run nothing

A change outside every package (Cargo.toml, Cargo.lock, .cargo/,
.config/) can affect anything, so it selects the whole workspace.
Extra arguments after `--` go to `cargo nextest run`.
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GLOBAL = ("Cargo.toml", "Cargo.lock", ".cargo/", ".config/", "rust-toolchain")


def run(*cmd):
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def changed_files(base):
    tracked = run("git", "diff", "--name-only", base).split()
    untracked = run("git", "ls-files", "--others", "--exclude-standard").split()
    return tracked + untracked


def workspace():
    meta = json.loads(run("cargo", "metadata", "--format-version", "1", "--no-deps"))
    packages = {p["name"]: Path(p["manifest_path"]).parent.relative_to(ROOT) for p in meta["packages"]}
    # (dependent, through_dev): a dev-dependency reaches only the
    # dependent's own tests, never the dependent's dependents.
    dependents = {name: set() for name in packages}
    for p in meta["packages"]:
        for dep in p["dependencies"]:
            if dep["name"] in packages:
                dependents[dep["name"]].add((p["name"], dep["kind"] == "dev"))
    return packages, dependents


def affected(files, packages, dependents):
    if any(f == g or f.startswith(g) for f in files for g in GLOBAL):
        return set(packages)
    owners = set()
    for f in files:
        path = Path(f)
        matches = [n for n, d in packages.items() if d != Path(".") and d in path.parents]
        if matches:
            owners.add(max(matches, key=lambda n: len(packages[n].parts)))
    reached, frontier = set(owners), list(owners)
    tested = set(owners)
    while frontier:
        for dependent, through_dev in dependents[frontier.pop()]:
            tested.add(dependent)
            if not through_dev and dependent not in reached:
                reached.add(dependent)
                frontier.append(dependent)
    return tested


def main(argv):
    passthrough = argv[argv.index("--") + 1 :] if "--" in argv else []
    argv = argv[: argv.index("--")] if "--" in argv else argv
    list_only = "--list" in argv
    refs = [a for a in argv if a != "--list"]
    base = refs[0] if refs else "HEAD"
    packages, dependents = workspace()
    selected = sorted(affected(changed_files(base), packages, dependents))
    if list_only:
        print("\n".join(selected))
        return 0
    if not selected:
        print(f"nothing changed against {base}")
        return 0
    print(f"testing {len(selected)} package(s): {' '.join(selected)}", flush=True)
    flags = [arg for name in selected for arg in ("-p", name)]
    return subprocess.run(["cargo", "nextest", "run", *flags, *passthrough], cwd=ROOT).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

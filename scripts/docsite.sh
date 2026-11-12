#!/usr/bin/env bash
# Build the documentation site into target/site (or $1): rustdoc for the
# whole workspace with every warning an error and the site's look
# (tools/docsite/assets/rustdoc-*.html), then tools/docsite, which lays
# out the guide, computes the reference pages and checks every link.
# Nothing it writes is committed; CI builds it fresh and deploys it
# (.github/workflows/docs.yml).
#
#   scripts/docsite.sh               # target/site
#   scripts/docsite.sh /tmp/site     # elsewhere
#   python3 -m http.server -d target/site   # to read it locally
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${1:-target/site}"
ASSETS="$ROOT/tools/docsite/assets"

# Its own target dir: other RUSTDOCFLAGS would rebuild every crate's docs.
export CARGO_TARGET_DIR="${DOCSITE_TARGET_DIR:-$ROOT/target/docsite}"
export RUSTDOCFLAGS="-D warnings --html-in-header $ASSETS/rustdoc-header.html --html-before-content $ASSETS/rustdoc-before.html"
# Rustdoc writes a link into another crate only if that crate's doc
# directory already exists when it starts; crates documented side by
# side would otherwise leave such links as plain text, silently. Make
# every workspace crate's directory first, and document the guide last.
# (The docsite still rejects any link rustdoc left unresolved.)
for lib in $(cargo metadata --format-version 1 --no-deps --locked |
  python3 -c 'import json,sys; print("\n".join(t["name"].replace("-","_") for p in json.load(sys.stdin)["packages"] for t in p["targets"] if set(t["kind"]) & {"lib","rlib","cdylib","proc-macro"}))'); do
  mkdir -p "$CARGO_TARGET_DIR/doc/$lib"
done
cargo doc --workspace --no-deps --locked --exclude stave-guide
cargo doc -p stave-guide --no-deps --locked
unset RUSTDOCFLAGS
cargo run --locked -p stave-docsite -- --doc "$CARGO_TARGET_DIR/doc" --out "$OUT"

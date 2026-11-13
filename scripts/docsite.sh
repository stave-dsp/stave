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
# scripts/rustdoc.sh: no cross-crate link dropped, every warning an
# error. (The docsite then rejects any link on any page that goes
# nowhere.)
scripts/rustdoc.sh
# The platform crates are empty off their own platform (the Windows one
# is `#![cfg(windows)]`): document each for its own target — rustdoc
# needs only that target's std, never its linker — and put its pages in
# place of the empty ones.
for pair in stave-ui-win32:x86_64-pc-windows-msvc stave-ui-x11:x86_64-unknown-linux-gnu; do
  pkg=${pair%%:*} triple=${pair#*:} lib=${pkg//-/_}
  rustup target add "$triple" >/dev/null
  scripts/rustdoc.sh --target "$triple" -p "$pkg"
  for d in "$lib" "src/$lib"; do
    rm -rf "${CARGO_TARGET_DIR:?}/doc/$d"
    cp -R "$CARGO_TARGET_DIR/$triple/doc/$d" "$CARGO_TARGET_DIR/doc/$d"
  done
done
unset RUSTDOCFLAGS
# The Cost page shows crates/stave-dsp/datasheet.json: the datasheet
# test (a debug build names every stage without timing) fails when the
# stages it benches and the committed data differ.
cargo test --locked -p stave-dsp --test datasheet --quiet
cargo run --locked -p stave-docsite -- --doc "$CARGO_TARGET_DIR/doc" --out "$OUT"

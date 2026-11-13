#!/usr/bin/env bash
# Rustdoc for the whole workspace, every warning an error, and no
# cross-crate link silently dropped. The one rustdoc gate: ci.yml's lint
# job runs it (private items included) and scripts/docsite.sh builds the
# published API reference with it.
#
#   scripts/rustdoc.sh                            # target/doc
#   scripts/rustdoc.sh --document-private-items   # extra `cargo doc` args
#   scripts/rustdoc.sh --target x86_64-pc-windows-msvc -p stave-ui-win32
#                                             # one crate, for its platform
#
# Rustdoc writes a link into another crate only if that crate's doc
# directory already exists when it starts; crates documented side by
# side would otherwise leave such a link as plain text, with no warning
# (`href="stave_test::PluginHarness"`). So: make every workspace crate's
# directory first, document the guide (which links into everything)
# last, then fail on any link rustdoc still left unresolved.
#
# Making the directories is not enough on its own. Cargo deletes a
# crate's doc directory when it starts documenting that crate (so no
# page of a removed item outlives it), and rustdoc creates it again only
# once it writes: a crate started in between drops its links into that
# one. The dropped links then stay in pages cargo considers up to date,
# and every later run fails on them (`stave_dsp::Convolver` on a
# stave-host page, `super::interpolators::Thiran` on a `stave` page).
# So a crate whose pages hold unresolved links is documented again, one
# crate at a time, while every other crate's directory stands; a link
# still unresolved after that is a real one. The directories of crates
# no longer in the workspace are removed.
#
# Honours CARGO_TARGET_DIR and RUSTDOCFLAGS (`-D warnings` is added).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

case " ${RUSTDOCFLAGS:-} " in
  *" -D warnings "*) ;;
  *) export RUSTDOCFLAGS="-D warnings ${RUSTDOCFLAGS:-}" ;;
esac

# Where the docs land: <target dir>[/<triple>]/doc.
triple=""
packages=()
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  case "${args[i]}" in
    --target) triple="${args[i + 1]}" ;;
    --target=*) triple="${args[i]#--target=}" ;;
    -p | --package) packages+=("${args[i + 1]}") ;;
  esac
done
DOC="${CARGO_TARGET_DIR:-$ROOT/target}${triple:+/$triple}/doc"

# Every target's doc directory name, "lib NAME" or "bin NAME" (a
# binary's docs are a directory of their own too).
targets=$(cargo metadata --format-version 1 --no-deps --locked |
  python3 -c 'import json,sys; print("\n".join(("lib " if set(t["kind"]) & {"lib","rlib","dylib","cdylib","staticlib","proc-macro"} else "bin ") + t["name"].replace("-","_") for p in json.load(sys.stdin)["packages"] for t in p["targets"]))')
for lib in $(awk '$1 == "lib" { print $2 }' <<<"$targets"); do
  mkdir -p "$DOC/$lib"
done
# A crate's pages (rustdoc writes `all.html` in each) or sources, left by
# a crate no longer in the workspace.
for d in "$DOC"/*/all.html "$DOC"/src/*/; do
  d="${d%/}"
  d="${d%/all.html}"
  if [[ -d "$d" ]] && ! grep -qx "[a-z]* ${d##*/}" <<<"$targets"; then
    rm -rf "$d"
  fi
done

document() { # extra cargo args first
  if ((${#packages[@]})); then
    # Named packages: exactly those.
    cargo doc --no-deps --locked "$@"
  else
    cargo doc --no-deps --locked --workspace --exclude stave-guide "$@"
    cargo doc --no-deps --locked -p stave-guide "$@"
  fi
}
# Every intra-doc link left as text (an `href` naming a path), as
# `page:href="…"`.
unresolved() {
  grep -roE --include='*.html' 'href="[^"]*"' "$DOC" | grep '::' | grep -v '://' || true
}

document "$@"

# The crates whose pages hold unresolved links, documented again one at
# a time (a crate without its `index.html` is out of date to cargo).
redo=()
for c in $(unresolved | sed -E "s|^$DOC/||; s|^src/||; s|[/:].*||" | sort -u); do
  if [[ -d "$DOC/$c" ]] && grep -qx "[a-z]* $c" <<<"$targets"; then
    redo+=("$c")
    rm -f "$DOC/$c/index.html"
  fi
done
if ((${#redo[@]})); then
  echo "rustdoc: documenting ${redo[*]} again, one crate at a time: their pages hold links left unresolved (documented while a crate they link to was being regenerated?)" >&2
  document -j 1 "$@"
fi

# The backstop: no intra-doc link left as text.
dropped=$(unresolved | sed "s|^$DOC/||; s|:|: |" | sort -u)
if [[ -n "$dropped" ]]; then
  echo "rustdoc left these links unresolved:" >&2
  echo "$dropped" | head -20 >&2
  exit 1
fi
echo "rustdoc: every link resolved ($DOC)"

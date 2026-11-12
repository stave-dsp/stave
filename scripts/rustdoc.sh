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

for lib in $(cargo metadata --format-version 1 --no-deps --locked |
  python3 -c 'import json,sys; print("\n".join(t["name"].replace("-","_") for p in json.load(sys.stdin)["packages"] for t in p["targets"] if set(t["kind"]) & {"lib","rlib","cdylib","proc-macro"}))'); do
  mkdir -p "$DOC/$lib"
done

if ((${#packages[@]})); then
  # Named packages: exactly those.
  cargo doc --no-deps --locked "$@"
else
  cargo doc --workspace --no-deps --locked --exclude stave-guide "$@"
  cargo doc -p stave-guide --no-deps --locked "$@"
fi

# The backstop: no intra-doc link left as text (an `href` naming a path).
dropped=$(grep -rhoE 'href="[^"]*"' --include='*.html' "$DOC" | grep '::' | grep -v '://' | sort -u || true)
if [[ -n "$dropped" ]]; then
  echo "rustdoc left these links unresolved (a crate documented before its target's docs existed?):" >&2
  echo "$dropped" | head -20 >&2
  exit 1
fi
echo "rustdoc: every link resolved ($DOC)"

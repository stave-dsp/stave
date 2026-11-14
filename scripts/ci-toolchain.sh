#!/usr/bin/env bash
# Hosted runners install tools; fleet runners verify their immutable image.
set -euo pipefail
pin=${1:-$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)}
[[ -n "$pin" ]] || { echo 'Missing Rust toolchain pin' >&2; exit 1; }
if [[ "${RUNNER_ENVIRONMENT:-}" == github-hosted ]]; then
  rustup toolchain install "$pin" --profile minimal
else
  rustup run "$pin" rustc --version
fi

#!/usr/bin/env bash
# Deliberately do not install tools: test the runner's actual service PATH.
set -euo pipefail
printf 'Runner: %s; capability: %s\n' "${RUNNER_NAME:-local}" "${CANARY_LABEL:?}"
pin=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
[[ -n "$pin" ]] || { echo 'Missing pinned Rust channel' >&2; exit 1; }
rustup --version
cargo +"$pin" --version
cargo nextest --version
if [[ "$CANARY_LABEL" == ngspice ]]; then ngspice --version; fi

#!/usr/bin/env bash
# The pinned tools ci/land.yaml needs beyond ctrl's session image (Ubuntu
# noble, build-essential, clang, pkg-config, python3, git, Rust 1.96.0 as
# real binaries, cargo-nextest; no rustup, no root):
#
#   ci/tools.sh install TOOL...   # into $CI_TOOLS, each version once
#   ci/tools.sh run [--rust VERSION] COMMAND [ARG...]
#
# `run` executes COMMAND with every installed tool on PATH, the ALSA
# headers and library found through pkg-config, and VALIDATOR naming
# Steinberg's validator; `--rust 1.86.0` puts that toolchain's cargo and
# rustc first (the MSRV check, as `cargo +1.86.0` does under rustup).
#
# CI_TOOLS is the tool directory, set by the pipeline: a directory in the
# instance's cache, so a warm instance installs nothing. Each version
# installs into its own directory, atomically, and is never modified
# afterwards. Every download is checked against the SHA-256 written here:
# upstream's published checksum (GitHub release digests, Kitware's
# SHA-256.txt, rustup's .sha256, the Ubuntu archive's Packages index at the
# image's snapshot). Rust 1.86.0 comes from a checksum-pinned rustup-init,
# which verifies each component against the channel manifest's SHA-256.
# The two validators are built from source at pinned commits, as ci.yml
# and platforms.yml build them.
set -euo pipefail

TOOLS="${CI_TOOLS:?CI_TOOLS names the tool directory}"
[[ "$(uname -sm)" == "Linux x86_64" ]] ||
  { echo "ci/tools.sh: pinned for x86_64 Linux, not $(uname -sm)" >&2; exit 2; }
TRIPLE=x86_64-unknown-linux-gnu MULTIARCH=x86_64-linux-gnu

UBUNTU=https://snapshot.ubuntu.com/ubuntu/20261117T000000Z/pool/main/a/alsa-lib
ALSA_VERSION=1.2.11-1ubuntu0.3
ALSA_DEBS=(
  "libasound2t64_${ALSA_VERSION}_amd64.deb 5d17bfb5683eb99f1be951fe48e1be5bf3c52391913b6b74a8aecf9b4f5e779f"
  "libasound2-dev_${ALSA_VERSION}_amd64.deb b83795b1ba0e40cfa8363359fd17a06260838d8ae0f315531771a62f7c7e215a"
  "libasound2-data_${ALSA_VERSION}_all.deb 0c26896193480d9dbb055e85e8ddfb9c9f34cf19afee340987cd6311e2886fc1"
)
DENY_VERSION=0.20.2
DENY_SHA=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
MACHETE_VERSION=0.9.2
MACHETE_SHA=48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9
RUSTUP_VERSION=1.29.3
RUSTUP_SHA=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
MSRV=1.86.0
CMAKE_VERSION=3.31.12
CMAKE_SHA=0dc2e9a6860f06bf10bd8fadc03e35d9eeb4df46e33763a7e480e987758f385c
CLAP_VALIDATOR_TAG=0.4.1
CLAP_VALIDATOR_REV=152b9823e992d782c5c1fd33bca0295478b919aa
VST3SDK_TAG=v3.8.1_build_84
VST3SDK_REV=3cdf9ca5d1f5b1b21e0a86832aa4abe55607bd96

ALSA="$TOOLS/alsa-$ALSA_VERSION"
DENY="$TOOLS/cargo-deny-$DENY_VERSION"
MACHETE="$TOOLS/cargo-machete-$MACHETE_VERSION"
RUST="$TOOLS/rust-$MSRV-rustup-$RUSTUP_VERSION"
CMAKE="$TOOLS/cmake-$CMAKE_VERSION"
CLAP_VALIDATOR="$TOOLS/clap-validator-$CLAP_VALIDATOR_TAG"
VST3SDK="$TOOLS/vst3sdk-$VST3SDK_TAG-cmake-$CMAKE_VERSION"

fetch() { # url sha256 out
  curl -fsSL --retry 3 -o "$3" "$1"
  echo "$2  $3" | sha256sum -c --quiet -
}
# install DIR FUNCTION: run FUNCTION in a temp dir beside DIR, then move it into place.
install() {
  [[ -f "$1/.installed" ]] && return 0
  mkdir -p "$TOOLS"
  local tmp
  tmp=$(mktemp -d "$1.tmp.XXXXXX")
  (cd "$tmp" && "$2")
  touch "$tmp/.installed"
  rm -rf "$1"
  mv "$tmp" "$1"
}

alsa() {
  local deb name sha
  for deb in "${ALSA_DEBS[@]}"; do
    read -r name sha <<<"$deb"
    fetch "$UBUNTU/$name" "$sha" "$name"
    dpkg-deb -x "$name" . && rm "$name"
  done
  # The package's prefix is /usr: point its pkg-config file where it is installed.
  sed -i "s#^prefix=/usr\$#prefix=$ALSA/usr#" "usr/lib/$MULTIARCH/pkgconfig/alsa.pc"
  grep -qx "prefix=$ALSA/usr" "usr/lib/$MULTIARCH/pkgconfig/alsa.pc"
}
cargo_deny() {
  local name="cargo-deny-$DENY_VERSION-x86_64-unknown-linux-musl"
  fetch "https://github.com/EmbarkStudios/cargo-deny/releases/download/$DENY_VERSION/$name.tar.gz" "$DENY_SHA" deny.tgz
  mkdir bin && tar xzf deny.tgz --strip-components=1 -C bin "$name/cargo-deny" && rm deny.tgz
}
cargo_machete() {
  local name="cargo-machete-v$MACHETE_VERSION-x86_64-unknown-linux-musl"
  fetch "https://github.com/bnjbvr/cargo-machete/releases/download/v$MACHETE_VERSION/$name.tar.gz" "$MACHETE_SHA" machete.tgz
  mkdir bin && tar xzf machete.tgz --strip-components=1 -C bin "$name/cargo-machete" && rm machete.tgz
}
rust_msrv() {
  fetch "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/$TRIPLE/rustup-init" "$RUSTUP_SHA" rustup-init
  chmod +x rustup-init
  RUSTUP_HOME="$PWD/rustup" CARGO_HOME="$PWD/cargo" ./rustup-init -y --no-modify-path \
    --profile minimal --default-toolchain "$MSRV"
  rm -rf rustup-init rustup/downloads rustup/tmp
  # Run as real binaries, as the image's own toolchain is: no rustup proxy at run time.
  ln -s "rustup/toolchains/$MSRV-$TRIPLE/bin" bin
  bin/cargo --version
}
cmake_() {
  fetch "https://github.com/Kitware/CMake/releases/download/v$CMAKE_VERSION/cmake-$CMAKE_VERSION-linux-x86_64.tar.gz" "$CMAKE_SHA" cmake.tgz
  tar xzf cmake.tgz --strip-components=1 && rm cmake.tgz
}
clap_validator() {
  cargo install --locked --root "$PWD" --git https://github.com/free-audio/clap-validator.git \
    --rev "$CLAP_VALIDATOR_REV" clap-validator
}
vst3sdk() {
  [[ -x "$CMAKE/bin/cmake" ]] || { echo "ci/tools.sh: install cmake before vst3-validator" >&2; return 2; }
  git clone -q --depth 1 --branch "$VST3SDK_TAG" --recurse-submodules --shallow-submodules \
    https://github.com/steinbergmedia/vst3sdk.git src
  [[ "$(git -C src rev-parse HEAD)" == "$VST3SDK_REV" ]] ||
    { echo "ci/tools.sh: $VST3SDK_TAG is not $VST3SDK_REV" >&2; return 1; }
  "$CMAKE/bin/cmake" -S src -B build -DCMAKE_BUILD_TYPE=Release \
    -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF \
    -DSMTG_ENABLE_VST3_PLUGIN_EXAMPLES=OFF \
    -DSMTG_ENABLE_VST3_HOSTING_EXAMPLES=OFF
  "$CMAKE/bin/cmake" --build build --config Release --target validator -j "$(nproc)"
  mkdir bin && cp build/bin/Release/validator bin/validator
  rm -rf src build
  [[ -x bin/validator ]]
}

case "${1:-}" in
  install)
    shift
    for tool in "$@"; do
      case "$tool" in
        alsa) install "$ALSA" alsa ;;
        cargo-deny) install "$DENY" cargo_deny ;;
        cargo-machete) install "$MACHETE" cargo_machete ;;
        rust-msrv) install "$RUST" rust_msrv ;;
        cmake) install "$CMAKE" cmake_ ;;
        clap-validator) install "$CLAP_VALIDATOR" clap_validator ;;
        vst3-validator) install "$VST3SDK" vst3sdk ;;
        *) echo "ci/tools.sh: unknown tool $tool" >&2; exit 2 ;;
      esac
    done
    ;;
  run)
    shift
    if [[ "${1:-}" == --rust ]]; then
      [[ "${2:-}" == "$MSRV" ]] || { echo "ci/tools.sh: only Rust $MSRV is installed here" >&2; exit 2; }
      export PATH="$RUST/bin:$PATH"
      shift 2
    fi
    export PATH="$DENY/bin:$MACHETE/bin:$CMAKE/bin:$CLAP_VALIDATOR/bin:$PATH"
    export PKG_CONFIG_PATH="$ALSA/usr/lib/$MULTIARCH/pkgconfig"
    export LD_LIBRARY_PATH="$ALSA/usr/lib/$MULTIARCH"
    export ALSA_CONFIG_DIR="$ALSA/usr/share/alsa"
    export VALIDATOR="$VST3SDK/bin/validator"
    exec "$@"
    ;;
  *) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2; exit 2 ;;
esac

# Shared by the bundle/install scripts — source, don't run.
#
#   STAVE_BUNDLE_VENDOR   reverse-DNS prefix for CFBundleIdentifier
#                         (default com.ryanbahan.stave; set your own)
#   STAVE_BUNDLE_VERSION  CFBundleVersion (default 0.0.1)
#   CARGO_TARGET_DIR      honoured when locating the built library
#
# One path per OS (uname): macOS bundles (Contents/MacOS + Info.plist,
# ad-hoc signed), Linux (VST3: Contents/<arch>-linux/<Name>.so; CLAP:
# the .so as <Name>.clap) and Windows under Git Bash (VST3:
# Contents/<arch>-win/<Name>.vst3; CLAP: the DLL as <Name>.clap) — the
# layouts the VST3 SDK and clap.h name.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAVE_BUNDLE_VENDOR="${STAVE_BUNDLE_VENDOR:-com.ryanbahan.stave}"
STAVE_BUNDLE_VERSION="${STAVE_BUNDLE_VERSION:-0.0.1}"

case "$(uname -s)" in
  Darwin) STAVE_OS=macos ;;
  Linux) STAVE_OS=linux ;;
  MINGW* | MSYS* | CYGWIN*) STAVE_OS=windows ;;
  *) echo "unsupported OS: $(uname -s)" >&2; return 1 2>/dev/null || exit 1 ;;
esac

# The VST3 SDK's architecture directory name.
vst3_arch() {
  case "$(uname -m)" in
    arm64 | aarch64) [[ $STAVE_OS == windows ]] && echo arm64 || echo aarch64 ;;
    *) echo x86_64 ;;
  esac
}

case "$STAVE_OS" in
  macos)
    USER_VST3_DIR="${HOME}/Library/Audio/Plug-Ins/VST3"
    USER_CLAP_DIR="${HOME}/Library/Audio/Plug-Ins/CLAP" ;;
  linux)
    USER_VST3_DIR="${HOME}/.vst3"
    USER_CLAP_DIR="${HOME}/.clap" ;;
  windows)
    # The per-user locations hosts scan (VST3 SDK 3.7+, clap.h).
    USER_VST3_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/Programs/Common/VST3"
    USER_CLAP_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/Programs/Common/CLAP" ;;
esac

# cargo's flag for a profile name: release → --release, debug → (none).
profile_args() {
  case "$1" in
    release) echo "--release" ;;
    debug) ;;
    *) echo "--profile $1" ;;
  esac
}

# Crate name → CamelCase bundle name: stave-multiband-comp-vst3 → StaveMultibandComp.
bundle_name() {
  echo "${1%-vst3}" | awk -F- '{for(i=1;i<=NF;i++) printf "%s%s", toupper(substr($i,1,1)), substr($i,2)}'
}

# The built library for <crate> <profile>; fails with the build command if absent.
lib_path() {
  local dir="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/$2" stem="${1//-/_}" path
  case "$STAVE_OS" in
    macos) path="$dir/lib${stem}.dylib" ;;
    linux) path="$dir/lib${stem}.so" ;;
    windows) path="$dir/${stem}.dll" ;;
  esac
  if [[ ! -f "$path" ]]; then
    echo "error: $path not found — run: cargo build --$2 -p $1" >&2
    return 1
  fi
  echo "$path"
}

# make_bundle <crate> <profile> <out-dir> <vst3|clap> — prints the bundle path.
make_bundle() {
  local crate=$1 profile=$2 out=$3 ext=$4 lib name trimmed id path
  lib=$(lib_path "$crate" "$profile") || return 1
  name=$(bundle_name "$crate")
  trimmed="${crate%-vst3}"
  id="${STAVE_BUNDLE_VENDOR}.${trimmed#stave-}.${ext}"
  case "$out" in /* | [A-Za-z]:*) ;; *) out="${REPO_ROOT}/${out}" ;; esac
  path="${out}/${name}.${ext}"

  rm -rf "$path"
  mkdir -p "$out"
  case "$STAVE_OS:$ext" in
    macos:*) make_macos_bundle "$lib" "$path" "$name" "$id" ;;
    linux:vst3)
      mkdir -p "$path/Contents/$(vst3_arch)-linux"
      cp "$lib" "$path/Contents/$(vst3_arch)-linux/${name}.so" ;;
    windows:vst3)
      mkdir -p "$path/Contents/$(vst3_arch)-win"
      cp "$lib" "$path/Contents/$(vst3_arch)-win/${name}.vst3" ;;
    *:clap) cp "$lib" "$path" ;;
  esac
  echo "$path"
}

# make_macos_bundle <dylib> <bundle> <name> <id>
make_macos_bundle() {
  local dylib=$1 path=$2 name=$3 id=$4
  mkdir -p "$path/Contents/MacOS" "$path/Contents/Resources"
  cp "$dylib" "$path/Contents/MacOS/${name}"
  echo "BNDL????" > "$path/Contents/PkgInfo"
  cat > "$path/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleExecutable</key><string>${name}</string>
  <key>CFBundleIdentifier</key><string>${id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${name}</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleShortVersionString</key><string>${STAVE_BUNDLE_VERSION}</string>
  <key>CFBundleSignature</key><string>????</string>
  <key>CFBundleVersion</key><string>${STAVE_BUNDLE_VERSION}</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
  # Ad-hoc sign the bundle: the linker signs the dylib, but an unsigned
  # (or stale-signed) bundle is rejected by strict scanners such as
  # Live's ("code has no resources but signature indicates they must be
  # present").
  codesign --sign - --force --deep --timestamp=none "$path" >/dev/null 2>&1 || true
}

# fresh_install <bundle> <dest-dir>: remove, then copy. Never cp -R over
# an installed bundle — a running host that has it mapped gets its vnode
# poisoned and macOS SIGKILLs every later loader (exit 137).
fresh_install() {
  mkdir -p "$2"
  rm -rf "$2/$(basename "$1")"
  cp -R "$1" "$2/"
}

# Every plugin crate in the workspace.
plugin_crates() {
  local toml
  for toml in "${REPO_ROOT}"/examples/*/Cargo.toml; do
    grep -m1 '^name' "$toml" | cut -d'"' -f2
  done
}

# The Steinberg validator: $VALIDATOR, else the SDK build under target/vst3sdk.
vst3_validator() {
  local v="${VALIDATOR:-$(ls "${REPO_ROOT}"/target/vst3sdk/build/bin/*/validator{,.exe} 2>/dev/null | head -1)}"
  [[ -n "$v" && ! -f "$v" && -f "$v.exe" ]] && v="$v.exe"
  echo "$v"
}

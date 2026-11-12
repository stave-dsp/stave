# Shared by the bundle/install scripts — source, don't run.
#
#   STAVE_BUNDLE_VENDOR   reverse-DNS prefix for CFBundleIdentifier
#                         (default com.ryanbahan.stave; set your own)
#   STAVE_BUNDLE_VERSION  CFBundleVersion (default 0.0.1)
#   CARGO_TARGET_DIR      honoured when locating the built dylib

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAVE_BUNDLE_VENDOR="${STAVE_BUNDLE_VENDOR:-com.ryanbahan.stave}"
STAVE_BUNDLE_VERSION="${STAVE_BUNDLE_VERSION:-0.0.1}"
USER_VST3_DIR="${HOME}/Library/Audio/Plug-Ins/VST3"
USER_CLAP_DIR="${HOME}/Library/Audio/Plug-Ins/CLAP"

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

# The built dylib for <crate> <profile>; fails with the build command if absent.
dylib_path() {
  local path="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/$2/lib${1//-/_}.dylib"
  if [[ ! -f "$path" ]]; then
    echo "error: $path not found — run: cargo build --$2 -p $1" >&2
    return 1
  fi
  echo "$path"
}

# make_bundle <crate> <profile> <out-dir> <vst3|clap> — prints the bundle path.
make_bundle() {
  local crate=$1 profile=$2 out=$3 ext=$4 dylib name trimmed id path
  dylib=$(dylib_path "$crate" "$profile") || return 1
  name=$(bundle_name "$crate")
  trimmed="${crate%-vst3}"
  id="${STAVE_BUNDLE_VENDOR}.${trimmed#stave-}.${ext}"
  case "$out" in /*) ;; *) out="${REPO_ROOT}/${out}" ;; esac
  path="${out}/${name}.${ext}"

  rm -rf "$path"
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
  echo "$path"
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
  for toml in "${REPO_ROOT}"/plugins/*/Cargo.toml; do
    grep -m1 '^name' "$toml" | cut -d'"' -f2
  done
}

# The Steinberg validator: $VALIDATOR, else the SDK build under target/vst3sdk.
vst3_validator() {
  echo "${VALIDATOR:-$(ls "${REPO_ROOT}"/target/vst3sdk/build/bin/*/validator 2>/dev/null | head -1)}"
}

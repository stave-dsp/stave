#!/usr/bin/env bash
# Package a stave plugin for strangers: every format this OS ships,
# signed and notarised where the credentials exist, in the file a person
# downloads (macOS .dmg, Windows .zip [+ installer], Linux .tar.gz).
#
#   scripts/package.sh <crate> [build|sign|all]   (default: all)
#
#   build  release builds (macOS: arm64 + x86_64, joined with lipo), the
#          bundles — CLAP, VST3, AU (when the plugin has one), the
#          standalone app (when the crate has a `standalone` feature, see
#          `stave::standalone!`) — into $OUT/bundles. Runs anywhere with
#          the toolchain: a build host through scripts/offload.sh with
#          STAVE_OFFLOAD_FETCH=target/package.
#   sign   on the machine that holds the identity: codesign (hardened
#          runtime), notarise and staple, the download (.dmg, .pkg with
#          an installer identity), verification. Windows: Authenticode
#          when a certificate is given. Linux: nothing to sign.
#   all    build, then sign.
#
# Configuration (stave.env.example; never committed — the values are
# yours, and scripts/check-local-leaks.sh refuses them in the tree):
#   STAVE_SIGN_IDENTITY       macOS "Developer ID Application: …" identity
#                             (or its SHA-1); unset: ad-hoc signed, and a
#                             stranger's Gatekeeper refuses the download
#   STAVE_NOTARY_PROFILE      notarytool keychain profile (made once with
#                             `xcrun notarytool store-credentials`); unset
#                             or missing: not notarised, the command to
#                             make it printed
#   STAVE_INSTALLER_IDENTITY  "Developer ID Installer: …": also build a .pkg
#   STAVE_WIN_SIGN_CERT       Windows: a .pfx for signtool (password in
#                             STAVE_WIN_SIGN_PASSWORD); unset: unsigned
#   STAVE_BUNDLE_VENDOR       bundle id prefix (lib/bundle-common.sh)
#   STAVE_BUNDLE_VERSION      the version bundles carry (default: the crate's)
#   STAVE_PACKAGE_DIR         output root (default target/package)
#   STAVE_PACKAGE_ARCHS       macOS slices (default "aarch64 x86_64")
#
# Output: $STAVE_PACKAGE_DIR/<crate>/ — bundles/ and the download,
# <Name>-<version>-<os>.<ext>. The guide: How-to → Ship your plugin.
set -euo pipefail
# The caller's version, before bundle-common.sh defaults it.
CALLER_VERSION="${STAVE_BUNDLE_VERSION:-}"
source "$(dirname "$0")/lib/bundle-common.sh"
source "$REPO_ROOT/scripts/lib/config.sh"
stave_config_load "$REPO_ROOT"
cd "$REPO_ROOT"

CRATE="${1:?usage: package.sh <crate> [build|sign|all]}"
STEP="${2:-all}"
case "$STEP" in build | sign | all) ;; *) echo "unknown step: $STEP (build, sign or all)" >&2; exit 2 ;; esac
OUT="${STAVE_PACKAGE_DIR:-$REPO_ROOT/target/package}/$CRATE"
BUNDLES="$OUT/bundles"
NAME=$(bundle_name "$CRATE")
TRIMMED="${CRATE%-vst3}"
ID_BASE="${STAVE_BUNDLE_VENDOR}.${TRIMMED#stave-}"

# The crate's version, and its standalone: the bin `<crate>-standalone`
# (`stave::standalone!`, behind the crate's `standalone` feature).
# No python here: Windows runners have none by that name.
pkgid=$(cargo pkgid -p "$CRATE") || { echo "no workspace crate named $CRATE" >&2; exit 1; }
VERSION="${pkgid##*[@#]}"
STAVE_BUNDLE_VERSION="${CALLER_VERSION:-$VERSION}"
export STAVE_BUNDLE_VERSION
STANDALONE_BIN=""
if cargo metadata --no-deps --format-version 1 | grep -F "\"name\":\"${CRATE}-standalone\"" >/dev/null; then
  STANDALONE_BIN="${CRATE}-standalone"
fi

note() { printf '==> %s\n' "$*"; }
exe() { [[ $STAVE_OS == windows ]] && echo "$1.exe" || echo "$1"; }

# ------------------------------------------------------------------ build

# cargo build of the plugin library and (separately: the feature would
# otherwise link the audio device layer into the plugin) the standalone,
# for one target; the results copied to $1 before the next build
# overwrites them.
build_target() {
  local stage=$1 target=${2:-} dir="${CARGO_TARGET_DIR:-$REPO_ROOT/target}" args=()
  [[ -n "$target" ]] && { args=(--target "$target"); dir="$dir/$target"; }
  mkdir -p "$stage"
  if [[ -n "$STANDALONE_BIN" ]]; then
    cargo build --release --locked "${args[@]}" -p "$CRATE" --features standalone --bin "$STANDALONE_BIN"
    cp "$dir/release/$(exe "$STANDALONE_BIN")" "$stage/"
  fi
  cargo build --release --locked "${args[@]}" -p "$CRATE" --lib
  local stem="${CRATE//-/_}"
  case "$STAVE_OS" in
    macos) cp "$dir/release/lib${stem}.dylib" "$stage/" ;;
    linux) cp "$dir/release/lib${stem}.so" "$stage/" ;;
    windows) cp "$dir/release/${stem}.dll" "$stage/" ;;
  esac
}

# The standalone as a macOS app: the executable, an Info.plist that asks
# for the microphone (an input device), ad-hoc signed until `sign`.
make_macos_app() {
  local exe=$1 app=$2
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$exe" "$app/Contents/MacOS/$NAME"
  echo "APPL????" > "$app/Contents/PkgInfo"
  cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleExecutable</key><string>${NAME}</string>
  <key>CFBundleIdentifier</key><string>${ID_BASE}.app</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${STAVE_BUNDLE_VERSION}</string>
  <key>CFBundleVersion</key><string>${STAVE_BUNDLE_VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSMicrophoneUsageDescription</key><string>${NAME} processes the audio input you choose in its audio settings.</string>
</dict>
</plist>
PLIST
  codesign --sign - --force --timestamp=none "$app" >/dev/null 2>&1 || true
}

build() {
  rm -rf "$BUNDLES"
  mkdir -p "$BUNDLES"
  local stage="$OUT/stage" stem="${CRATE//-/_}"
  rm -rf "$stage"
  case "$STAVE_OS" in
    macos)
      local arch slices=() exes=()
      for arch in ${STAVE_PACKAGE_ARCHS:-aarch64 x86_64}; do
        rustup target list --installed | grep -qx "$arch-apple-darwin" || rustup target add "$arch-apple-darwin"
        note "build $arch-apple-darwin"
        build_target "$stage/$arch" "$arch-apple-darwin"
        slices+=("$stage/$arch/lib${stem}.dylib")
        [[ -n "$STANDALONE_BIN" ]] && exes+=("$stage/$arch/$STANDALONE_BIN")
      done
      lipo -create "${slices[@]}" -output "$stage/lib${stem}.dylib"
      local ext
      for ext in clap vst3; do
        make_macos_bundle "$stage/lib${stem}.dylib" "$BUNDLES/$NAME.$ext" "$NAME" "$ID_BASE.$ext"
      done
      if [[ -n "$STANDALONE_BIN" ]]; then
        lipo -create "${exes[@]}" -output "$stage/$STANDALONE_BIN"
        make_macos_app "$stage/$STANDALONE_BIN" "$BUNDLES/$NAME.app"
      fi
      # AU v2: the plugin's own bundle script, when the framework has one.
      if [[ -x "$REPO_ROOT/scripts/bundle-au.sh" ]]; then
        CARGO_TARGET_DIR="$stage/au-target" "$REPO_ROOT/scripts/bundle-au.sh" "$CRATE" release "$BUNDLES" \
          || echo "note: AU bundle not built (scripts/bundle-au.sh failed)" >&2
      fi
      lipo -info "$stage/lib${stem}.dylib"
      ;;
    linux | windows)
      note "build ($STAVE_OS, $(uname -m))"
      build_target "$stage"
      local lib="$stage/lib${stem}.so"
      [[ $STAVE_OS == windows ]] && lib="$stage/${stem}.dll"
      local arch_dir
      arch_dir="$(vst3_arch)-$([[ $STAVE_OS == linux ]] && echo linux || echo win)"
      mkdir -p "$BUNDLES/$NAME.vst3/Contents/$arch_dir"
      if [[ $STAVE_OS == linux ]]; then
        cp "$lib" "$BUNDLES/$NAME.vst3/Contents/$arch_dir/$NAME.so"
      else
        cp "$lib" "$BUNDLES/$NAME.vst3/Contents/$arch_dir/$NAME.vst3"
      fi
      cp "$lib" "$BUNDLES/$NAME.clap"
      [[ -n "$STANDALONE_BIN" ]] && cp "$stage/$(exe "$STANDALONE_BIN")" "$BUNDLES/$(exe "$NAME")"
      ;;
  esac
  rm -rf "$stage"
  note "bundles in $BUNDLES:"
  ls -1 "$BUNDLES"
}

# ------------------------------------------------------------- download

# The install notes every download carries.
install_notes() {
  case "$STAVE_OS" in
    macos) cat <<EOF
${NAME} ${STAVE_BUNDLE_VERSION}

Drag each format onto the folder of the same name in this window
(installs for every user; macOS asks for your password), or into your
own Library for just you:

  ${NAME}.clap        ~/Library/Audio/Plug-Ins/CLAP/
  ${NAME}.vst3        ~/Library/Audio/Plug-Ins/VST3/
  ${NAME}.component   ~/Library/Audio/Plug-Ins/Components/   (Audio Unit, if present)
  ${NAME}.app         /Applications/   (runs on its own: Tab opens the audio settings)

Then rescan plugins in your DAW. Audio input in the app starts off;
choose an input device in its audio settings to hear it.
EOF
    ;;
    windows) cat <<EOF
${NAME} ${STAVE_BUNDLE_VERSION}

Copy each format to the folder your DAW scans:

  ${NAME}.vst3   C:\\Program Files\\Common Files\\VST3\\    (all users)
                 %LOCALAPPDATA%\\Programs\\Common\\VST3\\   (just you)
  ${NAME}.clap   C:\\Program Files\\Common Files\\CLAP\\    (all users)
                 %LOCALAPPDATA%\\Programs\\Common\\CLAP\\   (just you)
  ${NAME}.exe    anywhere: runs on its own (Tab opens the audio settings)

Then rescan plugins in your DAW. If Windows SmartScreen warns about an
unrecognised app, the download is not code-signed: choose More info, Run
anyway, only if you trust where it came from.
EOF
    ;;
    linux) cat <<EOF
${NAME} ${STAVE_BUNDLE_VERSION}

Run ./install.sh to copy everything for your user, or by hand:

  ${NAME}.vst3   ~/.vst3/
  ${NAME}.clap   ~/.clap/
  ${NAME}        ~/.local/bin/   (runs on its own: Tab opens the audio settings)

The plugins need an X11 display (XWayland works). The app plays through
ALSA; PipeWire and JACK systems route it through their ALSA plugins.
Then rescan plugins in your DAW.
EOF
    ;;
  esac
}

# shellcheck disable=SC2016 # expanded by install.sh, not here
LINUX_INSTALL='#!/bin/sh
# Install for the current user.
set -e
cd "$(dirname "$0")"
mkdir -p "$HOME/.vst3" "$HOME/.clap" "$HOME/.local/bin"
for b in *.vst3; do rm -rf "$HOME/.vst3/$b"; cp -R "$b" "$HOME/.vst3/"; done
for b in *.clap; do rm -f "$HOME/.clap/$b"; cp "$b" "$HOME/.clap/"; done
for f in *; do
  if [ -f "$f" ] && [ -x "$f" ] && [ "$f" != install.sh ]; then cp "$f" "$HOME/.local/bin/"; fi
done
echo "installed to ~/.vst3, ~/.clap and ~/.local/bin"
'

sign() {
  [[ -d "$BUNDLES" ]] || { echo "no bundles in $BUNDLES: run the build step first" >&2; exit 1; }
  local base="$OUT/${NAME}-${STAVE_BUNDLE_VERSION}-${STAVE_OS}"
  case "$STAVE_OS" in
    macos)
      source "$REPO_ROOT/scripts/lib/sign-macos.sh"
      macos_sign_and_ship
      ;;
    windows)
      if [[ -n "${STAVE_WIN_SIGN_CERT:-}" ]]; then
        local f
        for f in "$BUNDLES/$(exe "$NAME")" "$BUNDLES/$NAME.clap" "$BUNDLES"/"$NAME".vst3/Contents/*/"$NAME".vst3; do
          [[ -f "$f" ]] || continue
          signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 \
            /f "$STAVE_WIN_SIGN_CERT" /p "${STAVE_WIN_SIGN_PASSWORD:-}" "$f"
        done
      else
        echo "note: not Authenticode-signed (no STAVE_WIN_SIGN_CERT): SmartScreen warns on first run" >&2
      fi
      local dir="$OUT/${NAME}-${STAVE_BUNDLE_VERSION}"
      rm -rf "$dir" "$base.zip"
      cp -R "$BUNDLES" "$dir"
      install_notes > "$dir/INSTALL.txt"
      (cd "$OUT" && powershell -NoProfile -Command "Compress-Archive -Path '$(basename "$dir")' -DestinationPath '$(basename "$base").zip'")
      rm -rf "$dir"
      if command -v iscc >/dev/null; then
        windows_installer "$base"
      else
        echo "note: Inno Setup (iscc) not on PATH: no installer, the zip only" >&2
      fi
      ;;
    linux)
      local dir="$OUT/${NAME}-${STAVE_BUNDLE_VERSION}"
      rm -rf "$dir"
      cp -R "$BUNDLES" "$dir"
      install_notes > "$dir/INSTALL.txt"
      printf '%s' "$LINUX_INSTALL" > "$dir/install.sh"
      chmod +x "$dir/install.sh"
      tar -C "$OUT" -czf "$base.tar.gz" "$(basename "$dir")"
      rm -rf "$dir"
      ;;
  esac
  note "download:"
  local f
  for f in "$OUT"/*; do
    [[ -f "$f" ]] && echo "  $f"
  done
}

# An Inno Setup installer: per-user install (no admin) of the VST3 and
# CLAP into the folders hosts scan, the app into Programs.
windows_installer() {
  local base=$1 iss="$OUT/installer.iss"
  local src
  src=$(cygpath -w "$BUNDLES" 2>/dev/null || echo "$BUNDLES")
  cat > "$iss" <<ISS
[Setup]
AppName=${NAME}
AppVersion=${STAVE_BUNDLE_VERSION}
AppPublisher=${STAVE_BUNDLE_VENDOR}
DefaultDirName={localappdata}\\Programs\\${NAME}
PrivilegesRequired=lowest
OutputDir=$(cygpath -w "$OUT" 2>/dev/null || echo "$OUT")
OutputBaseFilename=$(basename "$base")-setup
DisableProgramGroupPage=yes

[Files]
Source: "${src}\\${NAME}.vst3\\*"; DestDir: "{localappdata}\\Programs\\Common\\VST3\\${NAME}.vst3"; Flags: recursesubdirs ignoreversion
Source: "${src}\\${NAME}.clap"; DestDir: "{localappdata}\\Programs\\Common\\CLAP"; Flags: ignoreversion
ISS
  if [[ -f "$BUNDLES/$NAME.exe" ]]; then
    cat >> "$iss" <<ISS
Source: "${src}\\${NAME}.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\\${NAME}"; Filename: "{app}\\${NAME}.exe"
ISS
  fi
  iscc "$(cygpath -w "$iss" 2>/dev/null || echo "$iss")"
  if [[ -n "${STAVE_WIN_SIGN_CERT:-}" ]]; then
    signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 \
      /f "$STAVE_WIN_SIGN_CERT" /p "${STAVE_WIN_SIGN_PASSWORD:-}" "$base-setup.exe"
  fi
}

case "$STEP" in
  build) build ;;
  sign) sign ;;
  all) build; sign ;;
esac

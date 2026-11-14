# shellcheck shell=bash
# macOS signing, notarisation and the download, for scripts/package.sh —
# source, don't run. Reads NAME, OUT, BUNDLES, ID_BASE,
# STAVE_BUNDLE_VERSION from the caller and the signing configuration
# (STAVE_SIGN_IDENTITY, STAVE_NOTARY_PROFILE, STAVE_INSTALLER_IDENTITY)
# from scripts/lib/config.sh. Never handles a password or key: notarytool
# reads them from the keychain profile its owner made.
#
# What a stranger's Mac checks, and what each step here gives it:
#   codesign, Developer ID + hardened runtime + secure timestamp
#       → the code is from a known developer and unaltered;
#   notarytool + stapler → Apple has scanned it; the ticket travels with
#       the bundle, so Gatekeeper passes it offline too;
#   both on the .dmg as well → the download itself opens without a
#       warning.
# Without an identity, bundles stay ad-hoc signed (fine on the machine
# that built them; quarantined on any other, Gatekeeper refuses them).

# resolve_identity <name or SHA-1> <key>: the identity's SHA-1. A name
# several certificates share (a renewed Developer ID beside the old one)
# resolves to the one that expires last; a 40-hex SHA-1 is taken as is.
resolve_identity() {
  local want="$1" key="$2" hashes
  if [[ "$want" =~ ^[0-9A-Fa-f]{40}$ ]]; then
    echo "$want"
    return
  fi
  hashes=$(security find-identity -v | grep -F "\"$want\"" | awk '{print $2}' | sort -u)
  case "$(printf '%s\n' "$hashes" | grep -c .)" in
    0) echo "no valid identity named by $key in the keychain" >&2; return 1 ;;
    1) echo "$hashes" ;;
    *)
      security find-certificate -a -Z -p -c "$want" | python3 -c '
import datetime, re, subprocess, sys
text, wanted = sys.stdin.read(), set(sys.argv[1].split())
best = None
for sha, pem in re.findall(r"SHA-1 hash: ([0-9A-F]{40}).*?(-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----)", text, re.S):
    if sha not in wanted:
        continue
    end = subprocess.run(["openssl", "x509", "-noout", "-enddate"], input=pem.encode(), capture_output=True).stdout.decode()
    # notAfter=Jan  3 23:44:05 2030 GMT
    when = datetime.datetime.strptime(end.split("=", 1)[1].replace("GMT", "").strip(), "%b %d %H:%M:%S %Y")
    if best is None or when > best[0]:
        best = (when, sha)
print(best[1] if best else "")
' "$hashes"
      ;;
  esac
}

# Entitlements for the standalone app under the hardened runtime: the
# microphone (an input device). Plugin bundles take none: they run in
# the host's process, under the host's entitlements.
app_entitlements() {
  local file="$OUT/.standalone.entitlements"
  cat > "$file" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.device.audio-input</key><true/>
</dict>
</plist>
PLIST
  echo "$file"
}

# The command that makes the notary profile, for its owner to run once.
notary_help() {
  local profile="${STAVE_NOTARY_PROFILE:-<profile>}"
  cat >&2 <<EOF
To notarise, store your notary credentials in the keychain once (this
script never sees them), then set STAVE_NOTARY_PROFILE=$profile in
~/.config/stave/env:

  xcrun notarytool store-credentials "$profile" \\
      --apple-id "<your Apple ID email>" --team-id "<your 10-character team ID>"

(it prompts for an app-specific password: appleid.apple.com → Sign-In and
Security → App-Specific Passwords), or with an App Store Connect API key:

  xcrun notarytool store-credentials "$profile" \\
      --key /path/to/AuthKey_XXXXXXXXXX.p8 --key-id <key id> --issuer <issuer id>
EOF
}

# True when the notary profile is configured and present.
notary_ready() {
  if [[ -z "${STAVE_NOTARY_PROFILE:-}" ]]; then
    echo "note: STAVE_NOTARY_PROFILE is not set: not notarising" >&2
    notary_help
    return 1
  fi
  if ! xcrun notarytool history --keychain-profile "$STAVE_NOTARY_PROFILE" >/dev/null 2>&1; then
    echo "note: no notarytool keychain profile \"$STAVE_NOTARY_PROFILE\" (or it does not authenticate): not notarising" >&2
    notary_help
    return 1
  fi
}

# notarize <file.zip|.dmg|.pkg>: submit, wait, fail with Apple's log.
notarize() {
  local file=$1 result id status
  note "notarise $(basename "$file") (minutes)"
  result=$(xcrun notarytool submit "$file" --keychain-profile "$STAVE_NOTARY_PROFILE" --wait --output-format json)
  id=$(printf '%s' "$result" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))')
  status=$(printf '%s' "$result" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))')
  echo "    submission $id: $status"
  if [[ "$status" != Accepted ]]; then
    [[ -n "$id" ]] && xcrun notarytool log "$id" --keychain-profile "$STAVE_NOTARY_PROFILE" >&2
    return 1
  fi
}

# What Gatekeeper and codesign say about one item, as a stranger's Mac
# would ask.
assess() {
  local item=$1 type=open
  [[ "$item" == *.app ]] && type=execute
  echo "--- $(basename "$item")"
  if [[ "$item" == *.pkg ]]; then
    pkgutil --check-signature "$item" 2>&1 | sed -n '1,4p' | sed 's/^/    /'
    spctl --assess -vv --type install "$item" 2>&1 | sed 's/^/    /' || true
    xcrun stapler validate "$item" 2>&1 | tail -1 | sed 's/^/    /' || true
    return
  fi
  codesign --verify --deep --strict --verbose=2 "$item" 2>&1 | sed 's/^/    /' || true
  if [[ "$type" == execute ]]; then
    spctl --assess -vv --type execute "$item" 2>&1 | sed 's/^/    /' || true
  else
    spctl --assess -vv --type open --context context:primary-signature "$item" 2>&1 | sed 's/^/    /' || true
  fi
  if [[ -d "$item" || "$item" == *.dmg || "$item" == *.pkg ]]; then
    xcrun stapler validate "$item" 2>&1 | tail -1 | sed 's/^/    /' || true
  fi
}

# Install the download the way a stranger would — mount the .dmg, copy
# each bundle into a fresh user's folders, mark the copies quarantined
# (as a browser download is) — and assess the installed copies.
test_install() {
  local dmg=$1 home mount
  home=$(mktemp -d "${TMPDIR:-/tmp}/stave-fresh-user.XXXXXX")
  mount=$(mktemp -d "${TMPDIR:-/tmp}/stave-dmg.XXXXXX")
  note "test install into a fresh user's folders ($home)"
  hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$dmg" >/dev/null
  mkdir -p "$home/Library/Audio/Plug-Ins/CLAP" "$home/Library/Audio/Plug-Ins/VST3" "$home/Library/Audio/Plug-Ins/Components" "$home/Applications"
  local b dest
  for b in "$mount"/*.clap "$mount"/*.vst3 "$mount"/*.component "$mount"/*.app; do
    [[ -e "$b" ]] || continue
    case "$b" in
      *.clap) dest="$home/Library/Audio/Plug-Ins/CLAP" ;;
      *.vst3) dest="$home/Library/Audio/Plug-Ins/VST3" ;;
      *.component) dest="$home/Library/Audio/Plug-Ins/Components" ;;
      *.app) dest="$home/Applications" ;;
    esac
    ditto "$b" "$dest/$(basename "$b")"
    xattr -w com.apple.quarantine "0081;$(printf '%x' "$(date +%s)");Safari;" "$dest/$(basename "$b")"
    assess "$dest/$(basename "$b")"
  done
  hdiutil detach "$mount" >/dev/null
  if [[ -x "$home/Applications/$NAME.app/Contents/MacOS/$NAME" ]]; then
    echo "--- the installed (quarantined) app, run:"
    local status=0
    HOME="$home" "$home/Applications/$NAME.app/Contents/MacOS/$NAME" --list-devices > "$home/run.log" 2>&1 || status=$?
    sed 's/^/    /' "$home/run.log"
    case "$status" in
      0) echo "    ran (exit 0)" ;;
      137) echo "    killed by the system (SIGKILL): Gatekeeper refuses to run the quarantined app, as on a stranger's Mac" ;;
      *) echo "    exit $status" ;;
    esac
  fi
  rm -rf "$home" "$mount"
}

macos_sign_and_ship() {
  local base="$OUT/${NAME}-${STAVE_BUNDLE_VERSION}-macos" identity="" items=() b
  for b in "$BUNDLES"/*.clap "$BUNDLES"/*.vst3 "$BUNDLES"/*.component "$BUNDLES"/*.app; do
    [[ -e "$b" ]] && items+=("$b")
  done
  ((${#items[@]})) || { echo "nothing to sign in $BUNDLES" >&2; return 1; }

  if [[ -n "${STAVE_SIGN_IDENTITY:-}" ]]; then
    identity=$(resolve_identity "$STAVE_SIGN_IDENTITY" STAVE_SIGN_IDENTITY)
    [[ -n "$identity" ]] || return 1
    note "sign with the configured identity (SHA-1 ${identity:0:8}…), hardened runtime, secure timestamp"
    local ent
    ent=$(app_entitlements)
    for b in "${items[@]}"; do
      if [[ "$b" == *.app ]]; then
        codesign --force --timestamp --options runtime --entitlements "$ent" --sign "$identity" "$b"
      else
        codesign --force --timestamp --options runtime --sign "$identity" "$b"
      fi
      codesign --verify --deep --strict --verbose=2 "$b"
    done
  else
    echo "note: STAVE_SIGN_IDENTITY is not set: bundles stay ad-hoc signed; another Mac's Gatekeeper refuses them" >&2
  fi

  local notarized=0
  if [[ -n "$identity" ]] && notary_ready; then
    local zipdir="$OUT/notarize"
    rm -rf "$zipdir"
    mkdir -p "$zipdir/$NAME"
    for b in "${items[@]}"; do ditto "$b" "$zipdir/$NAME/$(basename "$b")"; done
    ditto -c -k --keepParent "$zipdir/$NAME" "$zipdir/$NAME.zip"
    notarize "$zipdir/$NAME.zip"
    rm -rf "$zipdir"
    for b in "${items[@]}"; do xcrun stapler staple "$b"; done
    notarized=1
  fi

  note "disk image"
  local dmgroot="$OUT/dmg"
  rm -rf "$dmgroot" "$base.dmg"
  mkdir -p "$dmgroot"
  for b in "${items[@]}"; do ditto "$b" "$dmgroot/$(basename "$b")"; done
  install_notes > "$dmgroot/Install.txt"
  ln -s /Library/Audio/Plug-Ins/CLAP "$dmgroot/CLAP"
  ln -s /Library/Audio/Plug-Ins/VST3 "$dmgroot/VST3"
  [[ -e "$dmgroot/$NAME.component" ]] && ln -s /Library/Audio/Plug-Ins/Components "$dmgroot/Components"
  [[ -e "$dmgroot/$NAME.app" ]] && ln -s /Applications "$dmgroot/Applications"
  hdiutil create -quiet -volname "$NAME $STAVE_BUNDLE_VERSION" -srcfolder "$dmgroot" -ov -format UDZO "$base.dmg"
  rm -rf "$dmgroot"
  if [[ -n "$identity" ]]; then
    codesign --force --timestamp --sign "$identity" "$base.dmg"
    if ((notarized)); then
      notarize "$base.dmg"
      xcrun stapler staple "$base.dmg"
    fi
  fi

  # A flat installer package needs a Developer ID Installer identity.
  if [[ -n "${STAVE_INSTALLER_IDENTITY:-}" ]]; then
    note "installer package"
    local root="$OUT/pkgroot"
    rm -rf "$root"
    mkdir -p "$root/Library/Audio/Plug-Ins/CLAP" "$root/Library/Audio/Plug-Ins/VST3" "$root/Library/Audio/Plug-Ins/Components" "$root/Applications"
    for b in "${items[@]}"; do
      case "$b" in
        *.clap) ditto "$b" "$root/Library/Audio/Plug-Ins/CLAP/$(basename "$b")" ;;
        *.vst3) ditto "$b" "$root/Library/Audio/Plug-Ins/VST3/$(basename "$b")" ;;
        *.component) ditto "$b" "$root/Library/Audio/Plug-Ins/Components/$(basename "$b")" ;;
        *.app) ditto "$b" "$root/Applications/$(basename "$b")" ;;
      esac
    done
    local installer
    installer=$(resolve_identity "$STAVE_INSTALLER_IDENTITY" STAVE_INSTALLER_IDENTITY)
    [[ -n "$installer" ]] || return 1
    # Not relocatable: Installer would otherwise "upgrade" a copy of the
    # same bundle id wherever it finds one (a build folder) instead of
    # installing where the package says.
    local components="$OUT/.components.plist"
    pkgbuild --analyze --root "$root" "$components" >/dev/null
    python3 - "$components" <<'PY'
import plistlib, sys
path = sys.argv[1]
with open(path, "rb") as f:
    items = plistlib.load(f)
for item in items:
    item["BundleIsRelocatable"] = False
with open(path, "wb") as f:
    plistlib.dump(items, f)
PY
    pkgbuild --root "$root" --component-plist "$components" --install-location / --identifier "$ID_BASE.pkg" \
      --version "$STAVE_BUNDLE_VERSION" --sign "$installer" --timestamp "$base.pkg"
    rm -rf "$root" "$components"
    if ((notarized)); then
      notarize "$base.pkg"
      xcrun stapler staple "$base.pkg"
    fi
  elif security find-identity -v | grep -qF '"Developer ID Installer: '; then
    echo "note: a Developer ID Installer identity is in the keychain; set STAVE_INSTALLER_IDENTITY to build a .pkg too" >&2
  else
    echo "note: no .pkg: that needs a \"Developer ID Installer\" certificate (developer.apple.com → Certificates), then STAVE_INSTALLER_IDENTITY" >&2
  fi

  note "verify"
  for b in "${items[@]}"; do assess "$b"; done
  assess "$base.dmg"
  [[ -f "$base.pkg" ]] && assess "$base.pkg"
  if command -v syspolicy_check >/dev/null; then
    echo "--- syspolicy_check distribution (what notarisation and Gatekeeper require)"
    for b in "${items[@]}"; do syspolicy_check distribution "$b" 2>&1 | sed 's/^/    /' || true; done
  fi
  test_install "$base.dmg"
  if ((notarized)); then
    echo "notarised and stapled: a stranger's Mac opens $(basename "$base").dmg and every bundle in it without a warning"
  elif [[ -n "$identity" ]]; then
    echo "signed, NOT notarised: on a stranger's Mac Gatekeeper refuses the quarantined download (\"Apple could not verify … is free of malware\"); notarise to ship"
  else
    echo "ad-hoc signed only: for this machine; Gatekeeper refuses it anywhere else"
  fi
}

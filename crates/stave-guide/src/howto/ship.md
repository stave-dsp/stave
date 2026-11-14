# Ship your plugin

A plugin is shipped when a stranger can download it, install it and
run it: on macOS without a Gatekeeper warning, on Windows and Linux
with install steps that need no developer. This page makes a plugin its
own app (the standalone), packages every format into one download per
OS, signs and notarises it on macOS, and runs the same pipeline in CI
on a published release.

## The standalone

Any stave plugin also runs as its own app: its face is the window, the
audio device plays it, a MIDI keyboard plays an instrument, and its
state and the device choices are there next launch. Two lines in the
plugin crate turn it on. `src/main.rs`, beside `src/lib.rs`:

```rust,ignore
stave::standalone!(stave_tilt_vst3::Tilt);
```

and in its `Cargo.toml`:

```toml
[features]
standalone = ["stave/standalone"]

[[bin]]
name = "stave-tilt-vst3-standalone"   # <crate>-standalone
path = "src/main.rs"
required-features = ["standalone"]
```

The feature keeps the audio device layer out of the plugin library: the
VST3 and CLAP never link it. Try it:

```sh
cargo run -p stave-tilt-vst3 --features standalone --release
```

In the window, **Tab** swaps the face for the audio settings and back:
output device, input device, sample rate, buffer size and MIDI input,
each a stepper; a change takes effect when you let go, and each shows
what actually runs ("Device (48000 Hz)"). Audio input starts **off** —
an effect on a laptop would otherwise feed the speakers back into the
microphone — so choose an input to hear one. An instrument (or any
plugin without an audio input) takes MIDI from every input by default.

| Flag | Does |
|---|---|
| `--list-devices` | print the audio devices and MIDI sources, and exit |
| `--settings-dir` | print where the settings and state live, and exit |
| `--audio-settings` | open on the audio settings instead of the face |

The settings and the plugin's state live in the per-user settings
folder named by the plugin's `Info::id` — `~/Library/Application
Support/<id>/` on macOS, `%APPDATA%\<id>\` on Windows,
`$XDG_CONFIG_HOME/<id>/` on Linux: `settings.txt` (`key=value` lines,
hand-editable) and `state.bin` (the same chunk a DAW session stores).
The state is saved within a second of an edit and when the window
closes.

Underneath (`stave-standalone`): the plugin runs on the same runtime the
VST3 and CLAP wrappers use — the exclusive claim on the plugin, the
scratch, the framework's bypass, the I/O and CPU taps a face's meters read — with
the audio device in the host's seat. Audio I/O is cpal (CoreAudio,
WASAPI, ALSA; PipeWire and JACK through their ALSA plugins), the one
dependency it takes: three device backends of thousands of lines each,
already the workspace's audio layer. ASIO is cpal's `asio` feature,
which needs Steinberg's ASIO SDK at build time; it is not built. MIDI
input is stave's own code on each system API — CoreMIDI, WinMM, the
ALSA sequencer — a few hundred lines in all, against a dependency that
would buy little.

## Package

```sh
scripts/package.sh stave-tilt-vst3          # build, then sign
scripts/package.sh stave-tilt-vst3 build    # bundles only
scripts/package.sh stave-tilt-vst3 sign     # sign, notarise, download
```

`build` makes release builds and the bundles in
`target/package/<crate>/bundles/`:

| OS | Bundles | Download |
|---|---|---|
| macOS | universal (arm64 + x86_64): `.clap`, `.vst3`, `.app` (the standalone), `.component` (Audio Unit, when the framework builds one) | `<Name>-<version>-macos.dmg`, and `.pkg` with an installer identity |
| Windows | `.clap`, `.vst3`, `.exe` | `<Name>-<version>-windows.zip`, and `-setup.exe` where Inno Setup is installed |
| Linux | `.clap`, `.vst3`, the standalone binary | `<Name>-<version>-linux.tar.gz` with `install.sh` |

Every download carries install notes naming the folders hosts scan. The
`.dmg` holds the bundles beside links to the system plug-in folders and
Applications, to drag onto; the Windows installer installs for the
current user (no administrator); the Linux `install.sh` copies into
`~/.vst3`, `~/.clap` and `~/.local/bin`.

`build` runs on any machine with the toolchain, a build host included:

```sh
STAVE_OFFLOAD_FETCH=target/package scripts/offload.sh macos -- scripts/package.sh stave-tilt-vst3 build
scripts/package.sh stave-tilt-vst3 sign     # here, where the identity is
```

## Sign and notarise on macOS

A stranger's Mac quarantines a download and asks Gatekeeper about it.
Gatekeeper passes code that is **signed** with a Developer ID (with the
hardened runtime and a secure timestamp) and **notarised** by Apple,
with the notarisation ticket **stapled** to it so the check passes
offline too. `sign` does all three, to every bundle and then to the
`.dmg` itself. Without them:

| Signed | Notarised | On a stranger's Mac |
|---|---|---|
| ad-hoc | — | refused: "cannot be opened because the developer cannot be verified" |
| Developer ID | no | refused: "Apple could not verify … is free of malware"; a quarantined app is killed at launch |
| Developer ID | yes, stapled | opens; the DAW loads the plugins |

The configuration is yours and never enters the repository (put it in
`~/.config/stave/env`; see [Local configuration](crate::howto::local_config)):

```sh
# The identity, as `security find-identity -v -p codesigning` prints it
# (or its SHA-1). Several certificates under one name — a renewed
# Developer ID beside the old one — resolve to the one that expires last.
STAVE_SIGN_IDENTITY="Developer ID Application: <Your Name> (<TEAM_ID>)"
# The notarytool keychain profile.
STAVE_NOTARY_PROFILE=<profile>
# Optional: a Developer ID Installer identity, for a .pkg beside the .dmg.
STAVE_INSTALLER_IDENTITY="Developer ID Installer: <Your Name> (<TEAM_ID>)"
```

Make the notary profile once; `notarytool` keeps the credentials in your
keychain and the script never sees them (it prints this command when the
profile is missing):

```sh
xcrun notarytool store-credentials "<profile>" \
    --apple-id "<your Apple ID email>" --team-id "<TEAM_ID>"
# prompts for an app-specific password (appleid.apple.com → Sign-In and
# Security → App-Specific Passwords); or, with an App Store Connect key:
# --key AuthKey_XXXXXXXXXX.p8 --key-id <key id> --issuer <issuer id>
```

The plugin bundles are signed without entitlements (they run inside the
host's process, under the host's); the app gets
`com.apple.security.device.audio-input`, and its `Info.plist` says why it
asks for the microphone. `sign` then verifies what it made the way a
stranger's Mac would — `codesign --verify --deep --strict`, `spctl
--assess`, `stapler validate`, `syspolicy_check distribution` — and
installs the `.dmg` into a fresh user's plug-in folders with the
quarantine flag a browser sets, assessing each installed copy and
launching the app. `scripts/check-local-leaks.sh` refuses a commit that
holds your identity, its team ID or your profile name, and any real
`Developer ID …: Name (TEAMID)` string at all.

## Windows and Linux

Windows code signing is Authenticode: with a certificate (`.pfx`) in
`STAVE_WIN_SIGN_CERT` and its password in `STAVE_WIN_SIGN_PASSWORD`,
`sign` runs `signtool` (SHA-256, timestamped) over the `.exe`, the
plugin binaries and the installer. Without one the download is
unsigned and SmartScreen warns on first run ("More info → Run anyway");
the install notes say so. Linux downloads are not signed; publish a
checksum beside them if you want one checked.

## In CI

`.github/workflows/release.yml` packages on macOS, Windows and Linux
when a GitHub release is published (`scripts/release.sh` publishes one)
and attaches the downloads to it; run it by hand from the Actions tab
for any plugin crate. It signs only when the secrets exist, and builds
and uploads either way:

| Secret | What |
|---|---|
| `MACOS_CERTIFICATE_P12_BASE64`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY` | the Developer ID Application certificate with its key, exported from Keychain Access as `.p12`, base64 (`base64 -i cert.p12 \| pbcopy`); its password; its name |
| `MACOS_INSTALLER_P12_BASE64`, `MACOS_INSTALLER_PASSWORD`, `MACOS_INSTALLER_IDENTITY` | optional: the same for a Developer ID Installer certificate (the `.pkg`) |
| `NOTARY_APPLE_ID`, `NOTARY_TEAM_ID`, `NOTARY_PASSWORD` | notarisation with an app-specific password |
| `NOTARY_API_KEY_P8_BASE64`, `NOTARY_API_KEY_ID`, `NOTARY_API_ISSUER` | or with an App Store Connect API key |
| `WINDOWS_CERTIFICATE_PFX_BASE64`, `WINDOWS_CERTIFICATE_PASSWORD` | Authenticode |

The macOS job imports the certificates into a keychain made for the job
(a random password, first in the search list so `codesign` and
`notarytool` find it), stores the notary credentials there as a profile,
and deletes the keychain when the job ends, pass or fail. No secret is
written to the repository or printed.

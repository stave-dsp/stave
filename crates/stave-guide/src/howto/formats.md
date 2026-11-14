# Plugin formats

[`stave::export!`] makes one plugin crate every format a host can load:
a VST3, a CLAP plugin and, on macOS, an Audio Unit (v2). Each is a thin
translation of its ABI onto the same runtime — the exclusive claim on the plugin, the
state bytes, the click-free bypass, the telemetry, the panic guard — so
a plugin behaves the same in all three, and is tested once.

| Format | Platforms | Bundle | Validator |
|---|---|---|---|
| VST3 | macOS, Windows, Linux | `StaveTilt.vst3` | Steinberg's `validator` |
| CLAP | macOS, Windows, Linux | `StaveTilt.clap` | `clap-validator` |
| Audio Unit v2 | macOS | `StaveTilt.component` | Apple's `auval` |

Every format also passes stave-host's own validation suite:
`stave-bench <bundle> --validate` opens a `.vst3` or a `.component`
(see [Use the host tools](crate::howto::host_tools)).

## Identity

Hosts find a plugin again — in a saved session, a preset, an automation
lane — by its identity, so a shipped plugin's identity never changes.
[`stave::identity`] returns all of it, to pin in a test:

- **CLAP**: [`Info::id`](stave::Info::id), the reverse-DNS id.
- **VST3**: two class ids derived from `Info::id`, or pinned with
  `export!`'s `vst3_class_ids`.
- **Audio Unit**: a type, a subtype and a manufacturer code, four
  characters each. The type follows the plugin's kind: `aumu` for an
  instrument (no audio input, or a `Feature::Instrument`-like feature),
  `aumf` for an effect that takes notes (`Feature::NoteEffect`), `aufx`
  for any other effect. Subtype and manufacturer come from
  [`Info::au`](stave::Info::au), or are derived when it is `None`: the
  manufacturer from the vendor's first four letters (`Ryan` for "Ryan
  Bahan"), the subtype from a hash of the id. Pin them before shipping
  — two vendors whose names start alike derive the same manufacturer,
  and Apple reserves all-lowercase manufacturer codes:

```
use stave::prelude::*;
use stave::AuCodes;

const INFO: Info = Info {
    id: "com.example.tilt",
    name: "Tilt",
    vendor: "Example",
    au: Some(AuCodes::new(*b"Exmp", *b"tilt")),
    ..Info::EFFECT
};
# assert_eq!(INFO.au.unwrap().manufacturer, *b"Exmp");
```

## Bundling and installing

`scripts/install.sh <crate> [release|debug]` builds, bundles, validates
and installs every format this OS has. The steps alone:

```sh
scripts/bundle-vst3.sh stave-tilt-vst3 release     # build/StaveTilt.vst3
scripts/bundle-clap.sh stave-tilt-vst3 release     # build/StaveTilt.clap
scripts/bundle-au.sh stave-tilt-vst3 release       # build/StaveTilt.component (macOS)
```

An Audio Unit's `Info.plist` carries an `AudioComponents` entry — type,
subtype, manufacturer, "Vendor: Name", version, the factory function —
which hosts read before loading the binary. `export!` builds that entry
at compile time from the plugin's `Info` and puts it in the binary
(section `__DATA,__stave_au`); `bundle-au.sh` copies it into the
`Info.plist`, so the codes are never typed twice. Audio Units install to
`~/Library/Audio/Plug-Ins/Components`; AudioToolbox's registrar notices
a new component within seconds.

## Validating

`scripts/validate-formats.sh [vst3|clap|au|all]` builds every plugin in
`examples/` and runs each format's validator over it; CI runs all three
on macOS (the `validators` and `validators (auval)` jobs) and VST3 and
CLAP on Linux and Windows. For one Audio Unit by hand:

```sh
auval -v aufx 2br5 Ryan    # type subtype manufacturer: stave::identity, or the Info.plist
```

`stave-bench build/StaveTilt.component --validate` loads that bundle's
own binary and registers it in-process, so it checks exactly the bundle
named, installed or not. stave-host's tests do the same with each
example linked in: Tilt (`tests/au_effect.rs`) and the instrument Pluck
(`tests/au_instrument.rs`), driven through AudioToolbox and held to the
validation suite.

## What the Audio Unit does

- **Audio**: 32-bit float, non-interleaved, in place or not; the host's
  input callback or a connection feeds the main input, and each
  sidechain is an input element of its own (silent when unconnected).
  The channel counts a host may choose are the plugin's
  [`Layout`](stave::Layout)s (`SupportedNumChannels`); `Initialize`
  refuses others.
- **Parameters**: every param in the global scope with its id, range,
  unit and default; stepped params have value strings; every value's
  text comes from the param's own `format` and `parse`. Host automation
  sets the table, which the plugin's smoothers read each block
  (block-accurate, not sample-accurate). Edits on the face are sent to
  the host's listeners as begin / change / end gestures, so a host
  records them as automation.
- **State**: `ClassInfo` holds the framework's state bytes — the same
  bytes as the other formats, unknown params and migrations included.
- **Latency, tail, bypass**: the plugin's latency and tail; the host's
  `BypassEffect` goes through the same latency-matched gate as the bypass
  param.
- **Notes**: instruments and music effects take MIDI 1.0 through
  `MusicDeviceMIDIEvent`: note on and off (and all-notes-off as a
  choke), each at its frame. Other MIDI messages are ignored.
- **Transport**: playing, tempo, musical position and the timeline
  position from the host's callbacks.
- **Face**: a Cocoa view (`AudioUnitCocoaViewInfo`) showing the same
  face, at its own size.

Not yet: factory presets as `AUPreset`s, sample-accurate automation,
MIDI beyond notes, AUv3 (FRAMEWORK.md F-AUV3).

## A GarageBand smoke test

GarageBand is a real Audio Unit host on every Mac. After
`scripts/install.sh stave-tilt-vst3` and
`scripts/install.sh stave-pluck`:

1. Open GarageBand, make an empty project with a **Software Instrument**
   track. In the track's Smart Controls (`B`), open the **Plug-ins**
   section; on the instrument slot choose **AU Instruments → Ryan Bahan
   → Stave Pluck**. Play notes (Musical Typing, `⌘K`): each sounds, and
   stops ringing soon after its key is released.
2. On an empty plug-in slot of the same track choose **Audio Units →
   Ryan Bahan → Stave Tilt**. Its face opens in a window: turn Tilt to
   +6 dB and the sound brightens; click the bypass footswitch and it
   returns.
3. Show the track's automation (`A`), pick Stave Tilt's **Tilt**, draw
   a ramp, play: the knob on the face follows. Turn the knob while
   recording: GarageBand records the moves.
4. Save, quit, reopen the project: both plugins come back with their
   settings, and the face shows them.
5. Change the project's sample rate or the audio buffer size in
   Preferences while playing: no glitch beyond the change, no crash.

Note the date, the GarageBand and macOS versions, and anything that
misbehaved; a pass is what FRAMEWORK.md's F-DAW matrix records.

//! Run a stave plugin as its own app — the standalone: the plugin's face
//! is the window, the audio device is the host, a MIDI keyboard plays
//! it, and its state and the device choices are there next launch.
//!
//! An author never names this crate: `stave::standalone!(MyPlugin);` in
//! the plugin crate's `src/main.rs`, behind the `standalone` feature
//! (the guide: Ship your plugin). [`run`] is what that expands to.
//!
//! ```text
//! session     the plugin instance on stave-core's runtime (as a format
//!             wrapper runs it), behind the erased `Instance`
//! audio       cpal: devices, rate, buffer; input FIFO; the callback
//! midi/       MIDI input on CoreMIDI, WinMM, the ALSA sequencer
//! controller  the window's Controller: plugin params + audio settings
//! settings    settings.txt + state.bin in the per-user settings folder
//! ```
//!
//! Tab swaps the plugin's face for the audio settings and back. Input
//! starts off (an effect on a laptop would otherwise feed the speakers
//! back into the microphone); choose an input device to hear it.

mod audio;
mod controller;
mod midi;
mod session;
pub mod settings;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use stave_core::runtime::Erased;
use stave_face::Face;

pub use controller::SETTINGS_BASE;

#[cfg(target_os = "macos")]
mod shell {
    pub use stave_ui_cocoa::{run_app, FaceView};
    /// kVK_Tab.
    pub const TAB: u16 = 48;
}
#[cfg(windows)]
mod shell {
    pub use stave_ui_win32::keys::TAB;
    pub use stave_ui_win32::{run_app, FaceView};
}
#[cfg(all(unix, not(target_os = "macos")))]
mod shell {
    pub use stave_ui_x11::keys::TAB;
    pub use stave_ui_x11::{run_app, FaceView};
}

/// A plugin the standalone can run: the erased contract plus its face.
/// `stave::export!` implements it (with the `standalone` feature).
pub trait Standalone: Erased {
    fn face(&self) -> Option<Face> {
        None
    }
}

/// How often the saver looks for edits to save.
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// Run `P` as an app until its window closes. Flags: `--list-devices`
/// prints the audio devices and MIDI sources; `--settings-dir` prints
/// where settings and state live; `--audio-settings` opens on the audio
/// settings instead of the plugin's face.
pub fn run<P: Standalone>() {
    let store = settings::Store::for_plugin(P::INFO.id);
    let mut settings_first = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--audio-settings" => settings_first = true,
            "-h" | "--help" => {
                println!(
                    "{name} — the plugin as its own app. In the window, Tab swaps the face and the audio settings.\n\n\
                     usage: {exe} [--audio-settings | --list-devices | --settings-dir]\n\n  \
                     --audio-settings  open on the audio settings (output, input, sample rate, buffer, MIDI)\n  \
                     --list-devices    print the audio devices and MIDI sources, and exit\n  \
                     --settings-dir    print where settings and state are saved, and exit",
                    name = P::INFO.name,
                    exe = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_default()
                );
                return;
            }
            "--list-devices" => {
                let d = audio::devices();
                println!("outputs (default {}):", d.default_output.as_deref().unwrap_or("none"));
                d.outputs.iter().for_each(|n| println!("  {n}"));
                println!("inputs (default {}):", d.default_input.as_deref().unwrap_or("none"));
                d.inputs.iter().for_each(|n| println!("  {n}"));
                println!("MIDI sources:");
                midi::sources().iter().for_each(|n| println!("  {n}"));
                return;
            }
            "--settings-dir" => {
                println!("{}", store.as_ref().map_or_else(|| "(no settings folder)".into(), |s| s.dir().display().to_string()));
                return;
            }
            other => eprintln!("{}: ignoring argument {other:?}", P::INFO.name),
        }
    }

    let session = match session::Session::<P>::new() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("{}: {e}", P::INFO.name);
            std::process::exit(1);
        }
    };
    let instance: Arc<dyn session::Instance> = session;
    if let Some(bytes) = store.as_ref().and_then(settings::Store::state) {
        if let Err(e) = instance.load(&bytes) {
            eprintln!("{}: saved state not loaded ({e:?}); starting fresh", P::INFO.name);
        }
    }
    if let Some(store) = store.clone() {
        spawn_saver(instance.clone(), store);
    }

    let window = Rc::new(controller::Window::new(instance, store, P::INFO.name));
    window.start_audio();
    window.start_midi();
    window.refresh();

    let plugin_face = window.instance.face();
    let settings_first = settings_first || plugin_face.is_none();
    let first = match &plugin_face {
        Some(face) if !settings_first => face.clone(),
        _ => window.settings_face(),
    };
    let mut view = shell::FaceView::new(first, Box::new(controller::Handle(window.clone())));
    if let Some(plugin_face) = plugin_face {
        let mut settings_shown = settings_first;
        view.on_key(move |code| {
            if code != shell::TAB {
                return None;
            }
            settings_shown = !settings_shown;
            Some(if settings_shown {
                window.refresh();
                window.settings_face()
            } else {
                plugin_face.clone()
            })
        });
    }
    shell::run_app(&format!("{}  (Tab: audio settings)", P::INFO.name), view);
}

/// Save the state within a second of any parameter edit (the window may
/// be closed in ways that never run a destructor), and log the faults
/// the audio thread recorded. Comparing values needs no processor claim,
/// so an idle plugin's audio is never interrupted by a save.
fn spawn_saver(instance: Arc<dyn session::Instance>, store: settings::Store) {
    let spawned = std::thread::Builder::new().name("stave-saver".into()).spawn(move || {
        let (mut last, mut now) = (Vec::new(), Vec::new());
        instance.values(&mut last);
        loop {
            std::thread::sleep(SAVE_EVERY);
            instance.report_faults();
            instance.values(&mut now);
            if now != last {
                if let Err(e) = store.save_state(&instance.save()) {
                    eprintln!("state not saved: {e}");
                }
                core::mem::swap(&mut last, &mut now);
            }
        }
    });
    if let Err(e) = spawned {
        eprintln!("no saver thread ({e}): state saves on quit only");
    }
}

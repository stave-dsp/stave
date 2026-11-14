//! The Audio Unit path end to end on an instrument: Pluck, linked
//! in-process, registered with AudioToolbox and driven by notes through
//! `MusicDeviceMIDIEvent` (see `au_common`).

#![cfg(target_os = "macos")]

mod au_common;

use au_common::{energy, BLOCK};
use stave_core::{Event, NoteEvent};
use stave_host::au::AuInstance;
use stave_host::instance::Instance;

fn pluck() -> AuInstance {
    au_common::open(b"aumu", "stave tests: Pluck", stave_pluck::StaveAUFactory)
}

#[test]
fn sounds_on_notes_and_only_then() {
    let mut p = pluck();
    assert!(p.layout().is_instrument(), "{:?}", p.layout());
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    p.process(&[], &mut [&mut left, &mut right], &[]);
    assert_eq!(energy(&left), 0.0, "silent before a note");
    let on = [Event::note(17, NoteEvent::on(57, 0.9))];
    p.process(&[], &mut [&mut left, &mut right], &on);
    assert!(left[..17].iter().all(|v| *v == 0.0), "the note starts at its frame");
    assert!(energy(&left) > 1e-4, "a plucked string sounds");
}

#[test]
fn passes_the_validation_suite() {
    au_common::passes_the_validation_suite(pluck);
}

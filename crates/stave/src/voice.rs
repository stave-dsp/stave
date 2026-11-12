//! Notes into a [`VoiceRack`](stave_dsp::VoiceRack)'s events.

use stave_core::{NoteEvent, NoteKind};
use stave_dsp::VoiceEvent;

/// A note as a voice rack event.
pub trait ToVoiceEvent {
    /// This note for a voice rack, its pitch resolved by `tuning` (key →
    /// Hz; tuning is the instrument's business).
    fn voice_event(&self, tuning: impl Fn(i16) -> f32) -> VoiceEvent;
}

impl ToVoiceEvent for NoteEvent {
    fn voice_event(&self, tuning: impl Fn(i16) -> f32) -> VoiceEvent {
        let (id, key) = (self.note_id, self.key);
        match self.kind {
            NoteKind::On => VoiceEvent::On {
                id,
                key,
                hz: tuning(key),
                velocity: self.velocity as f32,
            },
            NoteKind::Off => VoiceEvent::Off { id, key },
            NoteKind::Choke => VoiceEvent::Choke { id, key },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_becomes_a_voice_event_with_its_tuned_pitch() {
        let on = NoteEvent {
            kind: NoteKind::On,
            key: 69,
            channel: 0,
            velocity: 0.5,
            note_id: -1,
        };
        let VoiceEvent::On { hz, velocity, .. } = on.voice_event(|k| f32::from(k) * 2.0) else {
            panic!("an on stays an on");
        };
        assert_eq!((hz, velocity), (138.0, 0.5));
    }
}

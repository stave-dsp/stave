#[derive(Copy, Clone, Debug)]
pub struct Event {
    /// Sample offset from the start of the current process block.
    pub frame: u32,
    pub kind: EventKind,
}

#[derive(Copy, Clone, Debug)]
pub enum EventKind {
    Note(NoteEvent),
}

#[derive(Copy, Clone, Debug)]
pub struct NoteEvent {
    pub kind: NoteKind,
    /// 0..=127, or -1 for "no key" (use note_id instead)
    pub key: i16,
    pub channel: i16,
    pub velocity: f64,
    pub note_id: i32,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum NoteKind {
    On,
    Off,
    Choke,
}

/// The block's events, taken as their frames come due.
pub struct EventCursor<'a> {
    events: &'a [Event],
    next: usize,
}

impl<'a> EventCursor<'a> {
    pub fn new(events: &'a [Event]) -> Self {
        Self { events, next: 0 }
    }

    /// Events at or before `frame` not yet taken, in order (wrappers
    /// deliver them sorted by frame).
    pub fn due(&mut self, frame: usize) -> impl Iterator<Item = &'a Event> + '_ {
        core::iter::from_fn(move || {
            let event = self
                .events
                .get(self.next)
                .filter(|e| e.frame as usize <= frame)?;
            self.next += 1;
            Some(event)
        })
    }
}

impl NoteEvent {
    /// This note for a voice rack, its pitch resolved by `tuning`
    /// (key → Hz; tuning is the instrument's business).
    pub fn voice_event(&self, tuning: impl Fn(i16) -> f32) -> stave_dsp::VoiceEvent {
        let (id, key) = (self.note_id, self.key);
        match self.kind {
            NoteKind::On => stave_dsp::VoiceEvent::On {
                id,
                key,
                hz: tuning(key),
                velocity: self.velocity as f32,
            },
            NoteKind::Off => stave_dsp::VoiceEvent::Off { id, key },
            NoteKind::Choke => stave_dsp::VoiceEvent::Choke { id, key },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(frame: u32, kind: NoteKind, key: i16) -> Event {
        Event {
            frame,
            kind: EventKind::Note(NoteEvent {
                kind,
                key,
                channel: 0,
                velocity: 0.5,
                note_id: -1,
            }),
        }
    }

    #[test]
    fn events_come_due_at_their_frames_once_each() {
        let events = [
            note(0, NoteKind::On, 60),
            note(3, NoteKind::Off, 60),
            note(3, NoteKind::On, 62),
        ];
        let mut cursor = EventCursor::new(&events);
        let due: Vec<usize> = (0..6).map(|f| cursor.due(f).count()).collect();
        assert_eq!(due, [1, 0, 0, 2, 0, 0]);
    }

    #[test]
    fn a_note_becomes_a_voice_event_with_its_tuned_pitch() {
        let EventKind::Note(on) = note(0, NoteKind::On, 69).kind;
        let stave_dsp::VoiceEvent::On { hz, velocity, .. } = on.voice_event(|k| f32::from(k) * 2.0)
        else {
            panic!("an on stays an on");
        };
        assert_eq!((hz, velocity), (138.0, 0.5));
    }
}

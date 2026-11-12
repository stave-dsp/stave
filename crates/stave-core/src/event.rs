#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Event {
    /// Frame offset from the start of the block.
    pub frame: usize,
    pub kind: EventKind,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EventKind {
    Note(NoteEvent),
}

impl Event {
    /// `note`, `frame` frames into its block.
    pub fn note(frame: usize, note: NoteEvent) -> Self {
        Self {
            frame,
            kind: EventKind::Note(note),
        }
    }
}

/// A note on, off or choke: the one note model, for plugins and for
/// every host and tool that drives them (stave-host converts to a
/// format's own events at the ABI).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NoteEvent {
    pub kind: NoteKind,
    /// 0..=127, or -1 for "no key" (use note_id instead)
    pub key: i16,
    pub channel: i16,
    pub velocity: f64,
    pub note_id: i32,
}

impl NoteEvent {
    /// A note on of `key` at `velocity` (0–1), channel 0, no note id.
    pub fn on(key: i16, velocity: f64) -> Self {
        Self {
            kind: NoteKind::On,
            key,
            channel: 0,
            velocity,
            note_id: -1,
        }
    }

    /// A note off of `key` with release `velocity` (0–1), channel 0, no
    /// note id.
    pub fn off(key: i16, velocity: f64) -> Self {
        Self {
            kind: NoteKind::Off,
            ..Self::on(key, velocity)
        }
    }
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
                .filter(|e| e.frame <= frame)?;
            self.next += 1;
            Some(event)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(frame: usize, kind: NoteKind, key: i16) -> Event {
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
}

//! Note schedules: what drives an instrument (or a note-aware effect)
//! through any [`Instance`](crate::instance::Instance). The events are
//! stave-core's own [`Event`]s; a VST3 plugin gets them converted at
//! its ABI.

pub use stave_core::{Event, EventKind, NoteEvent, NoteKind};

/// A note held from `on_s` to `off_s` (seconds), as a schedule entry.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Note {
    /// MIDI key, 0..=127.
    pub key: i16,
    /// Attack velocity, 0–1.
    pub velocity: f64,
    pub on_s: f64,
    pub off_s: f64,
}

impl Note {
    /// MIDI's customary default, 100 of 127.
    pub const DEFAULT_VELOCITY: f64 = 100.0 / 127.0;
}

/// A time-ordered set of events, sliced into per-block events.
#[derive(Clone, Debug, Default)]
pub struct NoteSchedule {
    /// (absolute frame, the note) — sorted by frame.
    events: Vec<(u64, NoteEvent)>,
}

impl NoteSchedule {
    pub fn new(notes: &[Note], sample_rate: f64) -> Self {
        let frame = |s: f64| (s.max(0.0) * sample_rate).round() as u64;
        let events = notes
            .iter()
            .flat_map(|n| {
                [
                    (frame(n.on_s), NoteEvent::on(n.key, n.velocity)),
                    (frame(n.off_s), NoteEvent::off(n.key, 0.0)),
                ]
            })
            .collect();
        Self::sorted(events)
    }

    /// Events at their frames from the schedule's start.
    pub fn of_events(events: &[Event]) -> Self {
        Self::sorted(
            events
                .iter()
                .map(|e| {
                    let stave_core::EventKind::Note(n) = e.kind;
                    (e.frame as u64, n)
                })
                .collect(),
        )
    }

    fn sorted(mut events: Vec<(u64, NoteEvent)>) -> Self {
        // Stable: at equal frames an off scheduled before an on stays first.
        events.sort_by_key(|(f, _)| *f);
        Self { events }
    }

    /// Notes held from frame 0 and never released (a steady chord).
    pub fn held(keys: &[i16], velocity: f64) -> Self {
        Self {
            events: keys
                .iter()
                .map(|&key| (0, NoteEvent::on(key, velocity)))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events falling in `[start, start + frames)`, frames made
    /// block-relative, appended to `out`.
    pub fn block(&self, start: u64, frames: usize, out: &mut Vec<Event>) {
        let end = start + frames as u64;
        let first = self.events.partition_point(|(f, _)| *f < start);
        for (f, n) in self.events[first..].iter().take_while(|(f, _)| *f < end) {
            out.push(Event::note((f - start) as usize, *n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(key: i16, on_s: f64, off_s: f64) -> Note {
        Note {
            key,
            velocity: 0.8,
            on_s,
            off_s,
        }
    }

    fn slice(s: &NoteSchedule, start: u64, frames: usize) -> Vec<Event> {
        let mut out = vec![];
        s.block(start, frames, &mut out);
        out
    }

    #[test]
    fn events_land_in_their_block_with_relative_frames() {
        let s = NoteSchedule::new(&[note(60, 0.5, 1.0)], 100.0); // frames 50, 100
        assert!(slice(&s, 0, 50).is_empty());
        assert_eq!(
            slice(&s, 32, 32),
            vec![Event::note(18, NoteEvent::on(60, 0.8))]
        );
        assert_eq!(
            slice(&s, 96, 32),
            vec![Event::note(4, NoteEvent::off(60, 0.0))]
        );
        assert!(slice(&s, 101, 1000).is_empty());
    }

    #[test]
    fn block_boundaries_are_half_open() {
        let s = NoteSchedule::new(&[note(1, 0.64, 2.0)], 100.0); // on at frame 64
        assert!(slice(&s, 0, 64).is_empty());
        assert_eq!(slice(&s, 64, 64)[0].frame, 0);
    }

    #[test]
    fn held_chord_starts_at_zero_and_never_releases() {
        let s = NoteSchedule::held(&[60, 64, 67], 0.8);
        assert_eq!(slice(&s, 0, 1).len(), 3);
        assert!(slice(&s, 1, 1_000_000).is_empty(), "no note-offs, ever");
    }

    #[test]
    fn events_keep_their_frames_from_the_start() {
        let on = Event::note(70, NoteEvent::on(60, 1.0));
        let s = NoteSchedule::of_events(&[on, Event::note(3, NoteEvent::off(61, 0.0))]);
        assert_eq!(slice(&s, 0, 64), vec![Event::note(3, NoteEvent::off(61, 0.0))]);
        assert_eq!(slice(&s, 64, 64), vec![Event::note(6, NoteEvent::on(60, 1.0))]);
    }
}

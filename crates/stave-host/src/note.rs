//! Note events, host-format-agnostic: what drives an instrument (or a
//! note-aware effect) through any [`Instance`](crate::instance::Instance).

/// A note on or off, `offset` frames into the block it is delivered
/// with. `velocity` is the attack velocity of an on and the release
/// velocity of an off, as in MIDI and VST3.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NoteEvent {
    pub offset: u32,
    pub pitch: i16,
    pub velocity: f32,
    pub kind: NoteKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NoteKind {
    On,
    Off,
}

/// A note held from `on_s` to `off_s` (seconds), as a schedule entry.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Note {
    pub pitch: i16,
    pub velocity: f32,
    pub on_s: f64,
    pub off_s: f64,
}

impl Note {
    /// MIDI's customary default, 100 of 127.
    pub const DEFAULT_VELOCITY: f32 = 100.0 / 127.0;
}

/// A time-ordered set of notes, sliced into per-block events.
#[derive(Clone, Debug, Default)]
pub struct NoteSchedule {
    /// (absolute frame, event with offset 0) — sorted by frame.
    events: Vec<(u64, NoteEvent)>,
}

impl NoteSchedule {
    pub fn new(notes: &[Note], sample_rate: f64) -> Self {
        let frame = |s: f64| (s.max(0.0) * sample_rate).round() as u64;
        let event = |n: &Note, kind, velocity| NoteEvent {
            offset: 0,
            pitch: n.pitch,
            velocity,
            kind,
        };
        let mut events: Vec<(u64, NoteEvent)> = notes
            .iter()
            .flat_map(|n| {
                [
                    (frame(n.on_s), event(n, NoteKind::On, n.velocity)),
                    (frame(n.off_s), event(n, NoteKind::Off, 0.0)),
                ]
            })
            .collect();
        // Stable: at equal frames an off scheduled before an on stays first.
        events.sort_by_key(|(f, _)| *f);
        Self { events }
    }

    /// Notes held from frame 0 and never released (a steady chord).
    pub fn held(pitches: &[i16], velocity: f32) -> Self {
        Self {
            events: pitches
                .iter()
                .map(|&pitch| {
                    let on = NoteEvent {
                        offset: 0,
                        pitch,
                        velocity,
                        kind: NoteKind::On,
                    };
                    (0, on)
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events falling in `[start, start + frames)`, offsets made
    /// block-relative, appended to `out`.
    pub fn block(&self, start: u64, frames: usize, out: &mut Vec<NoteEvent>) {
        let end = start + frames as u64;
        let first = self.events.partition_point(|(f, _)| *f < start);
        for (f, e) in self.events[first..].iter().take_while(|(f, _)| *f < end) {
            out.push(NoteEvent {
                offset: (f - start) as u32,
                ..*e
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pitch: i16, on_s: f64, off_s: f64) -> Note {
        Note {
            pitch,
            velocity: 0.8,
            on_s,
            off_s,
        }
    }

    fn slice(s: &NoteSchedule, start: u64, frames: usize) -> Vec<NoteEvent> {
        let mut out = vec![];
        s.block(start, frames, &mut out);
        out
    }

    #[test]
    fn events_land_in_their_block_with_relative_offsets() {
        let s = NoteSchedule::new(&[note(60, 0.5, 1.0)], 100.0); // frames 50, 100
        assert!(slice(&s, 0, 50).is_empty());
        let on = NoteEvent {
            offset: 18,
            pitch: 60,
            velocity: 0.8,
            kind: NoteKind::On,
        };
        let off = NoteEvent {
            offset: 4,
            velocity: 0.0,
            kind: NoteKind::Off,
            ..on
        };
        assert_eq!(slice(&s, 32, 32), vec![on]);
        assert_eq!(slice(&s, 96, 32), vec![off]);
        assert!(slice(&s, 101, 1000).is_empty());
    }

    #[test]
    fn block_boundaries_are_half_open() {
        let s = NoteSchedule::new(&[note(1, 0.64, 2.0)], 100.0); // on at frame 64
        assert!(slice(&s, 0, 64).is_empty());
        assert_eq!(slice(&s, 64, 64)[0].offset, 0);
    }

    #[test]
    fn held_chord_starts_at_zero_and_never_releases() {
        let s = NoteSchedule::held(&[60, 64, 67], 0.8);
        assert_eq!(slice(&s, 0, 1).len(), 3);
        assert!(slice(&s, 1, 1_000_000).is_empty(), "no note-offs, ever");
    }
}

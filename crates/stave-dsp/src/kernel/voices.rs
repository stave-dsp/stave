//! **Voices** — event-keyed lanes: a fixed pool of per-note voice
//! instances under a declared allocation policy.
//!
//! Layering: the kernel does not know hosts exist — [`VoiceEvent`] is
//! the form's own note vocabulary, and engines bridge host events to it.
//! Tuning is the caller's: events carry **hz**, never keys-as-pitch
//! ([`hz_of_key`](crate::hz_of_key) is the equal-temperament helper, not
//! an assumption), so microtuning is a different bridge, not a form
//! change.
//!
//! Sample-accurate by construction: the rack exposes per-frame
//! `dispatch` + `tick` — a note-on dispatched at frame 173 sounds at
//! frame 173 because there is no other place for it to sound. No
//! block-splitting machinery.
//!
//! The invariants the form owns (tests/voice_properties.rs, driven by TWO
//! consumer voices — a sine voice and a 2-op FM voice whose
//! modulator has its own envelope instance, proving per-voice
//! control instancing fits the trait):
//! * a note-on takes an idle voice; with none idle the POLICY picks
//!   the victim (quietest by `level()`, oldest by age) and
//!   retriggers it — never a dropped note;
//! * stealing is click-free: a stolen voice fades over ~2 ms before
//!   the incoming note retriggers it (the rack owns the fade — a naive
//!   immediate retrigger resets phase mid-sound and measured a 6×
//!   waveform discontinuity);
//! * note-off matches id-then-key and RELEASES (the voice rings
//!   through its release; `active()` reports until silent);
//! * choke silences its match immediately;
//! * the sum is superposition: rack output ≡ the per-slot sum of
//!   each voice ticked alone.

/// One per-note node. Stereo out — spread/detune are voice business.
pub trait Voice {
    fn prepare(&mut self, sample_rate: f32);
    fn reset(&mut self);
    /// Begin (or steal-retrigger) a note.
    fn note_on(&mut self, hz: f32, velocity: f32);
    /// Enter release; keep sounding until silent.
    fn note_off(&mut self);
    /// Immediate silence.
    fn choke(&mut self);
    /// Still sounding? False = the slot is reclaimable.
    fn active(&self) -> bool;
    /// Current loudness — what the quietest-steal policy compares.
    fn level(&self) -> f32;
    fn tick(&mut self) -> (f32, f32);
}

/// Who loses when the pool is full. Data, not code.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Steal {
    /// The voice with the lowest `level()` (least missed).
    Quietest,
    /// The longest-held note (round-robin feel).
    Oldest,
}

/// The form's note vocabulary. `id` is the host's note id (CLAP
/// contract: ≥ 0 identifies the exact note; −1 = match by key).
#[derive(Copy, Clone, Debug)]
pub enum VoiceEvent {
    On {
        id: i32,
        key: i16,
        hz: f32,
        velocity: f32,
    },
    Off {
        id: i32,
        key: i16,
    },
    Choke {
        id: i32,
        key: i16,
    },
}

#[derive(Copy, Clone, Debug)]
struct Tag {
    id: i32,
    key: i16,
    /// Monotone allocation stamp — the Oldest policy's clock.
    age: u64,
    /// Held = between on and off. A released voice still sounds
    /// (`active()`), but no longer matches note-offs.
    held: bool,
}

const EMPTY_TAG: Tag = Tag {
    id: -1,
    key: -1,
    age: 0,
    held: false,
};

/// The note a stolen slot will retrigger once its fade lands.
#[derive(Copy, Clone)]
struct PendingOn {
    hz: f32,
    velocity: f32,
    /// Note-off arrived DURING the fade: retrigger straight into
    /// release (the note happened; it was just short).
    released: bool,
}

#[derive(Clone)]
pub struct VoiceRack<V: Voice, const MAX: usize> {
    voices: [V; MAX],
    tags: [Tag; MAX],
    steal: Steal,
    clock: u64,
    /// Steal declick: linear fade length in samples (~2 ms at
    /// prepare; 0 before prepare = immediate retrigger).
    fade_len: u32,
    fades: [u32; MAX],
    pending: [Option<PendingOn>; MAX],
}

impl<V: Voice, const MAX: usize> VoiceRack<V, MAX> {
    pub fn new(steal: Steal, make: impl FnMut(usize) -> V) -> Self {
        Self {
            voices: core::array::from_fn(make),
            tags: [EMPTY_TAG; MAX],
            steal,
            clock: 0,
            fade_len: 0,
            fades: [0; MAX],
            pending: [None; MAX],
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        for v in self.voices.iter_mut() {
            v.prepare(sample_rate);
        }
        self.fade_len = ((0.002 * sample_rate) as u32).max(1);
        self.reset();
    }

    pub fn reset(&mut self) {
        for v in self.voices.iter_mut() {
            v.reset();
        }
        self.tags = [EMPTY_TAG; MAX];
        self.clock = 0;
        self.fades = [0; MAX];
        self.pending = [None; MAX];
    }

    /// Block-rate access to every voice (retuning envelopes, shapes —
    /// the engine's apply_changes reaches voices through here).
    pub fn voices_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.voices.iter_mut()
    }

    /// Sounding voices right now (invariants + telemetry).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active()).count()
    }

    /// Apply one event at the CURRENT frame position.
    pub fn dispatch(&mut self, ev: VoiceEvent) {
        match ev {
            VoiceEvent::On {
                id,
                key,
                hz,
                velocity,
            } => {
                let slot = self.allocate();
                self.clock += 1;
                // The new note claims the slot's IDENTITY immediately
                // (its off must match from now on); a SOUNDING victim
                // fades first and retriggers when the fade lands.
                self.tags[slot] = Tag {
                    id,
                    key,
                    age: self.clock,
                    held: true,
                };
                if self.voices[slot].active() && self.fade_len > 0 {
                    self.fades[slot] = self.fade_len;
                    self.pending[slot] = Some(PendingOn {
                        hz,
                        velocity,
                        released: false,
                    });
                } else {
                    self.fades[slot] = 0;
                    self.pending[slot] = None;
                    self.voices[slot].note_on(hz, velocity);
                }
            }
            VoiceEvent::Off { id, key } => {
                for i in 0..MAX {
                    if Self::hit(self.tags[i], id, key) {
                        self.tags[i].held = false;
                        match self.pending[i].as_mut() {
                            Some(p) => p.released = true,
                            None => self.voices[i].note_off(),
                        }
                    }
                }
            }
            VoiceEvent::Choke { id, key } => {
                for i in 0..MAX {
                    if Self::hit(self.tags[i], id, key) {
                        self.tags[i] = EMPTY_TAG;
                        self.fades[i] = 0;
                        self.pending[i] = None;
                        self.voices[i].choke();
                    }
                }
            }
        }
    }

    /// The id-then-key note-matching contract (CLAP shape): an id
    /// ≥ 0 names the exact note; −1 falls back to the key, which may
    /// match SEVERAL held voices — all of them take the event.
    #[inline]
    fn hit(t: Tag, id: i32, key: i16) -> bool {
        t.held && if id >= 0 { t.id == id } else { t.key == key }
    }

    /// One frame: the superposition of every active voice. Stolen
    /// slots ramp their OLD content to zero, then retrigger — the
    /// non-steal path is untouched (superposition stays bit-exact).
    #[inline]
    pub fn tick(&mut self) -> (f32, f32) {
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for i in 0..MAX {
            let v = &mut self.voices[i];
            if self.fades[i] > 0 {
                self.fades[i] -= 1;
                let g = self.fades[i] as f32 / self.fade_len as f32;
                let (vl, vr) = v.tick();
                l += vl * g;
                r += vr * g;
                if self.fades[i] == 0 {
                    if let Some(p) = self.pending[i].take() {
                        v.note_on(p.hz, p.velocity);
                        if p.released {
                            v.note_off();
                        }
                    }
                }
            } else if v.active() {
                let (vl, vr) = v.tick();
                l += vl;
                r += vr;
            }
        }
        (l, r)
    }

    /// An idle slot, or the policy's victim. A slot mid-fade with a
    /// pending note is CLAIMED, never idle (its old voice may have
    /// parked, but the pending note owns it).
    fn allocate(&self) -> usize {
        if let Some(i) = (0..MAX).find(|&i| !self.voices[i].active() && self.pending[i].is_none()) {
            return i;
        }
        match self.steal {
            Steal::Quietest => {
                let mut best = 0;
                for i in 1..MAX {
                    if self.voices[i].level() < self.voices[best].level() {
                        best = i;
                    }
                }
                best
            }
            Steal::Oldest => {
                let mut best = 0;
                for i in 1..MAX {
                    if self.tags[i].age < self.tags[best].age {
                        best = i;
                    }
                }
                best
            }
        }
    }
}

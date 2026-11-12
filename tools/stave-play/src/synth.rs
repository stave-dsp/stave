//! The built-in sine bank: the keyboard's notes made audible through
//! EFFECT plugins (which ignore note events) — each held note is a
//! plain sine voice mixed into the plugin's INPUT buffers. Synth
//! plugins still get the events themselves; this bank only feeds
//! inputs that exist. Anti-click by construction: 5 ms attack,
//! 80 ms exponential release, and a voice is only reused once its
//! release has died.

use stave_host::note::{NoteEvent, NoteKind};

const VOICES: usize = 8;
const LEVEL: f32 = 0.30;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Off,
    Held,
    Released,
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    pitch: i16,
    phase: f32,
    step: f32,
    env: f32,
    amp: f32,
}

impl Voice {
    const OFF: Voice = Voice {
        stage: Stage::Off,
        pitch: 0,
        phase: 0.0,
        step: 0.0,
        env: 0.0,
        amp: 0.0,
    };
}

pub struct SineBank {
    voices: [Voice; VOICES],
    attack: f32,
    release: f32,
    sr: f32,
}

impl SineBank {
    pub fn new(sr: f32) -> Self {
        Self {
            voices: [Voice::OFF; VOICES],
            // One-pole coefficients for the declared ramps.
            attack: 1.0 - (-1.0 / (0.005 * sr)).exp(),
            release: (-1.0 / (0.080 * sr)).exp(),
            sr,
        }
    }

    /// Start or release a voice for a note event.
    pub fn handle(&mut self, ev: &NoteEvent) {
        if ev.kind == NoteKind::Off {
            for v in self.voices.iter_mut() {
                if v.stage == Stage::Held && v.pitch == ev.pitch {
                    v.stage = Stage::Released;
                }
            }
            return;
        }
        let hz = 440.0 * 2f32.powf((f32::from(ev.pitch) - 69.0) / 12.0);
        // Free slot first; else steal the quietest voice.
        let idx = self
            .voices
            .iter()
            .position(|v| v.stage == Stage::Off)
            .unwrap_or_else(|| {
                (0..VOICES)
                    .min_by(|&a, &b| self.voices[a].env.total_cmp(&self.voices[b].env))
                    .unwrap_or(0)
            });
        self.voices[idx] = Voice {
            stage: Stage::Held,
            pitch: ev.pitch,
            phase: 0.0,
            step: core::f32::consts::TAU * hz / self.sr,
            env: 0.0,
            amp: LEVEL * ev.velocity.clamp(0.0, 1.0),
        };
    }

    /// Mix `frames` of the bank into every input channel (the same
    /// mono source in each lane — a centered image).
    pub fn mix_into(&mut self, inputs: &mut [Vec<f32>], frames: usize) {
        for v in self.voices.iter_mut() {
            if v.stage == Stage::Off {
                continue;
            }
            for f in 0..frames {
                match v.stage {
                    Stage::Held => v.env += (1.0 - v.env) * self.attack,
                    _ => {
                        v.env *= self.release;
                        if v.env < 1e-4 {
                            v.stage = Stage::Off;
                            break;
                        }
                    }
                }
                let s = v.phase.sin() * v.env * v.amp;
                v.phase = (v.phase + v.step) % core::f32::consts::TAU;
                for ch in inputs.iter_mut() {
                    ch[f] += s;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: NoteEvent = NoteEvent {
        offset: 0,
        pitch: 69,
        velocity: 1.0,
        kind: NoteKind::On,
    };

    fn run(bank: &mut SineBank, frames: usize) -> Vec<f32> {
        let mut buf = vec![vec![0.0; frames]];
        bank.mix_into(&mut buf, frames);
        buf.remove(0)
    }

    #[test]
    fn held_note_sounds_then_release_dies_out() {
        let mut b = SineBank::new(48_000.0);
        b.handle(&A4);
        let held = run(&mut b, 4_800);
        assert!(held.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.25);
        b.handle(&NoteEvent {
            kind: NoteKind::Off,
            ..A4
        });
        run(&mut b, 48_000);
        assert!(
            run(&mut b, 480).iter().all(|&v| v == 0.0),
            "voice freed after release"
        );
    }

    #[test]
    fn steals_the_quietest_voice_when_full() {
        let mut b = SineBank::new(48_000.0);
        for p in 0..VOICES as i16 {
            b.handle(&NoteEvent {
                pitch: 60 + p,
                ..A4
            });
            run(&mut b, 100); // later notes have less envelope
        }
        b.handle(&NoteEvent { pitch: 100, ..A4 });
        let pitches: Vec<i16> = b.voices.iter().map(|v| v.pitch).collect();
        assert!(
            pitches.contains(&100) && !pitches.contains(&67),
            "{pitches:?}"
        );
    }
}

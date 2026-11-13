//! The comparison player: an [`Instance`] (no inputs, stereo out) that
//! plays a set of time-aligned takes in a loop, switching between them
//! without clicks. The existing audio engine runs it on the device.
//!
//! - **Switching** ramps each take's gain linearly toward 1 (selected)
//!   or 0 over the crossfade. The takes are latency-aligned and
//!   correlated, so an equal-gain crossfade keeps the level, and since
//!   every take shares one playhead a switch never jumps in time; a
//!   switch during a switch just retargets the ramps. Each take's
//!   loudness-matching gain multiplies in here.
//! - **Looping and seeking** run a second playhead: the new position
//!   fades in while the old fades out (equal power: loop ends are
//!   uncorrelated), so the wrap is as smooth as a switch.
//! - **Start and stop** ramp a master gain.
//!
//! The UI thread talks to it only through [`Transport`]'s atomics;
//! nothing allocates or locks on the audio thread.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use stave_host::instance::{Instance, Layout};
use stave_host::note::Event;

use super::takes::Stereo;

/// The default crossfade: short enough that A/B memory survives the
/// switch, long enough that a polarity flip does not click.
pub const CROSSFADE_SECONDS: f64 = 0.010;

const NO_SEEK: u64 = u64::MAX;

/// What the UI sets and reads; shared with the audio thread.
pub struct Transport {
    pub playing: AtomicBool,
    selected: AtomicUsize,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
    seek: AtomicU64,
    position: AtomicU64,
    frames: u64,
}

impl Transport {
    pub fn new(frames: usize) -> Arc<Self> {
        Arc::new(Self {
            playing: AtomicBool::new(false),
            selected: AtomicUsize::new(0),
            loop_start: AtomicU64::new(0),
            loop_end: AtomicU64::new(frames as u64),
            seek: AtomicU64::new(NO_SEEK),
            position: AtomicU64::new(0),
            frames: frames as u64,
        })
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn select(&self, take: usize) {
        self.selected.store(take, Ordering::Relaxed);
    }

    pub fn selected(&self) -> usize {
        self.selected.load(Ordering::Relaxed)
    }

    pub fn set_playing(&self, on: bool) {
        self.playing.store(on, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// The playhead, frames.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    pub fn seek(&self, frame: u64) {
        self.seek.store(frame.min(self.frames.saturating_sub(1)), Ordering::Relaxed);
    }

    /// The loop region, clamped to the material. (A region shorter than
    /// two crossfades keeps fading; the UI keeps regions longer.)
    pub fn set_loop(&self, start: u64, end: u64) {
        let (start, end) = (start.min(end), start.max(end).min(self.frames));
        self.loop_start.store(start.min(self.frames), Ordering::Relaxed);
        self.loop_end.store(end, Ordering::Relaxed);
    }

    pub fn loop_region(&self) -> (u64, u64) {
        (
            self.loop_start.load(Ordering::Relaxed),
            self.loop_end.load(Ordering::Relaxed),
        )
    }
}

#[derive(Copy, Clone, Default)]
struct Head {
    pos: u64,
    active: bool,
}

pub struct Player {
    takes: Vec<(Arc<Stereo>, f32)>,
    transport: Arc<Transport>,
    sample_rate: f64,
    max_block: usize,
    fade: u64,
    /// Per-take switching gains.
    mix: Vec<f32>,
    master: f32,
    heads: [Head; 2],
    /// Which head is the current one; the other fades out.
    main: usize,
    /// Frames into the current head crossfade (`fade` when none).
    head_fade: u64,
}

impl Player {
    /// `takes` pairs each take with its loudness-matching gain. All
    /// takes are the same length.
    pub fn new(
        takes: Vec<(Arc<Stereo>, f32)>,
        transport: Arc<Transport>,
        sample_rate: f64,
        max_block: usize,
    ) -> Self {
        let fade = ((CROSSFADE_SECONDS * sample_rate) as u64).max(1);
        let mut mix = vec![0.0; takes.len()];
        if let Some(m) = mix.get_mut(transport.selected()) {
            *m = 1.0;
        }
        Self {
            takes,
            transport,
            sample_rate,
            max_block,
            fade,
            mix,
            master: 0.0,
            heads: [
                Head {
                    pos: 0,
                    active: true,
                },
                Head::default(),
            ],
            main: 0,
            head_fade: fade,
        }
    }

    #[cfg(test)]
    pub fn crossfade_frames(&self) -> u64 {
        self.fade
    }

    fn frames(&self) -> u64 {
        self.transport.frames
    }

    fn read(&self, pos: u64, ch: usize) -> f32 {
        let p = pos as usize;
        self.takes
            .iter()
            .zip(&self.mix)
            .filter(|(_, m)| **m > 0.0)
            .map(|((t, g), m)| t[ch].get(p).copied().unwrap_or(0.0) * g * m)
            .sum()
    }

    /// Start a head crossfade to `to` (ignored while one runs).
    fn jump(&mut self, to: u64) -> bool {
        if self.head_fade < self.fade {
            return false;
        }
        let other = 1 - self.main;
        self.heads[other] = Head {
            pos: to,
            active: true,
        };
        self.main = other;
        self.head_fade = 0;
        true
    }

    fn step(&mut self, out: [&mut f32; 2]) {
        let playing = self.transport.is_playing();
        let ramp = 1.0 / self.fade as f32;
        self.master = if playing {
            (self.master + ramp).min(1.0)
        } else {
            (self.master - ramp).max(0.0)
        };
        if self.master == 0.0 && !playing {
            *out[0] = 0.0;
            *out[1] = 0.0;
            return;
        }
        let selected = self.transport.selected();
        for (i, m) in self.mix.iter_mut().enumerate() {
            let target = if i == selected { 1.0 } else { 0.0 };
            *m = if *m < target {
                (*m + ramp).min(target)
            } else {
                (*m - ramp).max(target)
            };
        }
        // A pending seek waits for any running head crossfade.
        let seek = self.transport.seek.load(Ordering::Relaxed);
        if seek != NO_SEEK && self.jump(seek) {
            self.transport.seek.store(NO_SEEK, Ordering::Relaxed);
        }
        let (start, end) = self.transport.loop_region();
        let end = if end <= start { self.frames() } else { end };
        let main_pos = self.heads[self.main].pos;
        if main_pos + self.fade >= end && self.head_fade >= self.fade {
            self.jump(start);
        }
        let (w_main, w_old) = if self.head_fade < self.fade {
            let u = (self.head_fade as f64 + 0.5) / self.fade as f64;
            let a = u * std::f64::consts::FRAC_PI_2;
            (a.sin() as f32, a.cos() as f32)
        } else {
            (1.0, 0.0)
        };
        for (ch, o) in out.into_iter().enumerate() {
            let mut v = w_main * self.read(self.heads[self.main].pos, ch);
            let old = self.heads[1 - self.main];
            if w_old > 0.0 && old.active {
                v += w_old * self.read(old.pos, ch);
            }
            *o = v * self.master;
        }
        let frames = self.frames();
        for h in &mut self.heads {
            if h.active {
                h.pos += 1;
                if h.pos >= frames {
                    // Only a too-short loop gets here: wrap hard.
                    h.pos = start.min(frames.saturating_sub(1));
                }
            }
        }
        if self.head_fade < self.fade {
            self.head_fade += 1;
            if self.head_fade == self.fade {
                self.heads[1 - self.main].active = false;
            }
        }
        self.transport
            .position
            .store(self.heads[self.main].pos, Ordering::Relaxed);
    }
}

impl Instance for Player {
    fn layout(&self) -> Layout {
        Layout {
            inputs: 0,
            outputs: 2,
            sample_rate: self.sample_rate,
            max_block: self.max_block,
        }
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], _events: &[Event]) {
        let n = outputs.first().map_or(0, |o| o.len());
        for i in 0..n {
            let (mut l, mut r) = (0.0, 0.0);
            self.step([&mut l, &mut r]);
            if let Some(o) = outputs.get_mut(0) {
                o[i] = l;
            }
            if let Some(o) = outputs.get_mut(1) {
                o[i] = r;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn tone(hz: f64, amp: f64, frames: usize) -> Arc<Stereo> {
        let x: Vec<f32> = (0..frames)
            .map(|i| (amp * (std::f64::consts::TAU * hz * i as f64 / RATE).sin()) as f32)
            .collect();
        Arc::new([x.clone(), x])
    }

    fn pull(p: &mut Player, frames: usize) -> Vec<f32> {
        let mut l = vec![0.0; frames];
        let mut r = vec![0.0; frames];
        for (lc, rc) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            p.process(&[], &mut [lc, rc], &[]);
        }
        l
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    fn inverted(t: &Stereo) -> Arc<Stereo> {
        Arc::new([t[0].iter().map(|v| -v).collect(), t[1].iter().map(|v| -v).collect()])
    }

    #[test]
    fn switching_never_clicks_even_against_a_polarity_flip() {
        // The worst case: B is A inverted, so a hard switch would jump
        // by twice the amplitude.
        let frames = 96_000;
        let a = tone(1_000.0, 0.5, frames);
        let b = inverted(&a);
        let signal_step = max_step(&a[0]);
        let t = Transport::new(frames);
        let mut p = Player::new(vec![(a, 1.0), (b, 1.0)], t.clone(), RATE, 256);
        t.set_playing(true);
        let mut out = pull(&mut p, 4_800);
        // Switch back and forth, including mid-fade.
        for (take, hold) in [(1, 4_800), (0, 100), (1, 3_000), (0, 4_800)] {
            t.select(take);
            out.extend(pull(&mut p, hold));
        }
        let fade = p.crossfade_frames() as f32;
        // Allow the signal's own slope plus the fade's (2 × amplitude
        // over the crossfade), with a little numerical margin.
        let limit = signal_step + 2.0 * 0.5 / fade + 1e-4;
        let got = max_step(&out[1..]);
        assert!(got <= limit, "step {got} > {limit}");
        // And the level holds through a switch between equal takes.
        let quiet: Vec<f32> = out.iter().map(|v| v.abs()).collect();
        assert!(quiet[2_000..4_000].iter().any(|v| *v > 0.49));
    }

    #[test]
    fn loop_wrap_and_seek_are_smooth() {
        let frames = 48_000;
        // A non-periodic signal across the loop seam: a ramp tone whose
        // end and start differ.
        let a = tone(437.0, 0.5, frames);
        let signal_step = max_step(&a[0]);
        let t = Transport::new(frames);
        let mut p = Player::new(vec![(a, 1.0)], t.clone(), RATE, 256);
        t.set_loop(10_000, 20_000);
        t.seek(10_000);
        t.set_playing(true);
        let out = pull(&mut p, 50_000);
        // Two equal-power fades of uncorrelated material can sum to √2;
        // the per-sample step stays bounded by the signal's own slope
        // times √2 plus the fade slope.
        let fade = p.crossfade_frames() as f32;
        let limit = signal_step * 1.42 + 2.0 * 0.71 / fade;
        let got = max_step(&out[1..]);
        assert!(got <= limit, "step {got} > {limit}");
        // It looped: the playhead stays inside the region.
        let pos = t.position();
        assert!((10_000..20_000).contains(&pos), "{pos}");
    }

    #[test]
    fn start_and_stop_ramp_and_gains_apply() {
        let frames = 48_000;
        let a = tone(1_000.0, 0.5, frames);
        let t = Transport::new(frames);
        let mut p = Player::new(vec![(a.clone(), 0.5)], t.clone(), RATE, 256);
        assert!(pull(&mut p, 1_000).iter().all(|v| *v == 0.0), "silent until played");
        t.set_playing(true);
        let out = pull(&mut p, 4_800);
        assert!(out[0].abs() < 0.01, "fades in");
        let peak = out[2_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.25).abs() < 0.01, "gain 0.5 applied: {peak}");
        t.set_playing(false);
        let tail = pull(&mut p, 1_000);
        assert!(max_step(&tail) < 0.05 && tail[900..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn takes_stay_sample_aligned_through_a_switch() {
        // Two identical takes: switching between them must be inaudible
        // (bit-exact, as the ramps sum to one).
        let frames = 48_000;
        let a = tone(300.0, 0.5, frames);
        let t = Transport::new(frames);
        let mut p = Player::new(vec![(a.clone(), 1.0), (a.clone(), 1.0)], t.clone(), RATE, 256);
        t.set_playing(true);
        let mut out = pull(&mut p, 2_000);
        t.select(1);
        out.extend(pull(&mut p, 2_000));
        for (i, v) in out.iter().enumerate().skip(600) {
            assert!((v - a[0][i]).abs() < 1e-5, "frame {i}");
        }
    }
}

//! Four-flavour saturation: tube, tape, transistor, hard/soft clipper.
//!
//! Ported from the JUCE multiband-compressor's SaturationProcessor with
//! the same per-sample math. The `drive_db` knob (0..20 dB) sets a
//! pre-gain; the wet signal then runs through one of four nonlinearities;
//! `mix` blends wet/dry. A 5 Hz high-pass DC blocker on the wet path
//! removes any offset that asymmetric tube saturation can introduce.
//!
//! **Latency:** with oversampling on (the default), every type runs
//! inside a nested 4× linear-phase FIR half-band stack, which delays the
//! wet path by a fixed [`BandSaturator::latency_samples`] samples. The dry
//! reference is routed through a matching compensation delay — including
//! on the bypass / zero-drive early-out — so the latency is CONSTANT
//! regardless of settings (a multiband host can enable saturation on one
//! band without shifting it against the others), and the wet/dry `mix`
//! is phase-coherent at all frequencies. Report the latency to the host.

use stave_dsp::db_to_amp;
use stave_dsp::{
    Biquad, Compensator, EnvelopeFollower, EnvelopeTimes, Oversampler2xFir, Oversampler2xFirInner,
    SmoothedParam, Triode,
};

extern crate alloc;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use stave_dsp::FilterSpec;

/// Cross-channel link for the tube makeup AGC: both
/// channels key their makeup from the MAX of the two channels' input
/// and output envelopes, so correlated stereo material gets identical
/// L/R gain instead of independently wandering image. One-sample
/// staleness on the cross read (channels tick sequentially) is far
/// inside the 15/80 ms envelope constants.
#[derive(Default)]
pub struct StereoMakeupLink {
    cur: [[AtomicU32; 2]; 2], // [channel][ie, oe]
    prev: [[AtomicU32; 2]; 2],
    frame: [AtomicU32; 2],
}

impl StereoMakeupLink {
    fn publish(&self, ch: usize, frame: u32, ie: f32, oe: f32) {
        let c = ch & 1;
        self.prev[c][0].store(self.cur[c][0].load(Ordering::Relaxed), Ordering::Relaxed);
        self.prev[c][1].store(self.cur[c][1].load(Ordering::Relaxed), Ordering::Relaxed);
        self.cur[c][0].store(ie.to_bits(), Ordering::Relaxed);
        self.cur[c][1].store(oe.to_bits(), Ordering::Relaxed);
        self.frame[c].store(frame, Ordering::Relaxed);
    }
    /// The other channel's envelopes at frame `my_frame − 1`, whether
    /// it has ticked this frame yet or not — symmetric staleness, so
    /// linked channels compute IDENTICAL maxes on identical input.
    fn other(&self, ch: usize, my_frame: u32) -> (f32, f32) {
        let o = (ch & 1) ^ 1;
        let slot = if self.frame[o].load(Ordering::Relaxed) == my_frame {
            &self.prev[o]
        } else {
            &self.cur[o]
        };
        (
            f32::from_bits(slot[0].load(Ordering::Relaxed)),
            f32::from_bits(slot[1].load(Ordering::Relaxed)),
        )
    }
}

/// Round-trip group delay of the nested 4× stack at the base rate:
/// 66 (outer, 133-tap) + 18 (relaxed 73-tap inner) — both exactly
/// integer by the half-band designs.
const OS4X_LATENCY: usize =
    Oversampler2xFir::latency_samples() + Oversampler2xFirInner::latency_samples_base();

/// Re-engage fade length (~5 ms at 48 k) — see `BandSaturator::front`.
const PRIME_FADE: u16 = 240;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaturationCharacter {
    Tube = 0,
    Tape = 1,
    Transistor = 2,
    Clipper = 3,
}

pub const SATURATIONS: &[&str] = &["TUBE", "TAPE", "XSTR", "CLIP"];

impl stave_core::params::ParamEnum for SaturationCharacter {
    const COUNT: u32 = 4;
    fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Tube,
            1 => Self::Tape,
            2 => Self::Transistor,
            _ => Self::Clipper,
        }
    }
    fn to_index(self) -> u32 {
        self as u32
    }
    fn name(i: u32) -> &'static str {
        SATURATIONS[Self::from_index(i) as usize]
    }
}

/// Per-channel saturation state. Caller is responsible for processing
/// each channel through its own instance so the DC blocker stays
/// per-channel-coherent.
pub struct BandSaturator {
    drive_db: f32,
    drive_gain: f32,
    mix: f32, // 0..1
    ty: SaturationCharacter,
    bypass: bool,
    dc_blocker: Biquad,
    /// Nested 4× linear-phase FIR half-band stack around the nonlinear
    /// shape: harmonics that would fold back as aliasing are rejected by
    /// ~96 dB before decimation. Uniform 4× for every type keeps the
    /// latency constant across type switches. Disable for cheap mode
    /// (zero latency, aliased).
    oversampler: Oversampler2xFir,
    oversampler_inner: Oversampler2xFirInner,
    oversample: bool,
    /// Dry-path compensation for the FIR group delay (see module docs).
    dry_comp: Compensator,
    sample_rate: f32,
    /// Optional pre-shape filter applied before the nonlinearity.
    /// Currently used by TAPE for the "head bump" LF resonance.
    pre_shape: Biquad,
    use_pre_shape: bool,
    /// Optional post-shape filter applied after the nonlinearity.
    /// Currently used by TAPE for drive-dependent HF rolloff
    /// (the magnetic medium's limited high-freq headroom).
    post_shape: Biquad,
    use_post_shape: bool,
    /// Physical 12AX7 triode stage used for the TUBE type, plus a
    /// level-following makeup that keeps the saturated output at the
    /// input's level (the triode itself only normalises small-signal
    /// gain; hard clipping loses level, and that loss is level-dependent
    /// so it must be restored by tracking the actual envelopes). This is
    /// what makes "drive" read as audible grit rather than a volume drop.
    triode: Triode,
    tube_in_env: EnvelopeFollower,
    tube_out_env: EnvelopeFollower,
    tube_makeup: f32,
    /// Rate-derived makeup coefficient (τ ≈ 8.5 ms — matches the old
    /// fixed 0.05/sample feel at 48 k, now invariant across rates).
    makeup_coeff: f32,
    /// Setter smoothing: drive and mix glide instead of stepping.
    s_drive_db: SmoothedParam,
    s_mix: SmoothedParam,
    drive_now_cache: f32,
    shaper_drive_db: f32,
    /// Re-engage priming: after the gate has parked the wet path, its
    /// oversampler rings hold STALE signal; on re-engage we run the
    /// wet path for this many samples (state flows, rings flush) while
    /// still outputting dry — then fade in. Without this, the stale
    /// content crossed the decimator at full wet (measured 0.78 step).
    prime: u16,
    link: Option<(Arc<StereoMakeupLink>, usize)>,
    frame: u32,
    blk: BlkScratch,
}

impl Default for BandSaturator {
    fn default() -> Self {
        Self::new()
    }
}

impl BandSaturator {
    pub fn new() -> Self {
        Self {
            drive_db: 0.0,
            drive_gain: 1.0,
            mix: 0.5,
            ty: SaturationCharacter::Tube,
            bypass: false,
            dc_blocker: Biquad::default(),
            oversampler: Oversampler2xFir::new(),
            oversampler_inner: Oversampler2xFirInner::new(),
            oversample: true,
            dry_comp: Compensator::new(),
            sample_rate: 48_000.0,
            pre_shape: Biquad::default(),
            use_pre_shape: false,
            post_shape: Biquad::default(),
            use_post_shape: false,
            triode: Triode::new(),
            tube_in_env: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 15.0,
                release_ms: 80.0,
                slow_release_ms: None,
            }),
            tube_out_env: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 15.0,
                release_ms: 80.0,
                slow_release_ms: None,
            }),
            tube_makeup: 1.0,
            makeup_coeff: 0.05,
            s_drive_db: SmoothedParam::new(0.0),
            s_mix: SmoothedParam::new(0.5),
            drive_now_cache: 0.0,
            shaper_drive_db: 0.0,
            prime: 0,
            link: None,
            frame: 0,
            blk: BlkScratch::default(),
        }
    }

    /// Stereo-link the tube makeup AGC: both channels of a pair share
    /// `link`, with `channel` = 0 or 1.
    pub fn set_stereo_link(&mut self, link: Arc<StereoMakeupLink>, channel: usize) {
        self.link = Some((link, channel));
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.dc_blocker
            .set(FilterSpec::Highpass { hz: 5.0, q: 0.707 }, sample_rate);
        self.dc_blocker.reset();
        // The triode runs INSIDE the 4× stack, so its stateful cathode
        // dynamics must be prepared at the rate it actually ticks.
        self.triode.prepare(self.nonlinear_rate());
        self.triode.set_grid_drive(drive_volts(self.drive_db));
        // Slow envelopes (~15 ms attack / 80 ms release) so the makeup is
        // a smooth gain that restores saturation loss without pumping.
        // These run at BASE rate (outside the oversampler).
        self.tube_in_env.prepare(sample_rate);
        self.tube_out_env.prepare(sample_rate);
        self.tube_in_env.reset();
        self.tube_out_env.reset();
        self.tube_makeup = 1.0;
        // τ = 8.5 ms, rate-derived: 1 − e^(−1/(τ·fs)).
        self.makeup_coeff = 1.0 - (-1.0f32 / (0.0085 * sample_rate)).exp();
        self.s_drive_db.prepare(sample_rate, 0.005);
        self.s_drive_db.snap_to(self.drive_db);
        self.s_mix.prepare(sample_rate, 0.005);
        self.s_mix.snap_to(self.mix);
        self.drive_now_cache = self.drive_db;
        self.shaper_drive_db = self.drive_db;
        self.oversampler.prepare(sample_rate);
        // Inner oversampler sees the outer's already-2× rate as its
        // outer rate, giving 4× combined.
        self.oversampler_inner.prepare(sample_rate * 2.0);
        self.dry_comp.prepare(OS4X_LATENCY as u32);
        self.configure_shapers();
    }

    /// The rate the nonlinearity itself ticks at: the nested rate
    /// region's typed factors, never a loose literal.
    const OS_FACTOR: usize = Oversampler2xFir::FACTOR * Oversampler2xFirInner::FACTOR;

    fn nonlinear_rate(&self) -> f32 {
        if self.oversample {
            self.sample_rate * Self::OS_FACTOR as f32
        } else {
            self.sample_rate
        }
    }

    /// Wet-path group delay in samples at the base rate (0 with
    /// oversampling disabled). Constant across type/drive/mix/bypass —
    /// hosts should report this once.
    pub fn latency_samples(&self) -> u32 {
        if self.oversample {
            OS4X_LATENCY as u32
        } else {
            0
        }
    }

    /// Configure the optional pre/post shapers based on the current
    /// type + drive. Called from set_type, set_drive_db, and prepare.
    fn configure_shapers(&mut self) {
        match self.ty {
            SaturationCharacter::Tape => {
                // Head bump: low-shelf +3 dB at 80 Hz. Drive-modulated
                // — pushes harder when the user dials in more drive.
                let bump_db = 2.5 + 0.15 * self.drive_db;
                self.pre_shape.set(
                    FilterSpec::LowShelf {
                        hz: 80.0,
                        db: bump_db,
                    },
                    self.sample_rate,
                );
                self.pre_shape.reset();
                self.use_pre_shape = true;
                // HF rolloff: high-shelf that pulls down 8 kHz+ as
                // drive climbs. Heavier compression → less treble,
                // matching how oxide loses HF headroom under heavy
                // bias.
                let rolloff_db = -0.5 - 0.25 * self.drive_db;
                self.post_shape.set(
                    FilterSpec::HighShelf {
                        hz: 7_500.0,
                        db: rolloff_db,
                    },
                    self.sample_rate,
                );
                self.post_shape.reset();
                self.use_post_shape = true;
            }
            _ => {
                self.use_pre_shape = false;
                self.use_post_shape = false;
            }
        }
    }

    /// TAPE shaper coefficients following the SMOOTHED drive: same
    /// formulas as configure_shapers but no state reset (small steps at
    /// ≤ 0.25 dB granularity — a reset per step would click). The
    /// zipper instrument measured this cadence adds NOTHING over the
    /// drive-matched static floor (saturator_zipper.rs — 0.1 dB steps
    /// tried, identical spray, so 0.25 stays).
    fn follow_shapers(&mut self, drive_db: f32) {
        self.shaper_drive_db = drive_db;
        if self.ty == SaturationCharacter::Tape {
            let bump_db = 2.5 + 0.15 * drive_db;
            self.pre_shape.set(
                FilterSpec::LowShelf {
                    hz: 80.0,
                    db: bump_db,
                },
                self.sample_rate,
            );
            let rolloff_db = -0.5 - 0.25 * drive_db;
            self.post_shape.set(
                FilterSpec::HighShelf {
                    hz: 7_500.0,
                    db: rolloff_db,
                },
                self.sample_rate,
            );
        }
    }

    /// Toggle oversampling (changes latency — re-report to the host).
    /// Re-prepares the triode so its cathode dynamics track the rate the
    /// nonlinearity actually ticks at.
    pub fn set_oversample(&mut self, enabled: bool) {
        if enabled != self.oversample {
            self.oversample = enabled;
            self.triode.prepare(self.nonlinear_rate());
            self.triode.set_grid_drive(drive_volts(self.drive_db));
        }
    }

    pub fn set_drive_db(&mut self, drive_db: f32) {
        self.drive_db = drive_db.clamp(0.0, 24.0);
        // the audible gain path GLIDES to the target per sample in
        // process(); only the target changes here.
        self.s_drive_db.set_target(self.drive_db);
        // The triode maps drive to grid swing internally (it must NOT
        // also see the linear pre-gain), so feed it the dB directly —
        // its own cathode physics smooth the transition.
        self.triode.set_grid_drive(drive_volts(self.drive_db));
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
        self.s_mix.set_target(self.mix);
    }

    pub fn set_type(&mut self, ty: SaturationCharacter) {
        // Settle the triode's operating point when it becomes active so
        // the first samples aren't a DC-tracker transient.
        if ty == SaturationCharacter::Tube && self.ty != SaturationCharacter::Tube {
            self.triode.reset();
        }
        self.ty = ty;
        self.shaper_drive_db = self.drive_db;
        self.configure_shapers();
    }

    pub fn set_bypass(&mut self, b: bool) {
        self.bypass = b;
    }

    /// The per-sample FRONT half, shared verbatim by `process` and
    /// `process_block` (the block path must replay the exact same state
    /// sequence — bit-exactness is pinned in `tests/block_api.rs`):
    /// dry reference, control smoothing, engagement gate, prime
    /// bookkeeping. Returns `(dry, None)` when the wet path is parked
    /// for this sample, else `(dry, Some((wet_amt, priming)))`.
    #[inline]
    fn front(&mut self, input: f32) -> (f32, Option<(f32, bool)>) {
        // Constant-latency dry reference: ALWAYS routed through the
        // compensation delay (including the early-outs below), so timing
        // never depends on drive/type/bypass and the wet/dry mix is
        // phase-coherent with the FIR stack's group delay.
        let dry = if self.oversample {
            self.dry_comp.tick(input)
        } else {
            input
        };
        // Smoothed control values — the audible path never steps.
        let drive_now = self.s_drive_db.tick();
        let mix_now = self.s_mix.tick();
        if (drive_now - self.drive_now_cache).abs() > 1e-3 {
            self.drive_now_cache = drive_now;
            self.drive_gain = db_to_amp(drive_now);
            if self.ty == SaturationCharacter::Tape
                && (drive_now - self.shaper_drive_db).abs() > 0.25
            {
                self.follow_shapers(drive_now);
            }
        }
        // the wet path ENGAGES continuously over the first 0.5 dB
        // of drive (smoothstep), replacing the old hard swap at 0 dB —
        // automation across the gate can no longer click.
        let e = (drive_now / 0.5).clamp(0.0, 1.0);
        let wet_amt = mix_now * (e * e * (3.0 - 2.0 * e));
        // Re-engage choreography: FLUSH (wet runs, dry out) then FADE
        // (wet blends in from zero) — both fully click-free.
        let prime_flush: u16 = (OS4X_LATENCY + 8) as u16;
        if self.bypass || wet_amt <= 1e-4 {
            self.prime = prime_flush + PRIME_FADE;
            return (dry, None);
        }
        let priming = self.prime > PRIME_FADE;
        let mut wet_amt = wet_amt;
        if self.prime > 0 {
            self.prime -= 1;
            if !priming {
                // Fade leg: 1 at prime==0, 0 at prime==PRIME_FADE.
                let t = 1.0 - f32::from(self.prime) / f32::from(PRIME_FADE);
                wet_amt *= t * t * (3.0 - 2.0 * t);
            }
        }
        (dry, Some((wet_amt, priming)))
    }

    /// Process one sample. Returns the wet+dry mix, delayed by
    /// [`BandSaturator::latency_samples`] regardless of settings.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let (dry, gate) = self.front(input);
        let Some((wet_amt, priming)) = gate else {
            return dry;
        };
        // TUBE: the physical triode owns its drive (grid swing) via the
        // load-line solve, so it bypasses the linear pre-gain and the
        // pre/post shapers. It is a memoryless steady-state solve, so it
        // DOES need the external oversampler for anti-aliasing of the
        // cutoff/conduction harmonics — run it inside it.
        if self.ty == SaturationCharacter::Tube {
            let nonlinear = if self.oversample {
                let Self {
                    oversampler,
                    oversampler_inner,
                    triode,
                    ..
                } = self;
                oversampler.process_one(input, |a| {
                    oversampler_inner.process_one(a, |b| triode.process_sample(b))
                })
            } else {
                self.triode.process_sample(input)
            };
            return self.tube_post(dry, nonlinear, wet_amt, priming);
        }
        let driven = input * self.drive_gain;
        // Optional pre-shape (TAPE head bump runs here so the LF
        // resonance hits the nonlinearity, not just the dry signal).
        let pre_shaped = if self.use_pre_shape {
            self.pre_shape.process(driven)
        } else {
            driven
        };
        let ty = self.ty;
        let drive_gain = self.drive_gain;
        // shape applied at the oversampled rate
        let shape = move |x: f32| -> f32 {
            match ty {
                // Tube exits via the physical-triode path above; this
                // arm is structurally unreachable (F-list dead code) —
                // loud in debug if a refactor ever un-deads it.
                SaturationCharacter::Tube => {
                    debug_assert!(false, "Tube is handled by the triode path");
                    tube(x)
                }
                SaturationCharacter::Tape => tape(x),
                SaturationCharacter::Transistor => transistor(x, drive_gain),
                SaturationCharacter::Clipper => clipper(x, drive_gain),
            }
        };
        let nonlinear = if self.oversample {
            // Uniform nested 4×: constant latency across type switches,
            // and enough rejection even for CLIP's hard discontinuity.
            let Self {
                oversampler,
                oversampler_inner,
                ..
            } = self;
            oversampler.process_one(pre_shaped, |x2| oversampler_inner.process_one(x2, &shape))
        } else {
            shape(pre_shaped)
        };
        // Optional post-shape (TAPE HF rolloff under drive).
        let shaped = if self.use_post_shape {
            self.post_shape.process(nonlinear)
        } else {
            nonlinear
        };
        // DC-block the wet leg only.
        let wet = self.dc_blocker.process(shaped);
        if priming {
            return dry;
        }
        dry * (1.0 - wet_amt) + wet * wet_amt
    }

    /// The per-sample tube TAIL, shared verbatim by `process` and
    /// `process_block`. Level-following makeup: track input vs
    /// raw-output envelopes and restore the (level-dependent) RMS the
    /// clipping removed, so heavier drive adds harmonics at constant
    /// level instead of ducking the band. Clamped to ±12 dB so it can't
    /// run away on near-silence. Keyed from the DELAYED dry so both
    /// envelopes see time-aligned material.
    #[inline]
    fn tube_post(&mut self, dry: f32, nonlinear: f32, wet_amt: f32, priming: bool) -> f32 {
        let ie_own = self.tube_in_env.tick(dry.abs());
        let oe_own = self.tube_out_env.tick(nonlinear.abs());
        // stereo-linked sense (max of both channels) keeps the
        // image still on correlated material; τ is rate-derived.
        self.frame = self.frame.wrapping_add(1);
        let (ie, oe) = if let Some((link, ch)) = &self.link {
            link.publish(*ch, self.frame, ie_own, oe_own);
            let (i2, o2) = link.other(*ch, self.frame);
            (ie_own.max(i2), oe_own.max(o2))
        } else {
            (ie_own, oe_own)
        };
        let target = (ie / oe.max(1e-5)).clamp(0.25, 4.0);
        self.tube_makeup += self.makeup_coeff * (target - self.tube_makeup);
        let wet = self.dc_blocker.process(nonlinear * self.tube_makeup);
        if priming {
            return dry;
        }
        dry * (1.0 - wet_amt) + wet * wet_amt
    }

    /// Size the block-mode scratch (message thread — allocates). Without
    /// this, `process_block` falls back to the per-sample loop rather
    /// than allocate on the audio thread.
    pub fn prepare_block(&mut self, max_block: usize) {
        self.blk.dry.resize(max_block, 0.0);
        self.blk.wet.resize(max_block, 0.0);
        self.blk.flags.resize(max_block, 0);
        self.blk.up2.resize(2 * max_block, 0.0);
        self.blk.up4.resize(4 * max_block, 0.0);
    }

    /// Block form of [`process`](Self::process) — BIT-identical to the
    /// sample loop (pinned in `tests/block_api.rs`): the front/tail
    /// halves are the same per-sample functions in the same order; only
    /// the oversampled nonlinearity runs as a block, and the FIR block
    /// paths are bit-exact by construction. Gate/priming transitions
    /// partition the block into runs, so the wet state advances for
    /// exactly the samples it would have per-sample. Currently the TUBE
    /// path (where the end-to-end cost bar lives); other types keep the
    /// per-sample loop.
    pub fn process_block(&mut self, io: &mut [f32]) {
        let n = io.len();
        // Stereo-linked pairs stay per-sample: StereoMakeupLink's symmetric n−1
        // staleness contract assumes L/R interleave; channel-sequential
        // blocks would skew it (fresh one way, block-stale the other).
        if self.ty != SaturationCharacter::Tube
            || !self.oversample
            || self.link.is_some()
            || self.blk.dry.len() < n
        {
            for s in io.iter_mut() {
                *s = self.process(*s);
            }
            return;
        }
        for (i, &x) in io.iter().enumerate() {
            let (dry, gate) = self.front(x);
            self.blk.dry[i] = dry;
            match gate {
                Some((w, p)) => {
                    self.blk.wet[i] = w;
                    self.blk.flags[i] = if p { 2 } else { 1 };
                }
                None => self.blk.flags[i] = 0,
            }
        }
        let mut i = 0;
        while i < n {
            if self.blk.flags[i] == 0 {
                io[i] = self.blk.dry[i];
                i += 1;
                continue;
            }
            let start = i;
            while i < n && self.blk.flags[i] != 0 {
                i += 1;
            }
            // Active run: nested 4× block, triode ticked over the 4× slice.
            {
                let Self {
                    oversampler,
                    oversampler_inner,
                    triode,
                    blk,
                    ..
                } = self;
                oversampler.process_block(&mut io[start..i], &mut blk.up2, |b2| {
                    oversampler_inner.process_block(b2, &mut blk.up4, |b4| {
                        for s in b4.iter_mut() {
                            *s = triode.process_sample(*s);
                        }
                    });
                });
            }
            for (k, s) in io[start..i].iter_mut().enumerate() {
                let (d, w, p) = (
                    self.blk.dry[start + k],
                    self.blk.wet[start + k],
                    self.blk.flags[start + k] == 2,
                );
                *s = self.tube_post(d, *s, w, p);
            }
        }
    }
}

/// Scratch for `BandSaturator::process_block` — sized by `prepare_block`,
/// never touched by the sample path.
#[derive(Default)]
struct BlkScratch {
    dry: Vec<f32>,
    wet: Vec<f32>,
    flags: Vec<u8>,
    up2: Vec<f32>,
    up4: Vec<f32>,
}

/// Biased-tanh tube model. A real single-tube preamp stage operates
/// at a bias point that sits on the curved part of the plate-current
/// transfer function, so positive and negative excursions ride
/// different slopes — this is the physical source of strong 2nd-
/// harmonic content.
///
/// We model that directly: shift the input by `BIAS`, run through
/// tanh, then subtract `tanh(BIAS)` so the rest position is centred
/// (the upstream DC blocker mops up what's left). BIAS = 0.3 was
/// tuned by the profiler — it lifts h2 from -55 dB → -25 dB at
/// 10 dB drive (matching a real 12AX7 preamp stage's measured
/// harmonic content), while keeping the overall character musical
/// rather than woofy.
#[inline]
fn tube(x: f32) -> f32 {
    // Larger bias + softer pre-shape so the asymmetric bias dominates
    // over symmetric clipping at typical input levels. Tuned by the
    // profiler: BIAS = 0.45 gives h2 > h3 (tube-correct ordering) at
    // 10 dB drive while keeping output gain ≈ unity.
    const BIAS: f32 = 0.45;
    const BIAS_TANH: f32 = 0.421_899_02; // tanh(0.45)
    let driven = x + BIAS;
    (driven.tanh() - BIAS_TANH) * 1.18
}

/// Soft-knee compression-style nonlinearity above a 0.7 threshold.
#[inline]
fn tape(x: f32) -> f32 {
    const THRESH: f32 = 0.7;
    let a = x.abs();
    if a < THRESH {
        x
    } else {
        let excess = a - THRESH;
        // Keep the divisor away from zero — when drive is very small
        // the soft-knee math collapses; clamp.
        let denom = (2.0 * x.abs().max(1.0)).max(1e-3);
        let compressed = THRESH + excess * (1.0 - excess / denom);
        if x >= 0.0 {
            compressed
        } else {
            -compressed
        }
    }
}

/// Exponential saturation — transistor-style.
#[inline]
fn transistor(x: f32, drive: f32) -> f32 {
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    sign * (1.0 - (-x.abs() * drive).exp())
}

/// The drive knob's taper: 0–20 dB onto grid volts per unit of input,
/// an anchored exponential — clean at 0 (0.4 V), saturation audible by
/// ~20 % of the knob, extreme at the top (110 V).
fn drive_volts(drive_db: f32) -> f64 {
    const MAX_DB: f64 = 20.0;
    const CLEAN_VOLTS: f64 = 0.4;
    const SPAN_VOLTS: f64 = 110.0;
    const CURVE: f64 = 1.5;
    let position = (f64::from(drive_db) / MAX_DB).clamp(0.0, 1.0);
    CLEAN_VOLTS + SPAN_VOLTS * (10f64.powf(CURVE * position) - 1.0) / (10f64.powf(CURVE) - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
        let n = buf.len();
        let w = 2.0 * core::f64::consts::PI * f / fs;
        let cw = w.cos();
        let coeff = 2.0 * cw;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let re = s1 - s2 * cw;
        let im = s2 * w.sin();
        (re * re + im * im).sqrt() / (n as f64 * 0.5)
    }

    fn rms(buf: &[f32]) -> f64 {
        (buf.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / buf.len() as f64).sqrt()
    }

    fn run(sat: &mut BandSaturator, f0: f64, fs: f64, amp: f32, n: usize) -> Vec<f32> {
        // Warm up so the DC blocker settles.
        for i in 0..n {
            let s = amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            let _ = sat.process(s);
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let s = amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            out[i] = sat.process(s);
        }
        out
    }

    #[test]
    #[ignore = "report: prints CPU per instance (wall-clock timing); run with --run-ignored only --no-capture"]
    fn tube_cpu_and_proof_of_work() {
        // Proves the tube path is actually executing (output is saturated,
        // not a passthrough) and measures its real cost. Run in RELEASE:
        //   cargo test -p stave-dsp tube_cpu_and_proof_of_work --release -- --ignored --nocapture
        use std::time::Instant;
        let fs = 48_000.0;
        let f0 = 1_000.0;
        let n = 2_000_000usize;

        let mut sat = BandSaturator::new();
        sat.prepare(fs as f32);
        sat.set_type(SaturationCharacter::Tube);
        sat.set_mix(1.0);
        sat.set_drive_db(12.0);

        // Proof of work: compare to a passthrough reference (drive 0).
        let mut samples = vec![0.0f32; 4096];
        for i in 0..4096 {
            samples[i] = 0.4 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        }
        for &s in &samples {
            let _ = sat.process(s);
        }
        let mut out = vec![0.0f32; 4096];
        for (i, &s) in samples.iter().enumerate() {
            out[i] = sat.process(s);
        }
        let h1 = goertzel(&out, f0, fs);
        let h2 = goertzel(&out, 2.0 * f0, fs);
        let thd = h2 / h1;
        assert!(thd > 0.02, "tube path is a no-op? THD only {thd}");
        println!(
            "proof-of-work: 2nd-harmonic / fundamental = {:.1}% (saturating ✓)",
            thd * 100.0
        );

        // Timing: process N samples, black-box the result so it isn't
        // optimized away.
        let mut acc = 0.0f32;
        let t0 = Instant::now();
        for i in 0..n {
            let s = 0.4 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            acc += sat.process(s);
        }
        let dt = t0.elapsed();
        std::hint::black_box(acc);
        let ns_per = dt.as_nanos() as f64 / n as f64;
        // A stereo 3-band comp with TUBE on every band = 6 of these.
        let realtime_frac = ns_per * 1e-9 * fs * 6.0;
        println!(
            "tube path: {:.1} ns/sample/instance | 6 instances (3-band stereo) = {:.2}% of one core @ {:.0} kHz",
            ns_per, realtime_frac * 100.0, fs / 1000.0
        );
    }

    #[test]
    #[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
    fn tube_level_and_thd_across_bands() {
        // Full BandSaturator tube path (triode + oversampling + level-makeup).
        // Verify: THD rises with drive, output level stays ~constant at
        // ANY band level (no quiet-band blowup, no hot-band ducking).
        let fs = 48_000.0;
        let f0 = 1_000.0;
        let n = 16_384;
        for &amp in &[0.5_f32, 0.3, 0.1, 0.05] {
            for &drive in &[0.0_f32, 10.0, 20.0] {
                let mut sat = BandSaturator::new();
                sat.prepare(fs as f32);
                sat.set_type(SaturationCharacter::Tube);
                sat.set_mix(1.0);
                sat.set_drive_db(drive);
                let out = run(&mut sat, f0, fs, amp, n);
                let h1 = goertzel(&out, f0, fs);
                let thd: f64 = (2..=8)
                    .map(|k| goertzel(&out, k as f64 * f0, fs))
                    .map(|h| h * h)
                    .sum::<f64>()
                    .sqrt()
                    / h1.max(1e-9);
                let in_rms = amp as f64 / core::f64::consts::SQRT_2;
                println!(
                    "amp {:.2} drive {:>4.0} | THD {:5.1}%  level {:+.1} dB",
                    amp,
                    drive,
                    thd * 100.0,
                    20.0 * (rms(&out) / in_rms).log10()
                );
            }
        }
    }

    #[test]
    fn tube_path_is_bounded_dc_free_and_colours_with_drive() {
        let fs = 48_000.0;
        let mut sat = BandSaturator::new();
        sat.prepare(fs as f32);
        sat.set_type(SaturationCharacter::Tube);
        sat.set_mix(1.0);

        let mut last_thd = 0.0;
        for &drive in &[3.0_f32, 12.0, 24.0] {
            sat.set_drive_db(drive);
            let out = run(&mut sat, 1_000.0, fs, 0.5, 16_384);
            assert!(out.iter().all(|x| x.is_finite()));
            assert!(out.iter().fold(0.0f32, |m, &x| m.max(x.abs())) < 4.0);
            // DC blocked: mean ≈ 0.
            let mean = out.iter().map(|&x| x as f64).sum::<f64>() / out.len() as f64;
            assert!(mean.abs() < 1e-3, "residual DC {mean} at {drive} dB");
            // Total harmonic distortion across h2..h7 (the tube shifts
            // from 2nd-dominant to odd/square at extreme drive, so any
            // single harmonic is non-monotonic — total energy is not).
            let h1 = goertzel(&out, 1_000.0, fs);
            let harm: f64 = (2..=7)
                .map(|k| goertzel(&out, k as f64 * 1_000.0, fs))
                .map(|h| h * h)
                .sum::<f64>()
                .sqrt();
            let thd = harm / h1;
            assert!(
                thd > last_thd,
                "total THD should grow with drive ({thd} !> {last_thd})"
            );
            last_thd = thd;
        }
    }

    #[test]
    fn oversampling_suppresses_aliasing() {
        // 5 kHz fundamental: harmonics 5/10/15/20 kHz; alias products land
        // at non-harmonic bins (e.g. 3 kHz from the 45 kHz image). Driven
        // hard, oversampling must hold that alias bin far below the path
        // without it.
        let fs = 48_000.0;
        let f0 = 5_000.0;
        let alias_bin = 3_000.0; // not a harmonic of 5 kHz
        let n = 16_384;

        let mut os = BandSaturator::new();
        os.prepare(fs as f32);
        os.set_type(SaturationCharacter::Tube);
        os.set_mix(1.0);
        os.set_drive_db(24.0);
        os.set_oversample(true);
        let out_os = run(&mut os, f0, fs, 0.6, n);

        let mut no = BandSaturator::new();
        no.prepare(fs as f32);
        no.set_type(SaturationCharacter::Tube);
        no.set_mix(1.0);
        no.set_drive_db(24.0);
        no.set_oversample(false);
        let out_no = run(&mut no, f0, fs, 0.6, n);

        let a_os = goertzel(&out_os, alias_bin, fs);
        let a_no = goertzel(&out_no, alias_bin, fs);
        println!(
            "alias@3k: oversampled {:.2e}  raw {:.2e}  ({:.1} dB lower)",
            a_os,
            a_no,
            20.0 * (a_no / a_os.max(1e-12)).log10()
        );
        assert!(
            a_os < a_no * 0.5,
            "oversampling should cut aliasing: os={a_os} no={a_no}"
        );
    }
}

/// Hard clip with a small soft-knee at the threshold.
#[inline]
fn clipper(x: f32, drive: f32) -> f32 {
    let threshold = 0.8 / drive.max(1e-3);
    let knee = 0.1;
    let a = x.abs();
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    if a < threshold - knee {
        x
    } else if a < threshold + knee {
        let u = (a - threshold + knee) / (2.0 * knee);
        let soft = threshold - knee + 2.0 * knee * (u - u * u * u / 3.0);
        sign * soft
    } else {
        sign * threshold
    }
}

#[cfg(test)]
mod makeup_and_gate {
    use super::*;

    fn noise(n: usize, mut st: u32) -> alloc::vec::Vec<f32> {
        (0..n)
            .map(|_| {
                st = st.wrapping_mul(1664525).wrapping_add(1013904223);
                ((st >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5
            })
            .collect()
    }

    /// a stereo-linked pair fed IDENTICAL material produces
    /// bit-identical outputs — the makeup can no longer wander the
    /// image on correlated content.
    #[test]
    fn linked_tube_agc_is_stereo_coherent() {
        let link = Arc::new(StereoMakeupLink::default());
        let mut l = BandSaturator::new();
        let mut r = BandSaturator::new();
        for (i, s) in [(&mut l, 0usize), (&mut r, 1)] {
            i.prepare(48_000.0);
            i.set_type(SaturationCharacter::Tube);
            i.set_drive_db(10.0);
            i.set_mix(1.0);
            i.set_stereo_link(link.clone(), s);
        }
        // The cross-read is one sample stale on one side (channels tick
        // sequentially), so bit-exactness is unattainable by design;
        // the IMAGE bar is relative gain within 1e-4 (~0.001 dB).
        for (n, &x) in noise(24_000, 3).iter().enumerate() {
            let a = l.process(x);
            let b = r.process(x);
            // Frame-tagged link ⇒ symmetric staleness ⇒ identical
            // maxes ⇒ BIT-identical makeup on identical input.
            assert_eq!(a.to_bits(), b.to_bits(), "L/R diverge at sample {n}");
        }
    }

    /// the makeup time constant is rate-derived — the same
    /// program settles to the same makeup at 48 k and 96 k.
    #[test]
    fn makeup_settles_rate_invariantly() {
        let settle = |fs: f32| -> f32 {
            let mut s = BandSaturator::new();
            s.prepare(fs);
            s.set_type(SaturationCharacter::Tube);
            s.set_drive_db(12.0);
            s.set_mix(1.0);
            let n = noise((fs * 0.6) as usize, 5);
            for &x in &n {
                s.process(x);
            }
            s.tube_makeup
        };
        let a = settle(48_000.0);
        let b = settle(96_000.0);
        assert!(
            (a - b).abs() < 0.08 * a.max(b),
            "makeup rate-dependent: 48k={a:.4} vs 96k={b:.4}"
        );
    }

    /// automating drive across the old 0 dB gate no longer
    /// steps — the engagement fade + setter smoothing bound the
    /// per-sample output delta to glide scale.
    #[test]
    fn drive_gate_crossing_is_click_free() {
        let mut s = BandSaturator::new();
        s.prepare(48_000.0);
        s.set_type(SaturationCharacter::Clipper);
        s.set_mix(1.0);
        s.set_drive_db(6.0);
        // steady tone, settle
        let tone = |i: usize| (core::f32::consts::TAU * 997.0 * i as f32 / 48_000.0).sin() * 0.5;
        let mut out = alloc::vec::Vec::new();
        for i in 0..4_800 {
            out.push(s.process(tone(i)));
        }
        s.set_drive_db(0.0); // across the gate
        for i in 4_800..14_400 {
            out.push(s.process(tone(i)));
        }
        s.set_drive_db(6.0); // and back
        for i in 14_400..24_000 {
            out.push(s.process(tone(i)));
        }
        let step = |w: &[f32]| (w[1] - w[0]).abs();
        // The clipped waveform's own steady-state slope IS ~0.133 —
        // the honest bar compares transition regions against it, not
        // against a guessed constant (a hard gate swap measured 0.78).
        let steady = out[1_000..4_700]
            .windows(2)
            .map(step)
            .fold(0.0f32, f32::max);
        let overall = out.windows(2).map(step).fold(0.0f32, f32::max);
        assert!(
            overall <= steady * 1.1,
            "click across the gate: {overall} vs steady-state slope {steady}"
        );
    }

    #[test]
    fn block_processing_is_bit_identical_across_gates() {
        fn ragged_blocks(total: usize, mut state: u32) -> alloc::vec::Vec<usize> {
            let mut blocks = alloc::vec::Vec::new();
            let mut remaining = total;
            while remaining > 0 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let block_size = (1 + (state >> 20) as usize % 192).min(remaining);
                blocks.push(block_size);
                remaining -= block_size;
            }
            blocks
        }

        let sample_rate = 48_000.0;
        let input = noise(48_000, 0x9E37);
        let mut sample_processor = BandSaturator::new();
        let mut block_processor = BandSaturator::new();
        for saturator in [&mut sample_processor, &mut block_processor] {
            saturator.prepare(sample_rate);
            saturator.set_type(SaturationCharacter::Tube);
            saturator.set_mix(0.8);
            saturator.set_drive_db(9.0);
        }
        block_processor.prepare_block(192);

        let drive_at = |block_index: usize| -> f32 {
            match block_index % 24 {
                0..=7 => 9.0,
                8..=11 => 0.0,
                12..=15 => 0.3,
                _ => 12.0,
            }
        };
        let mut block_output = input.clone();
        let mut sample_output = input.clone();
        let mut position = 0;
        for (block_index, block_size) in ragged_blocks(input.len(), 0x2468).into_iter().enumerate()
        {
            let drive_decibels = drive_at(block_index);
            sample_processor.set_drive_db(drive_decibels);
            block_processor.set_drive_db(drive_decibels);
            let bypassed = block_index % 37 == 36;
            sample_processor.set_bypass(bypassed);
            block_processor.set_bypass(bypassed);
            for sample in &mut sample_output[position..position + block_size] {
                *sample = sample_processor.process(*sample);
            }
            block_processor.process_block(&mut block_output[position..position + block_size]);
            position += block_size;
        }
        for (sample_index, (expected, actual)) in
            sample_output.iter().zip(block_output.iter()).enumerate()
        {
            assert_eq!(
                expected.to_bits(),
                actual.to_bits(),
                "sample {sample_index}: {expected} vs {actual}"
            );
        }
    }
}

//! The kernel conformance suite: every form
//! instantiated with probe leaves, every invariant asserted headless.

use core::num::NonZeroU32;
use stave_dsp::{Loop, Oversampled2x, Par2, PerChannel, Serial, Stage};

/// Zero-latency gain leaf.
struct Gain(f32);
impl Stage for Gain {
    fn prepare(&mut self, _r: f32) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32 {
        x * self.0
    }
}

/// Integer-delay leaf with declared latency and tail.
struct Late {
    buf: Vec<f32>,
    idx: usize,
    n: u32,
}
impl Late {
    fn new(n: u32) -> Self {
        Self {
            buf: vec![0.0; n.max(1) as usize],
            idx: 0,
            n,
        }
    }
}
impl Stage for Late {
    fn prepare(&mut self, _r: f32) {}
    fn reset(&mut self) {
        self.buf.fill(0.0);
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.n == 0 {
            return x;
        }
        let y = self.buf[self.idx];
        self.buf[self.idx] = x;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
    fn latency_samples(&self) -> u32 {
        self.n
    }
    fn tail_samples(&self) -> Option<u32> {
        Some(self.n)
    }
}

/// Records the rate it was prepared at (region invariant probe).
struct RateSpy(f32);
impl Stage for RateSpy {
    fn prepare(&mut self, r: f32) {
        self.0 = r;
    }
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32 {
        x
    }
}

#[test]
fn sequence_properties() {
    let mut s = Serial((Gain(2.0), Late::new(3), Gain(0.5)));
    s.prepare(48_000.0);
    assert_eq!(s.latency_samples(), 3, "latency sums");
    assert_eq!(s.tail_samples(), Some(3), "tail sums");
    let mut out = [0.0f32; 8];
    out[0] = 1.0;
    s.process_block(&mut out);
    let mut s2 = Serial((Gain(2.0), Late::new(3), Gain(0.5)));
    s2.prepare(48_000.0);
    for (i, &b) in out.iter().enumerate() {
        assert_eq!(
            s2.tick(if i == 0 { 1.0 } else { 0.0 }),
            b,
            "block ≡ tick at {i}"
        );
    }
    assert_eq!(out[3], 1.0, "2.0 × 0.5 through 3 samples");
}

#[test]
fn branch_properties() {
    // Dry/wet shape: the join is aligned whatever either branch reports.
    let mut p = Par2::new(Gain(1.0), Late::new(7));
    p.prepare(48_000.0);
    assert_eq!(p.latency_samples(), 7, "join reports the slow branch");
    assert_eq!(p.tail_samples(), Some(7), "tail maxes");
    for i in 0..32 {
        let y = p.tick(if i == 0 { 1.0 } else { 0.0 });
        assert_eq!(y, if i == 7 { 2.0 } else { 0.0 }, "aligned join at {i}");
    }
    // Matrix lanes (M/S shape): distinct inputs, aligned outputs.
    let mut p = Par2::new(Late::new(3), Gain(0.5));
    p.prepare(48_000.0);
    for i in 0..16 {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let got = p.tick_pair(x, x);
        assert_eq!(
            got,
            if i == 3 { (1.0, 0.5) } else { (0.0, 0.0) },
            "lane pair at {i}"
        );
    }
}

#[test]
fn loop_properties() {
    // A one-pole y[n] = x[n] + a·y[n−1] built AS the form, checked
    // against its closed-form impulse response a^n.
    let a = 0.5f32;
    let mut lp = Loop::new(Gain(1.0), NonZeroU32::new(1).unwrap());
    lp.prepare(48_000.0);
    // identity node + unit delay = running sum of the impulse: 1,1,1…
    for i in 0..4 {
        assert_eq!(
            lp.tick(if i == 0 { 1.0 } else { 0.0 }),
            1.0,
            "integrator at {i}"
        );
    }
    // The voiced loop: node = ×a applied to (x + fb) gives y = a·x + a·y[n−1]
    // → impulse response a, a², a³ … (geometric, the closed form).
    let mut voiced = Loop::new(Gain(a), NonZeroU32::new(1).unwrap());
    voiced.prepare(48_000.0);
    for i in 0..12 {
        let y = voiced.tick(if i == 0 { 1.0 } else { 0.0 });
        let want = a.powi(i + 1);
        assert!(
            (y - want).abs() < 1e-6,
            "geometric decay at {i}: {y} vs {want}"
        );
    }
    assert_eq!(voiced.tail_samples(), None, "unbounded unless declared");
    assert_eq!(
        Loop::new(Gain(a), NonZeroU32::new(1).unwrap())
            .with_tail_bound(4800)
            .tail_samples(),
        Some(4800),
        "declared bound reported"
    );
    assert_eq!(voiced.latency_samples(), 0, "feed-forward path only");
}

#[test]
fn region_properties() {
    // Rate contract: interior prepared at 2× per nesting level.
    let mut r = Oversampled2x::front(Oversampled2x::nested(RateSpy(0.0)));
    r.prepare(48_000.0);
    assert_eq!(r.inner.inner.0, 192_000.0, "4× at the interior");
    // Latency from structure: the forge arithmetic.
    let r = Oversampled2x::front(Oversampled2x::nested(Gain(1.0)));
    assert_eq!(r.latency_samples(), 84, "66 + 36/2 computed, not pinned");
    assert_eq!(Oversampled2x::front(Gain(1.0)).latency_samples(), 66);
    assert_eq!(Oversampled2x::nested(Gain(1.0)).latency_samples(), 36);
    // Unity passthrough sanity at the join of both presets.
    let mut r = Oversampled2x::front(Gain(1.0));
    r.prepare(48_000.0);
    let mut peak = 0.0f32;
    for i in 0..4096 {
        let x = (0.5 * (2.0 * std::f64::consts::PI * 997.0 * i as f64 / 48_000.0).sin()) as f32;
        peak = peak.max(r.tick(x).abs());
    }
    assert!(
        (peak - 0.5).abs() < 0.01,
        "front region ≈ unity: peak {peak}"
    );
}

#[test]
fn lanes_properties() {
    // Lanes are who the audio is: per-lane state, never mixed.
    let mut lanes: PerChannel<Late, 2> = PerChannel::new(|l| Late::new(l as u32 + 1));
    let (a, b) = lanes.pair_mut();
    for i in 0..8 {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let (ya, yb) = (a.tick(x), b.tick(x));
        assert_eq!(ya, if i == 1 { 1.0 } else { 0.0 });
        assert_eq!(yb, if i == 2 { 1.0 } else { 0.0 });
    }
}

#[test]
fn ms_adapter_is_exact_in_domain() {
    use stave_dsp::{ms_decode, ms_encode};
    // The exactness invariant, domain-qualified: within a 28-octave
    // (≈168 dB) intra-pair span — wider than any audio program — the
    // f64-interior rotation loses nothing and decode∘encode is the
    // identity on f32 bits. A zero lane is always in-domain.
    let in_span = |l: f32, r: f32| -> bool {
        if l == 0.0 || r == 0.0 {
            return true;
        }
        let el = (l.abs().to_bits() >> 23) as i32; // biased; subnormals→0
        let er = (r.abs().to_bits() >> 23) as i32;
        (el - er).abs() <= 28
    };
    let mut lcg: u64 = 0x2545_F491_4F6C_DD1D;
    let mut checked = 0u32;
    while checked < 1_000_000 {
        lcg = lcg
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let l = f32::from_bits((lcg >> 32) as u32);
        let r = f32::from_bits(lcg as u32);
        if !l.is_finite() || !r.is_finite() || l == 0.0 || r == 0.0 {
            continue; // NaN/inf are not audio; zeros pinned below
        }
        let (m, s) = ms_encode(l, r);
        let (l2, r2) = ms_decode(m, s);
        if in_span(l, r) {
            assert_eq!(l.to_bits(), l2.to_bits(), "l {l} vs r {r} → {l2}");
            assert_eq!(r.to_bits(), r2.to_bits(), "r {r} vs l {l} → {r2}");
        } else {
            // Outside the span: the louder lane still round-trips
            // bit-exact; the quieter absorbed at the f64 floor.
            if l.abs() >= r.abs() {
                assert_eq!(l.to_bits(), l2.to_bits());
            } else {
                assert_eq!(r.to_bits(), r2.to_bits());
            }
        }
        checked += 1;
    }
    // Edges the sweep may miss: denormal floor, full-scale extremes
    // (l+r overflows f32 — the f64 interior must carry it), the
    // worst in-span rounding candidates, a zero lane against both
    // extremes (always exact).
    for (l, r) in [
        (f32::MIN_POSITIVE, -f32::MIN_POSITIVE),
        (1.0e-45, 1.0e-45), // subnormals
        (f32::MAX, f32::MAX),
        (f32::MAX, -f32::MAX),
        (1.0, 2.0_f32.powi(-24)),
        (1.0, -(2.0_f32.powi(-28) * 1.5)),
        (f32::MAX, 0.0),
        (0.0, 1.0e-45),
    ] {
        let (m, s) = ms_encode(l, r);
        let (l2, r2) = ms_decode(m, s);
        assert_eq!((l.to_bits(), r.to_bits()), (l2.to_bits(), r2.to_bits()));
    }
    // Signed zero: the documented exception — value-identical, the
    // sign of zero collapses to + (IEEE x−x = +0).
    let (m, s) = ms_encode(-0.0, -0.0);
    let (l2, r2) = ms_decode(m, s);
    assert_eq!(l2, 0.0);
    assert_eq!(r2, 0.0);
    // Downmix coefficients PINNED: mid is the stereo→mono downmix,
    // 0.5/0.5 exactly — correlated program sums to itself, opposite
    // program cancels, one-sided program halves.
    assert_eq!(ms_encode(1.0, 1.0), (1.0, 0.0));
    assert_eq!(ms_encode(1.0, -1.0), (0.0, 1.0));
    assert_eq!(ms_encode(1.0, 0.0), (0.5, 0.5));
    // 5.1 fold-down (ITU-R BS.775), coefficients pinned: fronts at
    // unity, centre/surrounds at exactly 1/√2, LFE omitted.
    use stave_dsp::downmix_51;
    const G: f64 = core::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(downmix_51(1.0, 0.0, 0.0, 0.0, 0.0, 0.0), (1.0, 0.0));
    assert_eq!(downmix_51(0.0, 1.0, 0.0, 0.0, 0.0, 0.0), (0.0, 1.0));
    assert_eq!(downmix_51(0.0, 0.0, 1.0, 0.0, 0.0, 0.0), (G, G));
    assert_eq!(downmix_51(0.0, 0.0, 0.0, 1.0, 0.0, 0.0), (0.0, 0.0));
    assert_eq!(downmix_51(0.0, 0.0, 0.0, 0.0, 1.0, 0.0), (G, 0.0));
    assert_eq!(downmix_51(0.0, 0.0, 0.0, 0.0, 0.0, 1.0), (0.0, G));
}

#[test]
fn control_properties() {
    use stave_dsp::{ControlRoute, Router, Source};
    struct Smooth(f32);
    impl Source for Smooth {
        fn prepare(&mut self, _r: f32) {}
        fn reset(&mut self) {
            self.0 = 0.0;
        }
        fn tick(&mut self, d: f32) -> f32 {
            self.0 += 0.5 * (d.abs() - self.0);
            self.0
        }
        fn state(&self) -> f32 {
            self.0
        }
    }
    // Edges are DATA: rebuildable at block rate, iterable for the
    // sidecar; the target vocabulary is the consumer's.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Inlet {
        Gain,
        Cutoff,
    }
    let mut r: Router<Smooth, Inlet, 2, 4> = Router::new([Smooth(0.0), Smooth(0.0)]);
    r.prepare(48_000.0);
    assert!(r.push_route(ControlRoute {
        source: 0,
        target: Inlet::Gain,
        amount: -0.5
    }));
    assert!(r.push_route(ControlRoute {
        source: 1,
        target: Inlet::Cutoff,
        amount: 1.0
    }));
    let states = r.tick_states(&[1.0, 0.5]);
    assert_eq!(states, [0.5, 0.25], "sources advance from their detectors");
    let wiring: Vec<_> = r.routes().map(|e| (e.source, e.target, e.amount)).collect();
    assert_eq!(
        wiring,
        vec![(0, Inlet::Gain, -0.5), (1, Inlet::Cutoff, 1.0)]
    );
    // Displace-from-base at the inlet: base is the knob's word.
    let base_gain = 0.8f32;
    let modulated = base_gain * (1.0 + wiring[0].2 * states[0]);
    assert!((modulated - 0.6).abs() < 1e-6);
    r.clear_routes();
    assert_eq!(r.routes().count(), 0, "wiring rebuilds at block rate");
}

#[test]
fn param_and_detector_sources_are_one_class() {
    use stave_dsp::{ControlRoute, EnvelopeFollower, EnvelopeTimes, Router, SmoothedParam, Source};
    // Doctrine 3 as structure: the class has two KINDS behind one
    // surface — writer-driven (param ramps) and program-driven
    // (detectors) — so a route from a knob and a route from an
    // envelope are the same data. Mixing kinds in one Router is a
    // consumer sum type (the Driver pattern).
    enum Kind {
        Param(SmoothedParam),
        Det(EnvelopeFollower),
    }
    impl Source for Kind {
        fn prepare(&mut self, r: f32) {
            match self {
                Kind::Param(p) => Source::prepare(p, r),
                Kind::Det(d) => Source::prepare(d, r),
            }
        }
        fn reset(&mut self) {
            match self {
                Kind::Param(p) => Source::reset(p),
                Kind::Det(d) => Source::reset(d),
            }
        }
        fn tick(&mut self, det: f32) -> f32 {
            match self {
                Kind::Param(p) => Source::tick(p, det),
                Kind::Det(d) => Source::tick(d, det),
            }
        }
        fn state(&self) -> f32 {
            match self {
                Kind::Param(p) => Source::state(p),
                Kind::Det(d) => Source::state(d),
            }
        }
    }
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Inlet {
        Depth,
        Gain,
    }
    let mut knob = SmoothedParam::new(0.0);
    knob.prepare(48_000.0, 0.005);
    knob.set_target(1.0); // the block-rate write — the knob's word
    let mut det = EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: 1.0,
        release_ms: 50.0,
        slow_release_ms: None,
    });
    det.prepare(48_000.0);
    let mut r: Router<Kind, Inlet, 2, 4> = Router::new([Kind::Param(knob), Kind::Det(det)]);
    assert!(r.push_route(ControlRoute {
        source: 0,
        target: Inlet::Depth,
        amount: 1.0
    }));
    assert!(r.push_route(ControlRoute {
        source: 1,
        target: Inlet::Gain,
        amount: -0.5
    }));
    // The distinguishing invariant: the param source glides to its WRITTEN
    // target no matter what program detector it is fed; the detector
    // source follows the program. Feed both a hot program.
    let mut states = [0.0f32; 2];
    for _ in 0..2_400 {
        states = r.tick_states(&[123.0, 0.8]);
    }
    assert!(
        (states[0] - 1.0).abs() < 1e-3,
        "param source follows its writer, not the program: {}",
        states[0]
    );
    assert!(
        (states[1] - 0.8).abs() < 1e-2,
        "detector source follows the program: {}",
        states[1]
    );
    // Both edges are the same wiring data (the sidecar's view).
    let wiring: Vec<_> = r.routes().map(|e| (e.source, e.target)).collect();
    assert_eq!(wiring, vec![(0, Inlet::Depth), (1, Inlet::Gain)]);
    // Rest states differ HONESTLY under one reset surface: a param's
    // rest is its target (no fabricated value), a detector's is
    // silence (no fabricated program history).
    r.reset();
    assert_eq!(r.sources[0].state(), 1.0, "param reset completes the glide");
    assert_eq!(
        r.sources[1].state(),
        0.0,
        "detector reset forgets the program"
    );
}

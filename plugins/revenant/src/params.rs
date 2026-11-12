//! Revenant's typed parameter table — the face. Ids are FOREVER —
//! pick once, never reuse or renumber.
//!
//! ```text
//!   ( the organism )            [ SURPRISE ]
//!   LEARN  MEMORY  CHAOS  HORIZON  DREAM  MIX
//!   (bypass)        revenant              (sleep)
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{BoolParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit};

#[derive(Params)]
pub struct RevenantParams {
    /// Output trim (host-side; not on the face).
    pub output: FloatParam,
    /// Left footswitch. The framework gate owns click-free bypass.
    pub bypass: BoolParam,
    /// How fast it learns you (the readout's step size). 0 = it stops
    /// learning and keeps what it knows.
    pub learn: FloatParam,
    /// How long the network remembers: its neurons' leak, from quick
    /// (timbre) to slow (smeared, long-memory).
    pub memory: FloatParam,
    /// The network's gain round its own loops (spectral radius
    /// 0.6 … 1.35). Past the middle it starts to ring on its own.
    pub chaos: FloatParam,
    /// How far ahead it must guess what you will play, ms. Short, it
    /// shadows you; long, it has to imagine.
    pub horizon: FloatParam,
    /// How much it listens to itself instead of you.
    pub dream: FloatParam,
    /// Clean dry ↔ the revenant (bit-exact dry at 0).
    pub mix: FloatParam,
    /// Right footswitch: it stops listening and stops learning — and
    /// plays on from what it has learned, feeding on its own output.
    pub sleep: BoolParam,
}

impl RevenantParams {
    pub fn new() -> Arc<Self> {
        let unit = || Range::new(0.0, 1.0, Taper::Linear);
        Arc::new(Self {
            output: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags { bypass: true, ..Default::default() }),
            learn: FloatParam::new(3, "learn", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Mind")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            memory: FloatParam::new(4, "memory", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Mind")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            chaos: FloatParam::new(5, "chaos", 0.45, unit())
                .with_unit(Unit::Percent)
                .with_group("Mind")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            horizon: FloatParam::new(6, "horizon", 3.0, Range::new(0.25, 25.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Mind"),
            dream: FloatParam::new(7, "dream", 0.0, unit())
                .with_unit(Unit::Percent)
                .with_group("Mind")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            mix: FloatParam::new(8, "mix", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 10.0 }),
            sleep: BoolParam::new(9, "sleep", false)
                .with_group("Mind")
                .with_names("AWAKE", "ASLEEP"),
        })
    }
}

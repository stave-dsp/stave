//! Ribbon's typed parameter table — the face. Ids are FOREVER — pick
//! once, never reuse or renumber.
//!
//! ```text
//!   ┌───────────── the tape window: the loop, the heads ─────────────┐
//!   RATE  INTENSITY  DRIVE  [HEADS]  WEAR  ERASE  SPRING  MIX
//!   (bypass)                 ribbon                        (brake)
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// Which playback heads are live. The heads sit 1 : 2 : 3 down the
    /// tape from the record head.
    pub const HEADS: Heads = ["1", "2", "3", "1+2", "2+3", "1+3", "1+2+3"];
}

#[derive(Params)]
pub struct RibbonParams {
    /// Output trim (host-side; not on the face).
    pub output: FloatParam,
    /// Left footswitch. The framework gate owns click-free bypass.
    pub bypass: BoolParam,
    /// REPEAT RATE — the capstan motor's speed (1 = nominal: heads at
    /// 90 / 180 / 270 ms). The motor has inertia, so turning it bends
    /// the pitch of everything on the tape.
    pub rate: FloatParam,
    /// How much of the heads goes back to the record head. Past ~85%
    /// the loop runs away into self-oscillation.
    pub intensity: FloatParam,
    /// The tube preamp into the record head (12AX7, Koren model).
    pub drive: FloatParam,
    pub heads: EnumParam<Heads>,
    /// The tape's age: wow and flutter, hiss, head-gap loss, dropouts,
    /// and each generation on the loop duller than the last.
    pub wear: FloatParam,
    /// The erase head. 100% wipes the tape before every recording (a
    /// plain echo); lower, the loop keeps what it held and the new pass
    /// records on top (sound-on-sound, a generation older each turn).
    pub erase: FloatParam,
    /// The spring tank's level.
    pub spring: FloatParam,
    /// Clean dry ↔ echoes + spring (bit-exact dry at 0).
    pub mix: FloatParam,
    /// Right footswitch: the motor winds down to a stop and stays there;
    /// release and it winds back up. The loop keeps its recording.
    pub brake: BoolParam,
}

impl RibbonParams {
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
            rate: FloatParam::new(3, "rate", 1.0, Range::new(0.35, 1.6, Taper::Log))
                .with_unit(Unit::Plain { decimals: 2 })
                .with_group("Tape"),
            intensity: FloatParam::new(4, "intensity", 0.4, unit())
                .with_unit(Unit::Percent)
                .with_group("Tape")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            drive: FloatParam::new(5, "drive", 0.25, unit())
                .with_unit(Unit::Percent)
                .with_group("Tape")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            heads: EnumParam::new(6, "heads", Heads(6)).with_group("Tape"),
            wear: FloatParam::new(7, "wear", 0.0, unit())
                .with_unit(Unit::Percent)
                .with_group("Tape")
                .with_smooth(Smooth::Exp { ms: 40.0 }),
            erase: FloatParam::new(8, "erase", 1.0, unit())
                .with_unit(Unit::Percent)
                .with_group("Tape")
                .with_smooth(Smooth::Exp { ms: 40.0 }),
            spring: FloatParam::new(9, "spring", 0.3, unit())
                .with_unit(Unit::Percent)
                .with_group("Spring")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            mix: FloatParam::new(10, "mix", 0.4, unit())
                .with_unit(Unit::Percent)
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 10.0 }),
            brake: BoolParam::new(11, "brake", false)
                .with_group("Tape")
                .with_names("RUN", "BRAKE"),
        })
    }
}

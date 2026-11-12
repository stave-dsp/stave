//! Engram's typed parameter table — the face. Ids are FOREVER — pick
//! once, never reuse or renumber.
//!
//! ```text
//!   [ the display: the memory bank (attention) · hearing · the neurons ]
//!   SIZE DECAY TONE RECALL FOCUS DÉJÀ MATCH TEXTURE PLASTICITY MIX
//!   (bypass)        engram          (hold)
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{BoolParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit};

#[derive(Params)]
pub struct EngramParams {
    /// Output trim (host-side; not on the face).
    pub output: FloatParam,
    /// Left footswitch. The framework gate owns click-free bypass.
    pub bypass: BoolParam,
    /// The room's size: its sixteen lines span 25 ms … 170 ms.
    pub size: FloatParam,
    /// The room's T60, seconds.
    pub decay: FloatParam,
    /// Dark ↔ bright: the lines' damping.
    pub tone: FloatParam,

    /// How loudly the memory answers: the retrieved moments' level
    /// into the room.
    pub recall: FloatParam,
    /// The attention's sharpness (softmax temperature): low, every
    /// memory that is at all alike answers together — a collective
    /// wash; high, only the single best match is recalled.
    pub focus: FloatParam,
    /// The youngest a memory may be to be recalled, seconds — so the
    /// room answers with something from BEFORE, not an echo of now.
    pub deja: FloatParam,
    /// The FORGET button: any press wipes the memory bank.
    pub forget: BoolParam,
    /// What makes two moments ALIKE: 0 = harmony (the notes), 1 =
    /// timbre and groove (the sound and the rhythm). Harmony also
    /// counts only as much as what you play is tonal — drums are
    /// remembered by their sound and pattern whatever MATCH says.
    pub matching: FloatParam,
    /// How memories play back: 0 = seamless (long grains walking
    /// through the moment), the middle = scattered (a cloud), 1 =
    /// shattered (short grains, reversed, leaping octaves).
    pub texture: FloatParam,
    /// Dry ↔ the room (bit-exact dry at 0).
    pub mix: FloatParam,
    /// Right footswitch: the room goes lossless, no new memories are
    /// written and the shape stops moving — everything stays.
    pub hold: BoolParam,
    /// How fast the room learns its own shape: lines that ring
    /// together pull their nodes together (Hebbian). At 0 the nodes
    /// relax back to where you drew them.
    pub plasticity: FloatParam,
    /// The GENERATED room: 0 = the knobs' room (SIZE/DECAY/TONE alone),
    /// 1 = the learned space's own early reflections, per-band decay,
    /// colour and width. DECAY and TONE stay live: around their
    /// defaults they stretch and tilt the generated room.
    pub space: FloatParam,
    /// Where on the MAP of learned spaces (≈700 real reverbs, laid out
    /// by likeness) the room is: between two dots is a room between them.
    pub space_x: FloatParam,
    pub space_y: FloatParam,
    /// How far the memory pulls the room: a recalled moment brings back
    /// the space it was heard in, by the attention it gets.
    pub steer: FloatParam,
    /// The room DREAMS: a slow wander through the learned space, around
    /// where the map and memory put it, into rooms no one recorded.
    pub drift: FloatParam,
    /// The sixteen neurons' drawn positions on the display's field
    /// (0..1 across, 0..1 down). Distance from the centre sets a line's
    /// length; closeness between nodes sets how strongly they couple.
    pub node_0_x: FloatParam,
    pub node_0_y: FloatParam,
    pub node_1_x: FloatParam,
    pub node_1_y: FloatParam,
    pub node_2_x: FloatParam,
    pub node_2_y: FloatParam,
    pub node_3_x: FloatParam,
    pub node_3_y: FloatParam,
    pub node_4_x: FloatParam,
    pub node_4_y: FloatParam,
    pub node_5_x: FloatParam,
    pub node_5_y: FloatParam,
    pub node_6_x: FloatParam,
    pub node_6_y: FloatParam,
    pub node_7_x: FloatParam,
    pub node_7_y: FloatParam,
    pub node_8_x: FloatParam,
    pub node_8_y: FloatParam,
    pub node_9_x: FloatParam,
    pub node_9_y: FloatParam,
    pub node_10_x: FloatParam,
    pub node_10_y: FloatParam,
    pub node_11_x: FloatParam,
    pub node_11_y: FloatParam,
    pub node_12_x: FloatParam,
    pub node_12_y: FloatParam,
    pub node_13_x: FloatParam,
    pub node_13_y: FloatParam,
    pub node_14_x: FloatParam,
    pub node_14_y: FloatParam,
    pub node_15_x: FloatParam,
    pub node_15_y: FloatParam,
}

impl EngramParams {
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
            size: FloatParam::new(3, "size", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Room")
                .with_smooth(Smooth::Exp { ms: 300.0 }),
            decay: FloatParam::new(4, "decay", 3.0, Range::new(0.3, 30.0, Taper::Log))
                .with_unit(Unit::Plain { decimals: 1 })
                .with_group("Room")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            tone: FloatParam::new(5, "tone", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Room")
                .with_smooth(Smooth::Exp { ms: 40.0 }),
            recall: FloatParam::new(7, "recall", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Memory")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            mix: FloatParam::new(8, "mix", 0.35, unit())
                .with_unit(Unit::Percent)
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 10.0 }),
            hold: BoolParam::new(9, "hold", false)
                .with_group("Room")
                .with_names("FREE", "HELD"),
            plasticity: FloatParam::new(10, "plasticity", 0.3, unit())
                .with_unit(Unit::Percent)
                .with_group("Memory")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            space: FloatParam::new(17, "space", 1.0, unit())
                .with_unit(Unit::Percent)
                .with_group("Space")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            space_x: FloatParam::new(18, "space x", 0.5, unit())
                .with_group("Space"),
            space_y: FloatParam::new(19, "space y", 0.5, unit())
                .with_group("Space"),
            steer: FloatParam::new(52, "steer", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Space"),
            drift: FloatParam::new(53, "drift", 0.15, unit())
                .with_unit(Unit::Percent)
                .with_group("Space"),
            // Ids 6 (learn) and 11 (feed) belonged to the retired RLS
            // memory: never reused.
            focus: FloatParam::new(12, "focus", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Memory")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            deja: FloatParam::new(13, "deja", 4.0, Range::new(0.0, 60.0, Taper::Linear))
                .with_unit(Unit::Plain { decimals: 1 })
                .with_group("Memory"),
            forget: BoolParam::new(14, "forget", false)
                .with_group("Memory")
                .with_names("·", "FORGET"),
            matching: FloatParam::new(15, "match", 0.3, unit())
                .with_unit(Unit::Percent)
                .with_group("Memory")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            texture: FloatParam::new(16, "texture", 0.15, unit())
                .with_unit(Unit::Percent)
                .with_group("Memory")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            node_0_x: node(20, "node 1 x", default_node(0).0),
            node_0_y: node(21, "node 1 y", default_node(0).1),
            node_1_x: node(22, "node 2 x", default_node(1).0),
            node_1_y: node(23, "node 2 y", default_node(1).1),
            node_2_x: node(24, "node 3 x", default_node(2).0),
            node_2_y: node(25, "node 3 y", default_node(2).1),
            node_3_x: node(26, "node 4 x", default_node(3).0),
            node_3_y: node(27, "node 4 y", default_node(3).1),
            node_4_x: node(28, "node 5 x", default_node(4).0),
            node_4_y: node(29, "node 5 y", default_node(4).1),
            node_5_x: node(30, "node 6 x", default_node(5).0),
            node_5_y: node(31, "node 6 y", default_node(5).1),
            node_6_x: node(32, "node 7 x", default_node(6).0),
            node_6_y: node(33, "node 7 y", default_node(6).1),
            node_7_x: node(34, "node 8 x", default_node(7).0),
            node_7_y: node(35, "node 8 y", default_node(7).1),
            node_8_x: node(36, "node 9 x", default_node(8).0),
            node_8_y: node(37, "node 9 y", default_node(8).1),
            node_9_x: node(38, "node 10 x", default_node(9).0),
            node_9_y: node(39, "node 10 y", default_node(9).1),
            node_10_x: node(40, "node 11 x", default_node(10).0),
            node_10_y: node(41, "node 11 y", default_node(10).1),
            node_11_x: node(42, "node 12 x", default_node(11).0),
            node_11_y: node(43, "node 12 y", default_node(11).1),
            node_12_x: node(44, "node 13 x", default_node(12).0),
            node_12_y: node(45, "node 13 y", default_node(12).1),
            node_13_x: node(46, "node 14 x", default_node(13).0),
            node_13_y: node(47, "node 14 y", default_node(13).1),
            node_14_x: node(48, "node 15 x", default_node(14).0),
            node_14_y: node(49, "node 15 y", default_node(14).1),
            node_15_x: node(50, "node 16 x", default_node(15).0),
            node_15_y: node(51, "node 16 y", default_node(15).1),
        })
    }
}

/// A node's drawn coordinate: a plain 0..1 param, gliding slowly (a
/// dragged node bends the room smoothly, not in a step).
fn node(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(0.0, 1.0, Taper::Linear))
        .with_unit(Unit::Plain { decimals: 3 })
        .with_group("Shape")
        .with_smooth(Smooth::Exp { ms: 120.0 })
}

/// The factory drawing: the sixteen nodes round an ellipse, at radii
/// that spread the lines' lengths (a well-diffused hall).
pub fn default_node(i: usize) -> (f64, f64) {
    let theta = core::f64::consts::TAU * (i as f64 / 16.0) + 0.21;
    let k = 0.45 + 0.55 * (((i * 7) % 16) as f64 / 15.0);
    (0.5 + 0.44 * k * theta.cos(), 0.5 + 0.40 * k * theta.sin())
}

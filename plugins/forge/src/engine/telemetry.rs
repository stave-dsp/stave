//! Telemetry — everything the engine reports OUT (the framework
//! attaches the bus via `attach_telemetry`; this component is the
//! other end of that seam): declared tap handles, the scope
//! capture, editor-feed caches, and the per-block publish pass.
//! Zero-cost with no bus attached.

extern crate alloc;

use std::time::Instant;

use stave_core::Tap;
use stave_dsp::shapes;

use crate::engine::voicing::{character_shape, stage_b_knee};

use stave_core::{DebugBus, Scope};

use crate::params::ForgeParams;

#[inline]
fn peak_db(peak: f32) -> f64 {
    20.0 * (peak.max(1e-6) as f64).log10()
}

/// Everything the engine reports out, as ONE component: declared
/// tap handles, the framework capture scope, and the editor-feed
/// caches. The engine processes audio; Telemetry publishes what
/// happened. Nothing here is DSP state.
pub(super) struct Telemetry {
    taps: ForgeTaps,
    /// Framework capture scratch + load meter.
    scope: Scope,
    /// Voicing tuple last published (transfer view republish gate).
    last_voicing: (i32, i32, f32, f32),
    /// Screamer input gain (primary is an RC there — no Frame to read).
    sc_gain: f32,
    /// Per-stage block peaks of the mid chain's sense endpoints
    /// (the topology strip's feed).
    tap_peaks: [f32; 6],
}

/// One processed block, as the per-block publish pass reports it.
pub(super) struct BlockReport {
    pub frames: usize,
    pub peak_in: f32,
    pub peak_out: f32,
    pub started: Instant,
}

/// The voicing tuple the static transfer view is drawn from.
pub(super) struct Voicing {
    pub character: usize,
    pub stack: usize,
    pub edge: f32,
    pub bias: f32,
    /// The primary frame's bias.
    pub frame_bias: f32,
    pub drive: f32,
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self {
            taps: ForgeTaps::new(),
            scope: Scope::new(),
            last_voicing: (-1, -1, -1.0, -1.0),
            sc_gain: 1.0,
            tap_peaks: [0.0; 6],
        }
    }
}

impl Telemetry {
    pub(super) fn prepare(&mut self, max_frames: usize) {
        self.scope.prepare(max_frames);
    }
    /// The declared observation points — instruments attach here
    /// (handle in, transport at the wire only).
    pub(crate) fn taps(&self) -> &ForgeTaps {
        &self.taps
    }
    /// Start-of-block reset for the per-stage peak accumulators.
    pub(super) fn begin_block(&mut self) {
        self.tap_peaks = [0.0; 6];
    }
    /// Scope reference for this frame: the LIVE frame's (bias, drive)
    /// on the primary stage — or the screamer's input gain when the
    /// primary is an RC (no Frame to read).
    #[inline]
    pub(super) fn scope_reference(&self, strip: &mut super::strip::Strip) -> (f32, f32) {
        let id_a = strip.amp().primary;
        if let Some(stave_dsp::StageKind::Curve(cs)) =
            strip.amp_mut().as_chain_mut().stage_mut(id_a)
        {
            let fr = *cs.pending_mut();
            (fr.bias, fr.drive)
        } else {
            (0.0, self.sc_gain)
        }
    }
    /// One frame's capture: the scope's transfer pair plus the
    /// topology strip's sense-endpoint peaks (derived from
    /// `topology()`, never an index literal).
    #[inline]
    pub(super) fn capture(&mut self, frame: usize, x: f32, y: f32, strip: &super::strip::Strip) {
        self.scope.write(frame, x, y);
        for (id, _) in strip.amp().as_chain().topology() {
            self.tap_peaks[id.0] = self.tap_peaks[id.0].max(strip.amp().as_chain().sense(id).abs());
        }
    }
}

impl Telemetry {
    /// The per-block publish pass: block peaks, the scope's capture
    /// pair, per-stage topology peaks, the load meter. Entirely
    /// gated on the bus — zero cost unattached.
    pub(super) fn publish(
        &mut self,
        debug: &Option<alloc::sync::Arc<DebugBus>>,
        p: &ForgeParams,
        sr: f32,
        block: BlockReport,
    ) {
        let BlockReport {
            frames,
            peak_in: peak_in_block,
            peak_out: peak_out_block,
            started: t_block,
        } = block;
        if let Some(bus) = debug {
            self.taps.io_in.set(bus, peak_db(peak_in_block));
            self.taps.io_out.set(bus, peak_db(peak_out_block));
            // Engine view: transfer pairs for the phosphor scope…
            self.scope.publish(bus, frames);
            // …and per-stage tap peaks for the topology strip, on
            // STABLE wires (values only — the editor's pill
            // declarations own the labels). Stage 2's pill hides in
            // SINGLE by not publishing (the strip ages it out).
            let t = self.tap_peaks;
            let cur_stack = p.stack.index();
            for (k, tap) in self.taps.stage.iter().enumerate() {
                if k == 2 && cur_stack == 0 {
                    continue;
                }
                tap.set(bus, t[k] as f64);
            }
            if frames > 0 {
                let budget = frames as f64 / sr as f64;
                let pct = self.scope.load_pct(t_block.elapsed().as_secs_f64(), budget);
                self.taps.cpu_pct.set(bus, pct);
            }
        }
    }
}

impl Telemetry {
    /// Static transfer view: the curve stack's skeleton (the voicing
    /// filters and memory bend it dynamically) plus the pad markers
    /// and the scope's amplitude reference. Republished only when the
    /// voicing tuple moves; gated entirely on the debug bus.
    pub(super) fn publish_static_viz(
        &mut self,
        debug: &Option<alloc::sync::Arc<DebugBus>>,
        voicing: Voicing,
    ) {
        let Voicing {
            character,
            stack,
            edge,
            bias,
            frame_bias,
            drive,
        } = voicing;
        // Transfer view: static skeleton of the curve stack (the
        // voicing filters and memory bend it dynamically).
        // (Observability only — gated on the debug bus.)
        if let Some(bus) = debug {
            if (character as i32, stack as i32, edge, bias) != self.last_voicing {
                self.last_voicing = (character as i32, stack as i32, edge, bias);
                let cs_params = character_shape(character, edge as f64);
                let shape_single = cs_params.eval();
                let shape_pp =
                    cs_params.eval_push_pull(0.7 - 0.45 * edge as f64, bias as f64 * 0.8);
                let kb = stage_b_knee(character, edge as f64);
                let post = shapes::knee(if stack == 2 { (1.5 + edge) as f64 } else { kb });
                let bf = frame_bias as f64;
                let span = stave_core::debug::CURVE_X_SPAN;
                let mut ys = [0.0f32; stave_core::debug::CURVE_POINTS];
                for (i, y) in ys.iter_mut().enumerate() {
                    let u = -span
                        + 2.0 * span * i as f64 / (stave_core::debug::CURVE_POINTS - 1) as f64;
                    let a = if stack == 3 {
                        shape_pp(u)
                    } else {
                        shape_single(u + bf)
                    };
                    *y = match stack {
                        0 => a,
                        _ => post(a),
                    } as f32;
                }
                bus.publish_curve(&ys);
            }
            {
                let span = stave_core::debug::CURVE_X_SPAN;
                let nx = |u: f64| ((u + span) / (2.0 * span)).clamp(0.0, 1.0);
                self.taps.pad_rest_x.set(bus, nx(frame_bias as f64));
                self.taps.pad_live_x.set(bus, nx(frame_bias as f64));
                self.taps.pad_live_g.set(bus, 0.0);
            }
            // Engine-view reference: the shaper-domain amplitude a −9 dBFS
            // tone reaches at the current drive (harmonic-ladder input).
            self.sc_gain = 1.2 + 0.38 * drive;
            let viz_amp = if stack == 2 {
                self.sc_gain * 0.35
            } else {
                (1.0 + drive * 0.9) * 0.35
            };
            self.taps.viz_amp.set(bus, viz_amp as f64);
        }
    }
}

/// Forge's declared observation points: every wire name the
/// engine publishes, in ONE place. The engine writes through these
/// handles only; the stage taps carry stable names — pill labels are
/// the editor's business (editor.rs pill declarations), never the
/// wire's.
pub struct ForgeTaps {
    pub io_in: Tap,
    pub io_out: Tap,
    pub viz_amp: Tap,
    pub cpu_pct: Tap,
    pub pad_rest_x: Tap,
    pub pad_live_x: Tap,
    pub pad_live_g: Tap,
    pub stage: [Tap; 6],
}

impl ForgeTaps {
    pub(super) const fn new() -> Self {
        Self {
            io_in: Tap::new("io_in"),
            io_out: Tap::new("io_out"),
            viz_amp: Tap::new("viz_amp"),
            cpu_pct: Tap::new("cpu_pct"),
            pad_rest_x: Tap::new("pad_rest_x"),
            pad_live_x: Tap::new("pad_live_x"),
            pad_live_g: Tap::new("pad_live_g"),
            stage: [
                Tap::new("tap0"),
                Tap::new("tap1"),
                Tap::new("tap2"),
                Tap::new("tap3"),
                Tap::new("tap4"),
                Tap::new("tap5"),
            ],
        }
    }
}

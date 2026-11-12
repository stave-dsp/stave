//! The block-rate binding pass: watches poll, rebuilds fire, routes
//! and base frames refresh (the Modulation plane's application half).

use stave_dsp::RcClipperConfig;
use stave_dsp::{shapes, Frame, PerChannel, StageKind};

use super::modulation::Modulation;

use stave_core::params::{ModalWatch, Watch};

use crate::engine::voicing::{
    character_shape, conduction_sense, post_spec, stage_b_knee, voice_spec,
};
use crate::params::ForgeParams;

extern crate alloc;

use super::strip::Strip;
use super::strip::OS_FACTOR;
use super::telemetry::Telemetry;
use crate::params::stack;

/// The binding sets: every watch the engine polls, one place.
pub(super) struct Bindings {
    /// Stage-table rebuild — MODE-KEYED: character/stack/edge
    /// always matter; drive only in screamer, bias only in push-pull.
    stages: ModalWatch<3, 2>,
    filters: Watch<4>,
    grit: Watch<1>,
    iron: Watch<1>,
}

impl Bindings {
    pub(super) const fn new() -> Self {
        Self {
            stages: ModalWatch::new([stack::SCREAMER, stack::PUSH_PULL]),
            filters: Watch::new(),
            grit: Watch::new(),
            iron: Watch::new(),
        }
    }
    pub(super) fn reset(&mut self) {
        self.stages.reset();
        self.filters.reset();
        self.grit.reset();
        self.iron.reset();
    }
}

/// Rebuild the primary/secondary curves and (re)assign stage kinds
/// for the current STACK. Called only when a selector or shaping
/// knob actually moved.
fn rebuild_stages(
    sr: f32,
    strips: &mut PerChannel<Strip>,
    character: usize,
    stack: usize,
    edge: f32,
    drive: f32,
    bias: f32,
) {
    let e = edge as f64;
    let inner = sr * OS_FACTOR;
    let cs_params = character_shape(character, e);
    // PUSH-PULL bakes the pair into ONE antisymmetric table: standing
    // bias cools with EDGE (toward crossover), BIAS is the mismatch.
    let pp_b0 = 0.7 - 0.45 * e;
    let pp_m = (bias as f64) * 0.8;
    let shape_single = cs_params.eval();
    let shape_pp = cs_params.eval_push_pull(pp_b0, pp_m);
    let shape = move |x: f64| {
        if stack == 3 {
            shape_pp(x)
        } else {
            shape_single(x)
        }
    };
    let kb = stage_b_knee(character, e);
    for strip in strips.iter_mut() {
        // --- primary ---------------------------------------------------
        if stack == 2 {
            // SCREAMER: a real charge-history clipper first. Drive
            // lives in its input gain; EDGE in conduction hardness.
            // Kind switches engage the swap point (moves, no
            // alloc); in-kind knob moves re-voice the table in
            // place and keep the cap charge — no state dump
            // mid-note.
            let primary_id = strip.region.inner.inner.primary;
            strip.primary_bench.engage(
                strip.region.inner.inner.as_chain_mut(),
                primary_id,
                |s| matches!(s, StageKind::Rc(_)),
                inner,
            );
            let id_a = strip.amp().primary;
            if let Some(StageKind::Rc(rc)) = strip.amp_mut().as_chain_mut().stage_mut(id_a) {
                rc.set_ts_feedback_clipper(
                    &RcClipperConfig {
                        tau_ms: 3.0,
                        threshold: 0.5,
                        hardness: 4.0 + 8.0 * e as f32,
                    },
                    1.2 + 0.38 * drive,
                );
            }
        } else {
            let primary_id = strip.region.inner.inner.primary;
            strip.primary_bench.engage(
                strip.region.inner.inner.as_chain_mut(),
                primary_id,
                |s| matches!(s, StageKind::Curve(_)),
                inner,
            );
            let id_a = strip.amp().primary;
            if let Some(StageKind::Curve(cs)) = strip.amp_mut().as_chain_mut().stage_mut(id_a) {
                cs.rebuild_with_sense(-12.0, 12.0, 0.0, &shape, conduction_sense);
            }
        }
        // --- second ---------------------------------------------------
        match stack {
            0 => {
                // SINGLE: second nonlinearity out of the circuit —
                // benched, not dropped (a drop deallocates its table
                // on the audio thread).
                let second_id = strip.region.inner.inner.second;
                strip.second_bench.engage(
                    strip.region.inner.inner.as_chain_mut(),
                    second_id,
                    |s| matches!(s, StageKind::Bypass),
                    inner,
                );
            }
            2 => {
                // SCREAMER: soft post-shape after the RC clipper
                // (unbenched stale — rebuilt right here).
                let second_id = strip.region.inner.inner.second;
                strip.second_bench.engage(
                    strip.region.inner.inner.as_chain_mut(),
                    second_id,
                    |s| matches!(s, StageKind::Curve(_)),
                    inner,
                );
                let id_b = strip.amp().second;
                if let Some(StageKind::Curve(cs)) = strip.amp_mut().as_chain_mut().stage_mut(id_b) {
                    cs.rebuild(-4.0, 4.0, 0.0, shapes::knee(1.5 + e));
                }
            }
            _ => {
                // CASCADE / PUSH-PULL: character-matched second knee.
                let second_id = strip.region.inner.inner.second;
                strip.second_bench.engage(
                    strip.region.inner.inner.as_chain_mut(),
                    second_id,
                    |s| matches!(s, StageKind::Curve(_)),
                    inner,
                );
                let id_b = strip.amp().second;
                if let Some(StageKind::Curve(cs)) = strip.amp_mut().as_chain_mut().stage_mut(id_b) {
                    cs.rebuild(-4.0, 4.0, 0.0, shapes::knee(kb));
                }
            }
        }
    }
}

impl Bindings {
    /// The block-rate pass: watches poll, rebuilds fire, routes and
    /// base frames refresh, the static viz republishes on change.
    /// Every dependency is an ARGUMENT — what this pass may touch is
    /// its signature.
    pub(super) fn apply_changes(
        &mut self,
        p: &ForgeParams,
        sr: f32,
        strips: &mut PerChannel<Strip>,
        modulation: &mut Modulation,
        telemetry: &mut Telemetry,
        debug: &Option<alloc::sync::Arc<stave_core::DebugBus>>,
    ) {
        let inner_rate = sr * OS_FACTOR;
        let drive = p.drive.value();
        let character = p.character.index();
        let edge = p.edge.value();
        let bias = p.bias.value();
        let stack = p.stack.index();
        let voice = p.filter.index();
        let post = p.post.index();
        let tone = p.tone.value();
        let memory = p.memory.index();
        let depth = p.depth.value();
        let grit = p.grit.value();
        let iron = p.iron.value();
        let sub_mix = p.sub_mix.value();
        let sub_mod = p.sub_mod.value();
        let width = p.width.value();

        // The stage-table binding set: base voicing params plus the
        // mode-gated pair (screamer drive / push-pull bias). The
        // ModalWatch polls everything unconditionally and fires
        // gated params only in their stack.
        if self.stages.changed(
            stack,
            [&p.character, &p.stack, &p.edge],
            [&p.drive, &p.bias],
        ) {
            rebuild_stages(sr, strips, character, stack, edge, drive, bias);
        }
        if self
            .filters
            .changed([&p.stack, &p.filter, &p.post, &p.tone])
        {
            let spec1 = voice_spec(voice, tone);
            let spec2 = post_spec(post);
            for strip in strips.iter_mut() {
                strip.amp_mut().voicing().set(spec1);
                strip.amp_mut().post().set(spec2);
            }
        }
        // Block-rate handoff to the modulation plane: wiring levels
        // and the tone base routes displace from. voice_spec is pure,
        // so recomputing it here is declaration, not work.
        modulation.begin_block(width, sub_mix, sub_mod, voice_spec(voice, tone));
        // Routes are data — rebuilt every block from (memory, depth,
        // stack, sub mod); a handful of pushes, no allocation.
        modulation.rebuild_routes(strips[0].amp(), memory, depth, stack, sub_mod);
        modulation.set_active(modulation.memory_on() || sub_mix > 1e-3 || sub_mod > 1e-3);

        // The second stage's base frame: interstage gain into the
        // second clipper; screamer's post-shape stays unity. The
        // primary's frame follows drive and bias per sample (process.rs).
        let second = Frame {
            bias: 0.0,
            drive: if stack == stack::SCREAMER { 1.0 } else { 1.3 },
            gain: 1.0,
        };
        for strip in strips.iter_mut() {
            strip.second_base = second;
            // Swap-aware: a frame only lands on a Curve.
            let id = strip.amp().second;
            if let Some(StageKind::Curve(cs)) = strip.amp_mut().as_chain_mut().stage_mut(id) {
                cs.set_pending(second);
            }
        }

        if self.grit.changed([&p.grit]) {
            for strip in strips.iter_mut() {
                // 0 = transparent, 1 = heavy (0.3 FS/ms).
                let r = 125.0 * (1.0 - grit).powf(2.5) + 0.3;
                strip.amp_mut().grit().set_rates(r, r, inner_rate);
            }
        }
        if self.iron.changed([&p.iron]) {
            for strip in strips.iter_mut() {
                let h = strip.amp_mut().iron();
                h.set_depth(iron);
                h.set_drive(0.4 + 1.6 * iron);
            }
        }

        // Static transfer view + viz reference republish on change —
        // observation code, so the Telemetry owns it (observe.rs).
        telemetry.publish_static_viz(
            debug,
            super::telemetry::Voicing {
                character,
                stack,
                edge,
                bias,
                frame_bias: super::process::primary_frame(drive, bias, stack).bias,
                drive,
            },
        );
    }
}

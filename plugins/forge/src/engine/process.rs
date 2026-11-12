//! The audio block — the implementation of the framework's
//! `Processor::process` contract: the M/S strips tick under the
//! modulation plane, dry paths pad, output trims ramp from the
//! table's declared smoothing. Channel plumbing (lane slicing,
//! zeroing, peak metering) is the framework's `Lanes` scaffolding;
//! only the DSP is ours.

use stave_core::ProcessCtx;
use stave_dsp::{db_to_amp, ms_decode, ms_encode, Frame, PerChannel, Stage, StageKind};

use crate::params::{stack, ForgeParams};

use super::modulation::{driver, Modulation};
use super::strip::Strip;
use super::telemetry::Telemetry;

/// The dry/wet blend, one spelling everywhere.
#[inline]
fn mix_dry(dry: f32, wet: f32, mix: f32) -> f32 {
    dry + (wet - dry) * mix
}

stave_core::continuous_controls! {
    pub(super) struct Controls<ForgeParams> => Ramps, Values {
        input, mix, output, drive, bias, width,
    }
}

/// The primary stage's operating point for this sample; push-pull keeps
/// its asymmetry inside the table, where a frame bias would break the
/// baked antisymmetry.
pub(super) fn primary_frame(drive: f32, bias: f32, stack: usize) -> Frame {
    Frame {
        bias: if stack == stack::PUSH_PULL {
            0.0
        } else {
            bias * 0.5
        },
        drive: 1.0 + drive * 0.9,
        gain: 1.0,
    }
}

/// Set a strip's primary base frame and land it on the stage (only a
/// curve stage carries a frame); modulation displaces from the base.
fn set_primary(strip: &mut Strip, frame: Frame) {
    strip.primary_base = frame;
    let id = strip.amp().primary;
    if let Some(StageKind::Curve(cs)) = strip.amp_mut().as_chain_mut().stage_mut(id) {
        cs.set_pending(frame);
    }
}

/// The block: the M/S strips tick under the modulation plane.
/// Returns the block peaks for the Telemetry. The signature IS the
/// access list — strips, modulation, telemetry, nothing else.
pub(super) fn process_block(
    ctx: &mut ProcessCtx<'_>,
    p: &ForgeParams,
    controls: &mut Controls,
    strips: &mut PerChannel<Strip>,
    modulation: &mut Modulation,
    telemetry: &mut Telemetry,
) -> (f32, f32) {
    let mut ramps = controls.begin_block(ctx.params, p);
    let stack = p.stack.index();

    telemetry.begin_block();

    let lanes = ctx.lanes::<2>();
    let peaks = match lanes.active() {
        2 => lanes.run(|fr| {
            let v = ramps.next();
            let (in_g, mix, out_g) = (db_to_amp(v.input), v.mix, db_to_amp(v.output));
            let (l, r) = (fr.input(0), fr.input(1));
            // Lane 0 = mid strip, lane 1 = side strip (M/S domain);
            // each lane's dry line carries its host-domain sample.
            let (mid_strip, side_strip) = strips.pair_mut();
            let frame = primary_frame(v.drive, v.bias, stack);
            let side_frame = Frame {
                drive: (frame.drive * v.width).max(1e-3),
                ..frame
            };
            set_primary(mid_strip, frame);
            set_primary(side_strip, side_frame);
            let ld = mid_strip.dry.tick(l);
            let rd = side_strip.dry.tick(r);
            // M/S lift via the kernel adapter (an explicit adapter,
            // never an implicit re-mix). The strips process f32, so
            // the narrowing cast is OURS — taken here at the lift
            // boundary, per the adapter contract.
            let (m64, s64) = ms_encode(l, r);
            let mid = (m64 as f32) * in_g;
            let side = (s64 as f32) * in_g;
            let mut mid = mid;
            if modulation.is_active() {
                // Detector feed, one entry per driver lane
                // (modulation.rs names them): LEVEL and RIPPLE follow
                // program level; CONDUCTION reads the primary curve's
                // own sense column (the side lane follows the
                // centre's charge); SUB gets the SIGNED mid — the
                // divider needs zero crossings, not a level.
                let level = mid.abs().max(side.abs());
                let conduction = if modulation.blocking_on() {
                    let id_a = mid_strip.amp().primary;
                    if let Some(StageKind::Curve(cs)) =
                        mid_strip.amp_mut().as_chain_mut().stage_mut(id_a)
                    {
                        cs.sense_tap()
                    } else {
                        // Screamer stack: no sense column on the RC —
                        // level-excess proxy, monotone in program heat.
                        (mid.abs() * 4.0 - 1.0).max(0.0)
                    }
                } else {
                    0.0
                };
                let mut feed = [0.0f32; 4];
                feed[driver::LEVEL] = level;
                feed[driver::CONDUCTION] = conduction;
                feed[driver::RIPPLE] = level;
                feed[driver::SUB] = mid;
                let states = modulation.tick(&feed);
                // Sub MIX: the divider's output joins the chain INPUT,
                // so the octave is saturated by the same circuit
                // (fuzz-lineage placement). Mid only — bass stays mono.
                mid += 1.2 * modulation.sub_mix() * states[driver::SUB];
                modulation.displace(&states, mid_strip);
                modulation.displace(&states, side_strip);
            }
            // Scope capture: shaper-domain input under the LIVE frame,
            // so memory choke/sag visibly warps the cloud.
            let (vb, vd) = telemetry.scope_reference(mid_strip);
            let mwet = mid_strip.region.tick(mid);
            let swet = side_strip.region.tick(side);
            telemetry.capture(fr.index(), vb + vd * mid, mwet, mid_strip);
            let (lw0, rw0) = ms_decode(mwet as f64, swet as f64);
            let (lw, rw) = (lw0 * out_g, rw0 * out_g);
            fr.write(0, mix_dry(ld, lw, mix));
            fr.write(1, mix_dry(rd, rw, mix));
        }),
        1 => lanes.run(|fr| {
            // Defensive lane-0 path for hosts that bypass the
            // declared stereo config (direct API, degraded hosts).
            // DELIBERATELY simpler: no modulation plane, no sub —
            // forge is a stereo instrument; this path only promises
            // a musical fallback, and its exact behavior is pinned.
            let v = ramps.next();
            let (in_g, mix, out_g) = (db_to_amp(v.input), v.mix, db_to_amp(v.output));
            let input = fr.input(0);
            let mid_strip = &mut strips[0];
            set_primary(mid_strip, primary_frame(v.drive, v.bias, stack));
            let dry = mid_strip.dry.tick(input);
            let driven = input * in_g;
            let (vb, vd) = telemetry.scope_reference(mid_strip);
            let wet_raw = mid_strip.region.tick(driven);
            telemetry.capture(fr.index(), vb + vd * driven, wet_raw, mid_strip);
            fr.write(0, mix_dry(dry, wet_raw * out_g, mix));
        }),
        _ => lanes.run(|_| {}),
    };
    (peaks.input, peaks.output)
}

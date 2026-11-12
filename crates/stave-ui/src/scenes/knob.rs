//! The knob's picture, as a pure function migration proof
//! geometry and passes ported EXACTLY from the
//! hand-drawn CoreGraphics original (the neumorphic puck, the
//! recessed track, the halo'd value arc, the on-face pointer dot),
//! so the renderer swap is pixel-faithful by construction.
//!
//! Orientation: the standard dead-zone-at-the-bottom knob in a
//! Y-UP space (the cocoa view's convention): value 0 at 7:30
//! (225°), a 270° clockwise sweep to 4:30 — clockwise in y-up is
//! DECREASING angle, which is what [`value_angle`] derives and the
//! in-module invariants pin.

use crate::scene::{pt, Brush, Cap, Path, Pt, Scene, Stroke};
use crate::style::Color;

/// Colors the knob takes from the theme at the call site (the
/// cocoa layer owns the token → theme mapping today).
#[derive(Copy, Clone, Debug)]
pub struct KnobStyle {
    /// The accent band (value arc, pointer glow).
    pub band: Color,
    /// The recessed track groove.
    pub track: Color,
    /// The pointer's bright core dot.
    pub ink_hi: Color,
}

/// The modulation ring: assigned depth as an arc around the value's
/// position, the LIVE modulated value as a dot riding it.
#[derive(Copy, Clone, Debug)]
pub struct ModRing {
    /// Assigned depth, ±0..1 around the base value.
    pub depth: f64,
    /// The live modulated value 0..1 (the moving dot).
    pub live: f64,
}

/// Geometry + value. `radius` is the OUTER radius (the widget's
/// diameter minus breathing room, halved — the caller's business).
#[derive(Copy, Clone, Debug)]
pub struct KnobState {
    pub center: Pt,
    pub radius: f64,
    /// Normalized value 0..1 (already clamped by the declaration).
    pub value01: f64,
    /// The modulation ring, when any route targets this knob.
    pub mod_ring: Option<ModRing>,
}

/// Value 0 sits at 7:30; the sweep is 270° clockwise (y-up:
/// decreasing angle).
pub const START: f64 = 225.0 * core::f64::consts::PI / 180.0;
pub const SWEEP: f64 = 270.0 * core::f64::consts::PI / 180.0;

const POINTER_HALO_RADIUS: f64 = 3.4;
const POINTER_CORE_RADIUS: f64 = 1.8;

/// Where the value arc ends and the pointer points — THE knob
/// equation, exported so invariants assert against the same derivation
/// the picture uses.
pub fn value_angle(value01: f64) -> f64 {
    START - SWEEP * value01.clamp(0.0, 1.0)
}

/// The whole knob picture, in paint order.
pub fn scene(st: &KnobState, style: &KnobStyle) -> Scene {
    let mut sc = Scene::new();
    let (cx, cy) = (st.center.x, st.center.y);
    let arc_radius = st.radius - 1.5;
    let face_radius = st.radius - 5.0;
    let end = START - SWEEP;
    let value_end = value_angle(st.value01);

    // 1) The raised puck: drop shadow (offset visually down = −y),
    //    "lit from above" cap gradient, hairline rim, top highlight.
    let sh_r = face_radius + 1.5;
    sc.solid(
        Path::circle(pt(cx, cy - 2.5), sh_r),
        Color::rgba(0.0, 0.0, 0.0, 0.45),
    );
    sc.fill(
        Path::circle(pt(cx, cy), face_radius),
        Brush::Linear {
            from: pt(cx, cy + face_radius), // visual top — lit
            to: pt(cx, cy - face_radius),   // visual bottom — deep
            stops: alloc::vec![
                (0.0, Color::rgba(0.215, 0.235, 0.285, 1.0)),
                (1.0, Color::rgba(0.072, 0.082, 0.108, 1.0)),
            ],
        },
    );
    sc.stroke(
        Path::circle(pt(cx, cy), face_radius),
        Stroke::new(Color::rgba(0.62, 0.67, 0.78, 0.24), 1.0),
    );
    sc.stroke(
        Path::arc(
            pt(cx, cy),
            face_radius - 0.6,
            40.0f64.to_radians(),
            140.0f64.to_radians(),
            false,
        ),
        Stroke::new(Color::rgba(1.0, 1.0, 1.0, 0.16), 1.2),
    );

    // 2) The recessed track groove — the faint full sweep.
    sc.stroke(
        Path::arc(pt(cx, cy), arc_radius, START, end, true),
        Stroke {
            color: style.track,
            width: 3.0,
            cap: Cap::Round,
        },
    );

    // 3) The value arc: two widening halo passes (cheap bloom),
    //    then the crisp core line of light.
    if st.value01 > 0.0005 {
        for (w, a) in [(6.0, 0.12), (4.0, 0.22)] {
            sc.stroke(
                Path::arc(pt(cx, cy), arc_radius, START, value_end, true),
                Stroke {
                    color: Color::rgba(style.band.r, style.band.g, style.band.b, a),
                    width: w,
                    cap: Cap::Round,
                },
            );
        }
        sc.stroke(
            Path::arc(pt(cx, cy), arc_radius, START, value_end, true),
            Stroke {
                color: style.band,
                width: 2.0,
                cap: Cap::Round,
            },
        );
    }

    // 3b) The modulation ring: depth arc OUTSIDE the value arc
    //     (from the base value's angle, ± depth of sweep), the live
    //     value as a dot riding the ring.
    if let Some(ring) = st.mod_ring {
        let ring_r = st.radius + 2.0;
        let a_base = value_end;
        let a_depth = value_angle((st.value01 + ring.depth).clamp(0.0, 1.0));
        sc.stroke(
            Path::arc(pt(cx, cy), ring_r, a_base, a_depth, ring.depth >= 0.0),
            Stroke {
                color: Color::rgba(style.band.r, style.band.g, style.band.b, 0.55),
                width: 1.6,
                cap: Cap::Round,
            },
        );
        let a_live = value_angle(ring.live.clamp(0.0, 1.0));
        let live_dot = pt(cx + a_live.cos() * ring_r, cy + a_live.sin() * ring_r);
        sc.solid(Path::circle(live_dot, 2.0), style.band);
    }

    // 4) The pointer: tinted halo + bright core dot ON the face.
    let pr = face_radius * 0.68;
    let dot = pt(cx + value_end.cos() * pr, cy + value_end.sin() * pr);
    sc.solid(
        Path::circle(dot, POINTER_HALO_RADIUS),
        Color::rgba(style.band.r, style.band.g, style.band.b, 0.22),
    );
    sc.solid(Path::circle(dot, POINTER_CORE_RADIUS), style.ink_hi);
    sc
}

extern crate alloc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Op, Seg};

    fn style() -> KnobStyle {
        KnobStyle {
            band: Color::rgba(0.4, 0.7, 1.0, 1.0),
            track: Color::rgba(0.2, 0.2, 0.25, 0.45),
            ink_hi: Color::rgba(0.95, 0.96, 1.0, 1.0),
        }
    }

    fn state(v: f64) -> KnobState {
        KnobState {
            center: pt(50.0, 50.0),
            radius: 40.0,
            value01: v,
            mod_ring: None,
        }
    }

    /// The first VISUAL INVARIANT: the value arc's end angle and the
    /// pointer dot's position both track the param through the SAME
    /// derived equation — asserted on scene ops, no screenshot.
    #[test]
    fn the_value_arc_and_pointer_track_the_param() {
        for v in [0.1, 0.25, 0.5, 0.75, 1.0] {
            let sc = scene(&state(v), &style());
            let expect = value_angle(v);
            // The crisp core stroke (width 2.0) is the value arc.
            let arc_end = sc
                .ops
                .iter()
                .find_map(|op| match op {
                    Op::Stroke { path, stroke } if stroke.width == 2.0 => match path.segs[0] {
                        Seg::Arc { a1, .. } => Some(a1),
                        _ => None,
                    },
                    _ => None,
                })
                .expect("value arc present");
            assert!(
                (arc_end - expect).abs() < 1e-12,
                "v={v}: arc {arc_end} vs {expect}"
            );
            // The pointer dot (last op) sits at the same angle.
            let dot_c = match sc.ops.last().unwrap() {
                Op::Fill { path, .. } => match path.segs[0] {
                    Seg::Arc { c, .. } => c,
                    _ => panic!("dot is a circle"),
                },
                _ => panic!("dot is a fill"),
            };
            let pr = (40.0 - 5.0) * 0.68;
            assert!((dot_c.x - (50.0 + expect.cos() * pr)).abs() < 1e-12);
            assert!((dot_c.y - (50.0 + expect.sin() * pr)).abs() < 1e-12);
        }
    }

    /// Value 0 draws no arc of light (the dead zone is honest).
    #[test]
    fn zero_draws_no_value_arc() {
        let at_zero = scene(&state(0.0), &style());
        let lit = scene(&state(0.5), &style());
        assert_eq!(
            lit.ops.len() - at_zero.ops.len(),
            3,
            "halo ×2 + core appear with value"
        );
    }

    /// The picture is a pure function: same state, same ops.
    #[test]
    fn the_scene_is_pure() {
        assert_eq!(scene(&state(0.7), &style()), scene(&state(0.7), &style()));
    }

    /// The mod ring: its depth arc starts at the
    /// BASE value's angle and its live dot rides the ring at the
    /// modulated value's angle — the same equation as everything.
    #[test]
    fn the_mod_ring_rides_the_value_equation() {
        let mut st = state(0.5);
        st.mod_ring = Some(ModRing {
            depth: 0.3,
            live: 0.65,
        });
        let sc = scene(&st, &style());
        let ring_r = 42.0;
        let want = value_angle(0.65);
        let dot = sc
            .ops
            .iter()
            .find_map(|op| match op {
                Op::Fill { path, .. } => match path.segs[0] {
                    Seg::Arc { c, r, .. } if (r - 2.0).abs() < 1e-9 => Some(c),
                    _ => None,
                },
                _ => None,
            })
            .expect("live dot present");
        assert!((dot.x - (50.0 + want.cos() * ring_r)).abs() < 1e-9);
        assert!((dot.y - (50.0 + want.sin() * ring_r)).abs() < 1e-9);
        // No routes → no ring ops (dormant is FREE).
        assert_eq!(scene(&state(0.5), &style()).ops.len() + 2, sc.ops.len());
    }
}

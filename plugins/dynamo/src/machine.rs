//! THE MACHINE DIAGRAM — the living center:
//! dynamo's OWN picture through the open scene kind, a
//! pure function of five probes. The boiler's needle droops under
//! chords, the rotor's spoke strobes with the shaft, the wires
//! wave with their energy, the bridge block glows with rail load.
//! Nothing here is decoration: every moving part IS a probe the
//! engine publishes as declared taps (never name-sniffed).
//!
//! Probe order: spin (rate/commanded 0..1), shaft (revolutions
//! 0..1), glow (ensemble wire energy 0..1), boiler (pressure
//! 0..1), rail (bridge |load| 0..1).

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::Dim;
use stave_core::UiNode;
use stave_ui::scene::{pt, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

extern crate alloc;
use alloc::vec::Vec;

/// The editor node: the diagram bound to the machine's taps.
pub fn diagram() -> UiNode {
    SceneDisplay {
        params: &[],
        probes: &["spin", "shaft", "glow", "boiler", "rail"],
        build,
        drag: None,
    }
    .node()
    .width(Dim::Fill(1.0))
    .height(Dim::Px(78.0))
}

fn build(inp: &SceneInputs<'_>) -> Scene {
    let spin = inp.probes.first().copied().unwrap_or(1.0) as f32;
    let shaft = inp.probes.get(1).copied().unwrap_or(0.0) as f32;
    let glow = inp.probes.get(2).copied().unwrap_or(0.0) as f32;
    let boiler = inp.probes.get(3).copied().unwrap_or(1.0) as f32;
    let rail = inp.probes.get(4).copied().unwrap_or(0.0) as f32;
    scene(
        &MachineState {
            spin,
            shaft,
            glow,
            boiler,
            rail,
            w: inp.w,
            h: inp.h,
        },
        &MachineStyle {
            accent: inp.palette.accent,
            ink: inp.palette.ink_muted,
            line: inp.palette.axis,
        },
    )
}

pub(crate) struct MachineState {
    pub spin: f32,
    pub shaft: f32,
    pub glow: f32,
    pub boiler: f32,
    pub rail: f32,
    pub w: f64,
    pub h: f64,
}

pub(crate) struct MachineStyle {
    pub accent: Color,
    pub ink: Color,
    pub line: Color,
}

fn dim(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, a)
}

/// The picture, pure (the laws read its ops).
pub(crate) fn scene(st: &MachineState, style: &MachineStyle) -> Scene {
    let mut sc = Scene::new();
    let cy = st.h * 0.44;
    let label_y = st.h - 6.0;
    let label = |sc: &mut Scene, x: f64, text: &str| {
        sc.ops.push(Op::Text {
            at: pt(x, label_y),
            text: alloc::string::String::from(text),
            size: 7.0,
            color: style.ink,
        });
    };

    // THE BOILER — a gauge whose needle IS the pressure: full
    // pressure points up-right, a drooping boiler swings left.
    let bx = st.w * 0.08;
    sc.stroke(
        Path::arc(
            pt(bx, cy),
            15.0,
            core::f64::consts::PI * 0.8,
            core::f64::consts::PI * 2.2,
            false,
        ),
        Stroke::round(style.line, 1.4),
    );
    let angle = core::f64::consts::PI * (0.8 + 1.4 * f64::from(st.boiler));
    let needle = pt(bx + 12.0 * angle.cos(), cy + 12.0 * angle.sin());
    sc.stroke(
        Path::polyline(&[pt(bx, cy), needle]),
        Stroke::round(style.accent, 1.6),
    );
    label(&mut sc, bx, "BOILER");

    // THE ROTOR — the shaft's spoke strobes with rotation; the rim
    // brightens as it reaches commanded speed.
    let rx = st.w * 0.26;
    let rim = dim(style.accent, 0.25 + 0.6 * f64::from(st.spin));
    sc.stroke(Path::circle(pt(rx, cy), 17.0), Stroke::round(rim, 1.6));
    let a = core::f64::consts::TAU * f64::from(st.shaft);
    for k in 0..4 {
        let ang = a + core::f64::consts::PI * 0.5 * k as f64;
        sc.stroke(
            Path::polyline(&[pt(rx, cy), pt(rx + 14.0 * ang.cos(), cy + 14.0 * ang.sin())]),
            Stroke::round(dim(style.ink, 0.8), 1.2),
        );
    }
    label(&mut sc, rx, "ROTOR");

    // THE WIRES — the pair waves with its energy: amplitude and
    // glow are the ensemble's ring.
    let (wx0, wx1) = (st.w * 0.36, st.w * 0.70);
    let amp = 7.0 * f64::from(st.glow);
    let wire_glow = dim(style.accent, 0.3 + 0.65 * f64::from(st.glow));
    for (dy, phase) in [(-6.0, 0.0), (6.0, core::f64::consts::PI)] {
        let mut pts: Vec<Pt> = Vec::with_capacity(25);
        for i in 0..=24 {
            let t = i as f64 / 24.0;
            let x = wx0 + (wx1 - wx0) * t;
            let y = cy + dy + amp * (t * core::f64::consts::TAU * 2.0 + phase).sin();
            pts.push(pt(x, y));
        }
        sc.stroke(Path::polyline(&pts), Stroke::round(wire_glow, 1.5));
    }
    label(&mut sc, (wx0 + wx1) * 0.5, "WIRES");

    // THE BRIDGE — one block, glowing with the rail's load.
    let brx = st.w * 0.78;
    sc.solid(
        Path::rect(pt(brx - 4.0, cy - 16.0), 8.0, 32.0),
        dim(style.accent, 0.15 + 0.8 * f64::from(st.rail)),
    );
    sc.stroke(
        Path::rect(pt(brx - 4.0, cy - 16.0), 8.0, 32.0),
        Stroke::new(style.line, 1.0),
    );
    label(&mut sc, brx, "BRIDGE");

    // OUT — the machine's voice leaves stage right.
    sc.stroke(
        Path::polyline(&[pt(brx + 8.0, cy), pt(st.w * 0.93, cy)]),
        Stroke::round(dim(style.ink, 0.7), 1.4),
    );
    sc.stroke(
        Path::polyline(&[
            pt(st.w * 0.93 - 5.0, cy - 4.0),
            pt(st.w * 0.93, cy),
            pt(st.w * 0.93 - 5.0, cy + 4.0),
        ]),
        Stroke::round(dim(style.ink, 0.7), 1.4),
    );

    sc
}

#[cfg(test)]
mod properties {
    use super::*;
    use stave_ui::scene::Seg;

    fn st(boiler: f32, glow: f32) -> MachineState {
        MachineState {
            spin: 1.0,
            shaft: 0.0,
            glow,
            boiler,
            rail: 0.0,
            w: 600.0,
            h: 96.0,
        }
    }

    fn style() -> MachineStyle {
        MachineStyle {
            accent: Color::rgba(0.5, 0.7, 0.9, 1.0),
            ink: Color::rgba(0.6, 0.6, 0.7, 1.0),
            line: Color::rgba(0.3, 0.3, 0.4, 1.0),
        }
    }

    /// The needle IS the pressure: full and empty point to the
    /// declared gauge extremes.
    #[test]
    fn the_needle_is_the_pressure() {
        let needle_x = |b: f32| {
            let sc = scene(&st(b, 0.0), &style());
            // Second stroke = the needle polyline; its endpoint.
            let Op::Stroke { path, .. } = &sc.ops[1] else {
                panic!("needle")
            };
            let Seg::LineTo(p) = path.segs[1] else {
                panic!("needle end")
            };
            p.x
        };
        let full = needle_x(1.0);
        let empty = needle_x(0.0);
        // Full pressure points RIGHT of the pivot; empty swings
        // LEFT past it.
        assert!(full > 600.0 * 0.08, "full needle right of pivot: {full}");
        assert!(
            empty < full - 10.0,
            "empty needle must swing left: {empty} vs {full}"
        );
    }

    /// Quiet wires are FLAT lines; energetic wires wave.
    #[test]
    fn the_wires_wave_with_energy() {
        let spread_of = |glow: f32| {
            let sc = scene(&st(1.0, glow), &style());
            let mut min = f64::MAX;
            let mut max = f64::MIN;
            for op in &sc.ops {
                if let Op::Stroke { path, .. } = op {
                    if path.segs.len() > 10 {
                        for seg in &path.segs {
                            if let Seg::LineTo(p) | Seg::MoveTo(p) = seg {
                                min = min.min(p.y);
                                max = max.max(p.y);
                            }
                        }
                    }
                }
            }
            max - min
        };
        let quiet = spread_of(0.0);
        let hot = spread_of(1.0);
        assert!(
            hot > quiet + 8.0,
            "energy must wave the wires: {quiet} → {hot}"
        );
    }

    /// The picture is pure.
    #[test]
    fn the_scene_is_pure() {
        let a = scene(&st(0.7, 0.4), &style());
        let b = scene(&st(0.7, 0.4), &style());
        assert_eq!(a, b);
    }
}

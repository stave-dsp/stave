//! The response scene — tolex's own picture: the cabinet's computed
//! magnitude curve, live from the knobs, drawn over the reference
//! population's p10–p90 territory (aggregate statistics baked in
//! curve.rs). Every knob is legible here: damping blooms the low
//! resonance, position walks the tilt and the teeth, doping calms
//! them, mass slides the cliff. The curve is the SAME math as the
//! sound (the engine's display twins) and a law renders audio to
//! keep the picture honest (tests/curve_properties.rs).

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::Dim;
use stave_core::UiNode;
use stave_ui::scene::{pt, Path, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

extern crate alloc;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::curve::{self, Sheet};
use crate::params::TolexParams;

const N_PTS: usize = 140;
const F_LO: f64 = 45.0;
const F_HI: f64 = 16_000.0;
const DB_TOP: f64 = 14.0;
const DB_BOT: f64 = -36.0;
const PAD_L: f64 = 6.0;
const PAD_R: f64 = 6.0;
const PAD_T: f64 = 6.0;
const PAD_B: f64 = 14.0;

fn x_of(f: f64, w: f64) -> f64 {
    PAD_L + (f / F_LO).ln() / (F_HI / F_LO).ln() * (w - PAD_L - PAD_R)
}

fn y_of(db: f64, h: f64) -> f64 {
    PAD_T + (DB_TOP - db.clamp(DB_BOT, DB_TOP)) / (DB_TOP - DB_BOT) * (h - PAD_T - PAD_B)
}

/// The build fn (declaration order fixed by response_node):
/// size, resonance, q, damping, back, cab, mass, breakup, doping,
/// position, mic.
fn build(inp: &SceneInputs<'_>) -> Scene {
    let p = inp.params;
    let sheet = Sheet {
        size: p[0] as usize,
        resonance: p[1] as f32,
        q: p[2] as f32,
        damping: p[3] as f32,
        open: p[4] >= 0.5,
        cab_l: p[5] as f32,
        mass: p[6] as f32,
        breakup: p[7] as f32,
        doping: p[8] as f32,
        position: p[9] as f32,
        mic_cm: p[10] as f32,
        material: p[11] as usize,
        model: p[12] as usize,
        layout: p[13] as usize,
        variant: p[14].round().max(0.0) as usize,
    };
    let (w, h) = (inp.w, inp.h);
    let mut sc = Scene::new();

    // Grid: dB rules every 12, decade frequency rules with labels.
    let grid = Stroke::new(inp.palette.axis, 0.6);
    for db in [-24.0, -12.0, 0.0] {
        sc.stroke(
            Path::polyline(&[pt(PAD_L, y_of(db, h)), pt(w - PAD_R, y_of(db, h))]),
            grid,
        );
        sc.ops.push(stave_ui::scene::Op::Text {
            at: pt(PAD_L + 2.0, y_of(db, h) - 3.0),
            text: (db as i32).to_string(),
            size: 7.0,
            color: inp.palette.ink_muted,
        });
    }
    for (f, label) in [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        sc.stroke(
            Path::polyline(&[pt(x_of(f, w), PAD_T), pt(x_of(f, w), h - PAD_B)]),
            grid,
        );
        sc.ops.push(stave_ui::scene::Op::Text {
            at: pt(x_of(f, w) + 2.0, h - 4.0),
            text: label.to_string(),
            size: 7.0,
            color: inp.palette.ink_muted,
        });
    }

    // The population territory: p90 rail forward, p10 rail back,
    // filled faint — where real cabinets live.
    let band_n = 72;
    let mut band: Vec<stave_ui::scene::Pt> = Vec::with_capacity(band_n * 2);
    for i in 0..band_n {
        let f = F_LO * (F_HI / F_LO).powf(i as f64 / (band_n - 1) as f64);
        band.push(pt(
            x_of(f, w),
            y_of(f64::from(curve::envelope_at(f as f32, true)), h),
        ));
    }
    for i in (0..band_n).rev() {
        let f = F_LO * (F_HI / F_LO).powf(i as f64 / (band_n - 1) as f64);
        band.push(pt(
            x_of(f, w),
            y_of(f64::from(curve::envelope_at(f as f32, false)), h),
        ));
    }
    let a = inp.palette.accent;
    sc.solid(
        {
            let mut path = Path::polyline(&band);
            path.segs.push(stave_ui::scene::Seg::Close);
            path
        },
        Color::rgba(a.r, a.g, a.b, 0.10),
    );

    // Two curves, same math as the sound: the RESTING sheet as a
    // ghost, and the LIVE curve at the motor's operating point
    // (probe 0 = the excursion envelope) — the picture sags and
    // softens exactly as the audio does while you play.
    let excursion = inp.probes.first().copied().unwrap_or(0.0) as f32;
    let mut hz = [0.0f32; N_PTS];
    for (i, slot) in hz.iter_mut().enumerate() {
        *slot = (F_LO * (F_HI / F_LO).powf(i as f64 / (N_PTS - 1) as f64)) as f32;
    }
    let to_pts = |db: &[f32; N_PTS]| -> Vec<stave_ui::scene::Pt> {
        hz.iter()
            .zip(db.iter())
            .map(|(f, d)| pt(x_of(f64::from(*f), w), y_of(f64::from(*d), h)))
            .collect()
    };
    let mut db = [0.0f32; N_PTS];
    if excursion > 0.01 {
        curve::response_into(&sheet, 0.0, &hz, &mut db);
        let ghost = inp.palette.ink_muted;
        sc.stroke(
            Path::polyline(&to_pts(&db)),
            Stroke::new(Color::rgba(ghost.r, ghost.g, ghost.b, 0.5), 1.0),
        );
    }
    curve::response_into(&sheet, excursion, &hz, &mut db);
    sc.stroke(
        Path::polyline(&to_pts(&db)),
        Stroke::round(inp.palette.accent, 1.8),
    );

    sc
}

/// The editor node: the response display bound to the whole build
/// sheet (declaration order is build()'s contract).
pub fn response_node(p: &TolexParams) -> UiNode {
    SceneDisplay {
        params: &[
            &p.size,
            &p.resonance,
            &p.q,
            &p.damping,
            &p.back,
            &p.cab,
            &p.mass,
            &p.breakup,
            &p.doping,
            &p.position,
            &p.mic,
            &p.material,
            &p.model,
            &p.speakers,
            &p.cone,
        ],
        probes: &["excursion"],
        build,
        drag: None,
    }
    .node()
    .width(Dim::Fill(1.0))
    .height(Dim::Px(118.0))
}

//! Mod rings — the tick half of `Widget::ModRings`: read the
//! route slot params + per-slot live probes off the bus, compute
//! each targeted knob's ring (net depth arc + live dot), and dirty
//! the knob when its ring moved. The ring is an INDICATOR, not a
//! meter: the arc spans RING_SWING of the knob's sweep at |depth| 1,
//! and the dot rides source × depth inside it — the Vital feel;
//! calibrating rings to each target's true units is recorded
//! follow-up (it needs per-target application math the UI should
//! not own).

use core::sync::atomic::Ordering;

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::scenes::knob::ModRing;

use crate::view::PluginEditorView;

/// Arc swing (fraction of the knob's normalized range) at |depth| 1.
const RING_SWING: f64 = 0.35;

pub(crate) struct RingsState {
    /// (src ci, tgt ci, depth ci) per slot.
    pub(crate) slots: Vec<(usize, usize, usize)>,
    /// Target index → ringed knob's control index.
    pub(crate) targets: Vec<Option<usize>>,
    /// Per-slot live source probe NAMES — resolved LAZILY: probes
    /// register when the engine first publishes, which may be after
    /// the editor mounts (a DAW can open the editor before
    /// processing starts).
    pub(crate) probe_names: Vec<&'static str>,
    pub(crate) probe_pis: Vec<Option<usize>>,
    /// Computed rings: (knob ci, ring), refreshed per tick.
    pub(crate) computed: Vec<(usize, ModRing)>,
    /// Per-slot shaping cis (via, mode, lag, quant) — the cable
    /// inspector's bindings; None when a project declares none.
    pub(crate) extras: Vec<Option<stave_core::editor::SlotShaping<usize>>>,
}

impl RingsState {
    pub(crate) fn new(
        slots: Vec<(usize, usize, usize)>,
        targets: Vec<Option<usize>>,
        probe_names: Vec<&'static str>,
    ) -> Self {
        let n = probe_names.len();
        Self {
            slots,
            targets,
            probe_names,
            probe_pis: alloc::vec![None; n],
            computed: Vec::new(),
            extras: Vec::new(),
        }
    }
}

/// The knob's ring, if any route targets it (knob drawRect asks).
pub(crate) fn ring_for(v: &PluginEditorView, ci: usize) -> Option<ModRing> {
    let rings = v.rings.as_ref()?;
    rings
        .computed
        .iter()
        .find(|(k, _)| *k == ci)
        .map(|(_, r)| *r)
}

/// The tick half: recompute rings, dirty knobs whose ring moved.
pub(crate) fn update_rings(v: &mut PluginEditorView) {
    let Some(mut rings) = v.rings.take() else {
        return;
    };
    // Lazy probe resolution (see probe_names).
    for (i, pi) in rings.probe_pis.iter_mut().enumerate() {
        if pi.is_none() {
            *pi = crate::mapping::find_probe(&v.bus, rings.probe_names[i]);
        }
    }
    let rings = rings;
    let mut computed: Vec<(usize, ModRing)> = Vec::with_capacity(rings.targets.len());
    for (t_idx, target) in rings.targets.iter().enumerate() {
        let Some(knob_ci) = *target else { continue };
        let (mut depth, mut live_off) = (0.0f64, 0.0f64);
        for (s_idx, &(src_ci, tgt_ci, depth_ci)) in rings.slots.iter().enumerate() {
            let armed = v.plain(src_ci) as usize != 0; // source 0 = OFF
            let tgt = v.plain(tgt_ci) as usize;
            if !armed || tgt != t_idx {
                continue;
            }
            let d = v.plain(depth_ci);
            depth += d;
            let live = rings
                .probe_pis
                .get(s_idx)
                .copied()
                .flatten()
                .map(|pi| f64::from_bits(v.bus.probes[pi].latest.load(Ordering::Relaxed)))
                .unwrap_or(0.0);
            live_off += live * d;
        }
        if depth.abs() > 1e-6 {
            let base = v.param(knob_ci).normalized();
            computed.push((
                knob_ci,
                ModRing {
                    depth: (depth * RING_SWING).clamp(-1.0, 1.0),
                    live: (base + live_off * RING_SWING).clamp(0.0, 1.0),
                },
            ));
        }
    }
    // Dirty any knob whose ring appeared, moved, or vanished.
    let mut dirty: Vec<usize> = Vec::new();
    for (ci, ring) in &computed {
        let prev = rings
            .computed
            .iter()
            .find(|(k, _)| k == ci)
            .map(|(_, r)| *r);
        if prev.map(|p| (p.depth - ring.depth).abs() > 1e-4 || (p.live - ring.live).abs() > 1e-4)
            != Some(false)
        {
            dirty.push(*ci);
        }
    }
    for (ci, _) in &rings.computed {
        if !computed.iter().any(|(k, _)| k == ci) {
            dirty.push(*ci);
        }
    }
    let mut rings = rings;
    rings.computed = computed;
    v.rings = Some(rings);
    for ci in dirty {
        if let Some(&(_, knob_view, _, _)) = v.knobs.iter().find(|(k, ..)| *k == ci) {
            unsafe { crate::msg_void_int(knob_view, crate::sel(c"setNeedsDisplay:"), 1) };
        }
    }
}

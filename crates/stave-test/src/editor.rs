//! The owned-editor bindings, shared by every project's colocated
//! editor tests: the tree must resolve against the table, and each
//! widget must agree with its param's DECLARED endpoint.

use stave_core::params::ParamId;
use stave_core::ui::Kind;
use stave_core::{Plugin, UiNode, Widget};

fn walk(
    n: &UiNode,
    out: &mut Vec<(&'static str, ParamId)>,
    views: &mut Vec<u16>,
    view_pages: &mut Vec<u16>,
    flags: &mut (bool, bool), // (has ModRings, has ModSources)
) {
    if let Kind::Widget(w) = &n.kind {
        match w {
            Widget::Knob { id, .. } => out.push(("knob", *id)),
            Widget::Toggle { id, .. } => out.push(("toggle", *id)),
            Widget::Selector { id, .. } => out.push(("selector", *id)),
            Widget::Power(id) => out.push(("power", *id)),
            Widget::Pad { x, y } => {
                out.push(("pad", *x));
                out.push(("pad", *y));
            }
            Widget::TransferPlot { keys, .. } => {
                out.push(("plot", keys.threshold));
                out.push(("plot", keys.ratio));
                out.push(("plot", keys.makeup));
            }
            Widget::Spectrum(k) => {
                for &id in k.splits.iter().chain(&k.thresholds).chain(&k.sc_hpfs) {
                    out.push(("spectrum", id));
                }
            }
            Widget::Topology(pills) => {
                // Pill labels bind params too — they must resolve.
                for pill in pills {
                    if let stave_core::TapLabel::StepOf(id)
                    | stave_core::TapLabel::StepSuffixed(id, _) = pill.label
                    {
                        out.push(("pill", id));
                    }
                    for &(sel, _, _) in &pill.overrides {
                        out.push(("pill", sel));
                    }
                }
            }
            Widget::Tabs { id, .. } => out.push(("tabs", *id)),
            Widget::Scene { params, .. } => {
                for id in params {
                    out.push(("scene", *id));
                }
            }
            Widget::ModSources => flags.1 = true,
            Widget::ModRings {
                slots,
                targets,
                probes,
                extras,
            } => {
                flags.0 = true;
                for (s, t, d) in slots {
                    out.push(("ring-slot", *s));
                    out.push(("ring-slot", *t));
                    out.push(("ring-slot", *d));
                }
                assert!(
                    extras.is_empty() || extras.len() == slots.len(),
                    "ModRings: {} extras for {} slots",
                    extras.len(),
                    slots.len()
                );
                for e in extras {
                    for id in [e.via, e.mode, e.lag, e.quant] {
                        out.push(("ring-slot", id));
                    }
                }
                for t in targets.iter().flatten() {
                    out.push(("ring-target", *t));
                }
                assert!(
                    probes.len() == slots.len(),
                    "ModRings: {} probes for {} slots",
                    probes.len(),
                    slots.len()
                );
            }
            Widget::ViewTabs { view, labels } => {
                assert!(!labels.is_empty(), "ViewTabs with no labels");
                views.push(*view);
            }
            Widget::Meter { .. }
            | Widget::Harmonics
            | Widget::PresetBar { .. }
            | Widget::Wave { .. }
            | Widget::History { .. } => {}
        }
    }
    // `show_when` pages bind a param by key — they must resolve too,
    // or a page would silently never show. View-bit keys bind a
    // ViewTabs pager instead, checked after the walk.
    if let Some((key, _)) = n.when {
        if key & stave_core::editor::VIEW_WHEN_BIT != 0 {
            view_pages.push((key & 0xFFFF) as u16);
        } else {
            out.push(("page", ParamId(key)));
        }
    }
    for c in &n.children {
        walk(c, out, views, view_pages, flags);
    }
}

pub fn owned_tree_resolves<P: Plugin>(name: &str) -> Vec<(&'static str, String)> {
    let plugin = P::new();
    let table = plugin.param_table();
    let tree = plugin
        .editor_ui()
        .unwrap_or_else(|| panic!("{name}: owns no editor tree"));
    let mut got = Vec::new();
    let (mut views, mut view_pages) = (Vec::new(), Vec::new());
    let mut flags = (false, false);
    walk(&tree, &mut got, &mut views, &mut view_pages, &mut flags);
    // Drag chips are sugar over the rings declaration — chips with
    // no declaration would drop into the void.
    assert!(
        !flags.1 || flags.0,
        "{name}: ModSources present without a ModRings declaration"
    );
    assert!(!got.is_empty(), "{name}: tree has no bound widgets");
    // Every show_view page must have a ViewTabs pager declaring its
    // key — an unpaged view page would silently never flip.
    for vp in &view_pages {
        assert!(
            views.contains(vp),
            "{name}: show_view page keys view {vp} but no ViewTabs declares it"
        );
    }
    got.into_iter()
        .map(|(kind, id)| {
            let p = (0..table.count())
                .filter_map(|i| table.by_index(i))
                .find(|p| p.meta().id == id)
                .unwrap_or_else(|| panic!("{name}: tree id {} not in table", id.0));
            (kind, p.meta().name.to_string())
        })
        .collect()
}

/// Endpoint round-trip check: the widget a
/// param binds to and the endpoint the param DECLARES must agree —
/// one source (params.rs) for param↔widget metadata. A Knob on a
/// stepped param or a Selector on a continuous one is a declaration
/// bug, caught here for every project at once.
pub fn endpoints_agree<P: Plugin>(name: &str) {
    let plugin = P::new();
    let table = plugin.param_table();
    let tree = plugin.editor_ui().unwrap();
    let mut got = Vec::new();
    walk(
        &tree,
        &mut got,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut (false, false),
    );
    for (kind, id) in got {
        let p = (0..table.count())
            .filter_map(|i| table.by_index(i))
            .find(|p| p.meta().id == id)
            .unwrap();
        let steps = p.step_count();
        let pname = p.meta().name;
        match kind {
            "knob" | "pad" => assert_eq!(
                steps, 0,
                "{name}: '{pname}' is a {kind} but declares {steps} steps"
            ),
            "selector" => assert!(
                steps >= 2,
                "{name}: '{pname}' is a selector but declares a continuous endpoint"
            ),
            "pill" => assert!(
                steps >= 2,
                "{name}: '{pname}' labels a topology pill but has no step names"
            ),
            _ => {}
        }
    }
}

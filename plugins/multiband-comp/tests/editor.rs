//! Owned-editor bindings (colocated): the tree resolves against the
//! table; every widget agrees with its param's declared endpoint.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_multiband_comp_vst3::MultibandComp>("multiband");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_multiband_comp_vst3::MultibandComp>("multiband");
}

/// The knob-order record for the band cards: reading order per band,
/// then the footer (successor of the heuristic-parity pin).
#[test]
fn knob_order_is_the_record() {
    let named = owned_tree_resolves::<stave_multiband_comp_vst3::MultibandComp>("multiband");
    let knobs: Vec<&str> = named
        .iter()
        .filter(|(k, _)| *k == "knob")
        .map(|(_, n)| n.as_str())
        .collect();
    let expect: Vec<&str> = [
        [
            "lo threshold",
            "lo ratio",
            "lo knee",
            "lo makeup",
            "lo attack",
            "lo release",
            "lo sc HPF",
            "lo rms mix",
            "lo drive",
            "lo sat mix",
            "lo mix",
        ]
        .as_slice(),
        [
            "mid threshold",
            "mid ratio",
            "mid knee",
            "mid makeup",
            "mid attack",
            "mid release",
            "mid sc HPF",
            "mid rms mix",
            "mid drive",
            "mid sat mix",
            "mid mix",
        ]
        .as_slice(),
        [
            "hi threshold",
            "hi ratio",
            "hi knee",
            "hi makeup",
            "hi attack",
            "hi release",
            "hi sc HPF",
            "hi rms mix",
            "hi drive",
            "hi sat mix",
            "hi mix",
        ]
        .as_slice(),
        ["low/mid", "mid/high"].as_slice(),
    ]
    .concat();
    assert_eq!(knobs, expect);
}

//! The heirloom editor: no cards, no top bar — the pedal, and only
//! the pedal. One scene (pedal/) bound to the face's eleven controls;
//! presets and output trim stay in the host's own menus.

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::HeirloomParams;
use crate::pedal;

pub fn editor(p: &HeirloomParams) -> UiNode {
    let face = SceneDisplay {
        params: &[
            &p.time, &p.repeats, &p.mix, &p.wobble, &p.rpm, &p.age,
            &p.machine, &p.warp, &p.media, &p.bypass, &p.hold,
        ],
        probes: &["spin"],
        build: pedal::build,
        drag: Some(pedal::drag),
    }
    .node()
    .size(pedal::W, pedal::H);
    ui! {
        <Column w={Dim::Px(pedal::W)} h={Dim::Px(pedal::H)}>
            {face}
        </Column>
    }
}

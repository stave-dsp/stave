//! The aeolian editor: no cards, no top bar — the pedal, and only
//! the pedal. One scene (pedal/) bound to the face's nine controls,
//! the twelve string taps and the harp clock; output trim stays in
//! the host's menus.

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::AeolianParams;
use crate::pedal;

pub fn editor(p: &AeolianParams) -> UiNode {
    let face = SceneDisplay {
        params: &[
            &p.wind, &p.sustain, &p.tone, &p.drift, &p.mix,
            &p.register, &p.air, &p.sway, &p.bypass,
        ],
        probes: &crate::FACE_TAPS,
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

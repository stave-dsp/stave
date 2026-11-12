//! The ribbon editor: no cards, no top bar — the machine, and only
//! the machine. One scene (pedal/) bound to the face's ten controls
//! and the tape's two taps; output trim stays in the host's menus.

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::RibbonParams;
use crate::pedal;

pub fn editor(p: &RibbonParams) -> UiNode {
    let face = SceneDisplay {
        params: &[
            &p.rate, &p.intensity, &p.drive, &p.heads, &p.wear,
            &p.erase, &p.spring, &p.mix, &p.bypass, &p.brake,
        ],
        probes: &[crate::LOOP_TAP, crate::SPEED_TAP],
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

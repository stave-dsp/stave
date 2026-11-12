//! The revenant editor: the instrument, and only the instrument. One
//! scene (pedal/) bound to the face's eight controls, the SURPRISE tap
//! and the neurons; output trim stays in the host's menus.

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::RevenantParams;
use crate::pedal;

pub fn editor(p: &RevenantParams) -> UiNode {
    let face = SceneDisplay {
        params: &[&p.learn, &p.memory, &p.chaos, &p.horizon, &p.dream, &p.mix, &p.bypass, &p.sleep],
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

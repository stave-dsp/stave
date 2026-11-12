//! The engram editor: the instrument, and only the instrument. One
//! scene (pedal/) bound to the face's sixteen controls, the SPACE map's
//! cursor and the sixteen neurons' x/y (both dragged as pairs), fed by
//! the packed display taps (energies, positions, hearing, attention, the
//! bank, recall, novelty, the room on the map, every memory's room);
//! output trim stays in the host's menus.

use stave_core::editor_kit::SceneDisplay;
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::EngramParams;
use crate::pedal;

pub fn editor(p: &EngramParams) -> UiNode {
    let face = SceneDisplay {
        params: &[
            &p.size, &p.decay, &p.tone, &p.space, &p.steer, &p.drift,
            &p.recall, &p.focus, &p.deja, &p.matching, &p.texture, &p.plasticity, &p.mix,
            &p.bypass, &p.hold, &p.forget, &p.space_x, &p.space_y,
            &p.node_0_x, &p.node_0_y, &p.node_1_x, &p.node_1_y, &p.node_2_x, &p.node_2_y, &p.node_3_x, &p.node_3_y,
            &p.node_4_x, &p.node_4_y, &p.node_5_x, &p.node_5_y, &p.node_6_x, &p.node_6_y, &p.node_7_x, &p.node_7_y,
            &p.node_8_x, &p.node_8_y, &p.node_9_x, &p.node_9_y, &p.node_10_x, &p.node_10_y, &p.node_11_x, &p.node_11_y,
            &p.node_12_x, &p.node_12_y, &p.node_13_x, &p.node_13_y, &p.node_14_x, &p.node_14_y, &p.node_15_x, &p.node_15_y,
        ],
        probes: &crate::FACE_TAPS,
        build: pedal::build,
        drag: Some(pedal::drag),
    }
    .node_xy(pedal::drag_node)
    .size(pedal::W, pedal::H);
    ui! {
        <Column w={Dim::Px(pedal::W)} h={Dim::Px(pedal::H)}>
            {face}
        </Column>
    }
}

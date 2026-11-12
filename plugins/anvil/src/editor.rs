//! The anvil editor as owned `ui!` markup — the whole window, a
//! pure function of params.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{Knob, Power, PresetBar};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as typography};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::AnvilParameters;

const CONTROL_CELL_WIDTH: f64 = 100.0;
const CONTROL_CELL_HEIGHT: f64 = 92.0;

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

fn build_control_group(
    caption: &'static str,
    control_nodes: Vec<UiNode>,
    column_count: u16,
) -> UiNode {
    let group_width = column_count as f64 * CONTROL_CELL_WIDTH + 24.0;
    ui! {
        <Column w={Dim::Px(group_width)} pad=12 gap=6 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>{caption}</Text>
            {stave_core::ui::grid(column_count, control_nodes)}
        </Column>
    }
}

fn build_knob(parameter: &dyn AnyParam) -> UiNode {
    Knob {
        param: parameter,
        band: None,
    }
    .node()
    .size(CONTROL_CELL_WIDTH, CONTROL_CELL_HEIGHT)
}

pub fn build_editor(parameters: &AnvilParameters) -> UiNode {
    ui! {
        <Column w=1030 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={typography::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={typography::SECTION}>"ANVIL"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::build_presets(parameters) }
                    .node().size(176.0, 26.0)}
                <Power param={&parameters.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {build_control_group("GAIN", alloc::vec![build_knob(&parameters.preamp_gain), build_knob(&parameters.input_low_cut), build_knob(&parameters.topology_separation_amount), build_knob(&parameters.difference_tone), build_knob(&parameters.high_frequency_emphasis)], 2)}
                {build_control_group("TONE", alloc::vec![build_knob(&parameters.bass_tone), build_knob(&parameters.midrange_tone), build_knob(&parameters.treble_tone)], 3)}
                {build_control_group("POWER", alloc::vec![build_knob(&parameters.power_pair_overdrive), build_knob(&parameters.presence_tone), build_knob(&parameters.depth_tone)], 3)}
                {build_control_group("OUTPUT", alloc::vec![build_knob(&parameters.output_level)], 1)}
            </Row>
        </Column>
    }
}

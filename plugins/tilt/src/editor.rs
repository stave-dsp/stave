//! The tilt editor as owned `ui!` markup — the whole window, a pure
//! function of params: a header, one card, two knobs.

extern crate alloc;

use stave_core::editor_kit::{card, Knob, Power, CELL_H, CELL_W};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::TiltParams;

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob {
        param: p,
        band: None,
    }
    .node()
    .size(CELL_W, CELL_H)
}

pub fn editor(p: &TiltParams) -> UiNode {
    ui! {
        <Column w=280 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"TILT"</Text>
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"TILT EQ"</Text>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {card("TONE", alloc::vec![knob(&p.tilt), knob(&p.output)], 2)}
            </Row>
        </Column>
    }
}

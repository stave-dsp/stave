//! The Smear editor as owned ui! markup — the whole window, no
//! framework layout knowledge.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{Knob, Meter, PresetBar};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::SmearParams;

const RAIL_W: f64 = 60.0;
const CELL_W: f64 = 100.0;
const CELL_H: f64 = 92.0;

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

fn card(caption: &'static str, cells: Vec<UiNode>, cols: u16) -> UiNode {
    let w = cols as f64 * CELL_W + 24.0;
    ui! {
        <Column w={Dim::Px(w)} pad=12 gap=6 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>{caption}</Text>
            {stave_core::ui::grid(cols, cells)}
        </Column>
    }
}

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob {
        param: p,
        band: None,
    }
    .node()
    .size(CELL_W, CELL_H)
}

pub fn editor(p: &SmearParams) -> UiNode {
    let garden = card(
        "GARDEN",
        alloc::vec![
            knob(&p.seed),
            knob(&p.smear),
            knob(&p.bloom),
            knob(&p.decay)
        ],
        4,
    );
    let motion = card(
        "MOTION",
        alloc::vec![
            knob(&p.flux),
            knob(&p.shimmer),
            knob(&p.motion),
            knob(&p.depth)
        ],
        4,
    );
    let tone = card("TONE", alloc::vec![knob(&p.drive)], 1);
    let output = card("OUTPUT", alloc::vec![knob(&p.tone), knob(&p.mix)], 2);

    let rail_out: UiNode = ui! {
        <Column w={Dim::Px(RAIL_W)} pad=2 gap=4 align=Center>
            <Text class={CAPTION}>"OUT"</Text>
            <Meter probe="io_out" w=10 h={Dim::Fill(1.0)}/>
            {Knob { param: &p.output, band: None }
                .node()
                .size(RAIL_W - 4.0, 24.0)
                .margin_edges(stave_core::ui::Edges { t: 14.0, r: 0.0, b: 34.0, l: 0.0 })}
        </Column>
    };
    let rail_in: UiNode = ui! {
        <Column w={Dim::Px(RAIL_W)} pad=2 gap=4 align=Center>
            <Text class={CAPTION}>"IN"</Text>
            <Meter probe="io_in" w=10 h={Dim::Fill(1.0)}/>
        </Column>
    };

    ui! {
        <Column w=680 bg={tokens::BG_0}>
            <Row h=42 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Spacer/>
                {PresetBar { presets: Vec::new() }.node().size(176.0, 26.0)}
                <Spacer/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} align=Stretch>
                {rail_in}
                <Column w={Dim::Fill(1.0)} pad=18 gap=18>
                    <Row gap=18 wrap>
                        {garden}
                        {motion}
                        {tone}
                        {output}
                    </Row>
                </Column>
                {rail_out}
            </Row>
            <Row w={Dim::Fill(1.0)} h=24 justify=Center>
                <Text class={CAPTION}>"S T A V E"</Text>
            </Row>
        </Column>
    }
}

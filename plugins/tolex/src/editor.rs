//! The tolex editor as owned `ui!` markup — the whole window, a
//! pure function of params. The signal path reads left to right:
//! SPEAKER → AMP → CAB → MIC → OUTPUT, same order the engine runs.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{History, Knob, Power, PresetBar, Selector, Spectrum};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::TolexParams;

const CELL_W: f64 = 80.0;
const CELL_H: f64 = 76.0;
/// Selector-bearing cards get wider cells — the dropdown pills need
/// ~90 px before they collide with their card borders.
const SEL_W: f64 = 94.0;

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

fn response_card(p: &TolexParams) -> UiNode {
    ui! {
        <Column w={Dim::Fill(1.0)} pad=12 gap=6 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"RESPONSE — MODEL OVER THE REFERENCE POPULATION"</Text>
            {crate::scene::response_node(p)}
            {Spectrum { splits: &[], thresholds: &[], sc_hpfs: &[], gr_probes: &[] }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(32.0))}
            <Text class={CAPTION}>"CONE TRAVEL"</Text>
            {History { probe: "travel" }.node().width(Dim::Fill(1.0)).height(Dim::Px(20.0))}
        </Column>
    }
}

pub fn editor(p: &TolexParams) -> UiNode {
    // ONE row of five cards, every card exactly two cells tall —
    // no ragged bottoms, no dead corners. The bypass Power fills
    // OUTPUT's second cell (where it belongs anyway); the header
    // keeps only the wordmark. Selector-bearing cards use SEL_W
    // cells so the pills stop kissing their borders.
    let speaker = card(
        "SPEAKER",
        alloc::vec![
            Selector {
                param: &p.size,
                labels: crate::params::SIZES,
                pill: false
            }
            .node()
            .size(CELL_W, CELL_H),
            knob(&p.resonance),
            knob(&p.q),
            knob(&p.cone),
            knob(&p.mass),
            knob(&p.breakup),
            knob(&p.doping),
            stave_core::ui::spacer().size(CELL_W, CELL_H),
        ],
        4,
    );
    let amp = card("AMP", alloc::vec![knob(&p.drive), knob(&p.damping)], 1);
    let cab = ui! {
        <Column w={Dim::Px(2.0 * SEL_W + 24.0)} pad=12 gap=6 bg={tokens::BG_1}
                radius={tokens::RADIUS_MD} border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"CAB"</Text>
            {stave_core::ui::grid(2, alloc::vec![
                Selector { param: &p.back, labels: crate::params::BACKS, pill: false }
                    .node()
                    .size(SEL_W, CELL_H),
                knob(&p.cab).size(SEL_W, CELL_H),
                Selector { param: &p.material, labels: crate::params::MATERIALS, pill: false }
                    .node()
                    .size(SEL_W, CELL_H),
                Selector { param: &p.speakers, labels: crate::params::SPEAKERS, pill: false }
                    .node()
                    .size(SEL_W, CELL_H),
            ])}
        </Column>
    };
    let mic = ui! {
        <Column w={Dim::Px(2.0 * SEL_W + 24.0)} pad=12 gap=6 bg={tokens::BG_1}
                radius={tokens::RADIUS_MD} border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"MIC"</Text>
            {Selector { param: &p.model, labels: crate::params::MICS, pill: false }
                .node()
                .size(2.0 * SEL_W, CELL_H)}
            {stave_core::ui::grid(2, alloc::vec![
                knob(&p.mic).size(SEL_W, CELL_H),
                knob(&p.position).size(SEL_W, CELL_H),
            ])}
        </Column>
    };
    let output = card(
        "OUTPUT",
        alloc::vec![
            knob(&p.output),
            Power { param: &p.bypass }.node().size(CELL_W, CELL_H),
        ],
        1,
    );

    ui! {
        <Column w=1060 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=36
                 pad={stave_core::ui::Edges { t: 8.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK}>"STAVE"</Text>
                <Text class={ty::SECTION}>"TOLEX"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::presets(p) }
                    .node().size(176.0, 26.0)}
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)}
                 pad={stave_core::ui::Edges { t: 12.0, r: 14.0, b: 0.0, l: 14.0 }}>
                {response_card(p)}
            </Row>
            <Row w={Dim::Fill(1.0)}
                 pad={stave_core::ui::Edges { t: 12.0, r: 14.0, b: 14.0, l: 14.0 }} gap=12
                 justify=Center>
                {speaker}
                {amp}
                {cab}
                {mic}
                {output}
            </Row>
        </Column>
    }
}

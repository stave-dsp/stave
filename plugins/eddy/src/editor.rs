//! The eddy editor as owned `ui!` markup — the whole window, a pure
//! function of params. Landscape, organized by SCOPE: the left
//! column is the globals (the delay line, the levels — they shape
//! every algorithm), MOTION sits center (the global wobble), and the
//! ALGORITHM card on the right is the one SECTIONED area — a `Tabs`
//! bar over four `show_when` pages stacked in one rect. The TAIL
//! strip spans the full width beneath them all: the one global
//! visual, present with every tab.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{History, Knob, Power, PresetBar, Selector, ShowWhen, Tabs, Wave};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::EddyParams;

const CELL_W: f64 = 100.0;
/// Full grid-cell height (label + knob + value readout).
const CELL_H: f64 = 92.0;

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

/// A section card: tracked-caps caption over an N-column cell grid
/// (the forge recipe — one visual language across the line).
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

/// One tab page: its unique cells, centered in the shared page
/// rect, shown only while the selector param reads `index`.
fn page(sel: &dyn AnyParam, index: usize, cells: Vec<UiNode>) -> UiNode {
    ui! {
        <Row w={Dim::Fill(1.0)} h={Dim::Px(CELL_H)} gap=8 justify=Center
             show_when={(sel, index)}>
            {stave_core::ui::row(cells).gap(8.0)}
        </Row>
    }
}

pub fn editor(p: &EddyParams) -> UiNode {
    // Globals, left: the line every algorithm regenerates through,
    // and the three levels around it.
    let delay = card("DELAY", alloc::vec![knob(&p.time), knob(&p.feedback)], 2);
    let levels = card(
        "LEVELS",
        alloc::vec![knob(&p.dry), knob(&p.wet), knob(&p.output)],
        2,
    );

    // Center: the global wobble — the picture of the cycle, then its
    // knobs.
    let motion: UiNode = ui! {
        <Column w=324 pad=12 gap=6 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"MOTION"</Text>
            {Wave { probe: "lfo_phase" }.node().size(300.0, 56.0)}
            {stave_core::ui::grid(3, alloc::vec![
                knob(&p.rate),
                knob(&p.depth),
                knob(&p.spread),
                knob(&p.flavor),
                Selector { param: &p.motion, labels: crate::params::MOTIONS, pill: false }
                    .node()
                    .size(CELL_W, CELL_H),
            ])}
        </Column>
    };

    // Right: the ONE sectioned area — four feedback laws behind tabs,
    // each page carrying only its own controls.
    let pages: UiNode = ui! {
        <Stack w={Dim::Fill(1.0)} h={Dim::Px(CELL_H)}>
            {page(&p.algo, 0, alloc::vec![
                Selector { param: &p.circuit, labels: crate::params::CIRCUITS, pill: false }
                    .node()
                    .size(CELL_W, CELL_H),
                knob(&p.heat),
            ])}
            {page(&p.algo, 1, alloc::vec![knob(&p.sag), knob(&p.inertia)])}
            {page(&p.algo, 2, alloc::vec![knob(&p.shift), knob(&p.grain), knob(&p.splice)])}
            {page(&p.algo, 3, alloc::vec![knob(&p.orbit)])}
        </Stack>
    };
    let algorithm: UiNode = ui! {
        <Column w=340 pad=12 gap=10 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"ALGORITHM"</Text>
            {Tabs { param: &p.algo, labels: crate::params::ALGOS }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(24.0))}
            {pages}
        </Column>
    };

    // The second sectioned area: the FIELD — four voices behind
    // tabs, and the three couplings shared beneath them (DIFFUSE =
    // heard, BLEED = regeneration, HOLD = the field's own ring).
    let diffusion_pages: UiNode = ui! {
        <Stack w={Dim::Fill(1.0)} h={Dim::Px(CELL_H)}>
            {page(&p.diffusion, 0, alloc::vec![knob(&p.density), knob(&p.wander)])}
            {page(&p.diffusion, 1, alloc::vec![knob(&p.bend), knob(&p.smear)])}
            {page(&p.diffusion, 2, alloc::vec![knob(&p.spray), knob(&p.motes)])}
            {page(&p.diffusion, 3, alloc::vec![knob(&p.size), knob(&p.weave)])}
        </Stack>
    };
    let diffusion_card: UiNode = ui! {
        <Column w=340 pad=12 gap=10 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"DIFFUSION"</Text>
            {Tabs { param: &p.diffusion, labels: crate::params::DIFFUSIONS }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(24.0))}
            {diffusion_pages}
            <Row w={Dim::Fill(1.0)} gap=8 justify=Center>
                {knob(&p.diffuse)}
                {knob(&p.hold)}
                {knob(&p.bleed)}
            </Row>
        </Column>
    };

    ui! {
        <Column w=956 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"EDDY"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::presets(p) }
                    .node().size(176.0, 26.0)}
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"DELAY / CHORUS"</Text>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 align=Stretch>
                <Column gap=16>
                    {delay}
                    {levels}
                </Column>
                {motion}
                <Column gap=16>
                    {algorithm}
                    {diffusion_card}
                </Column>
            </Row>
            <Column w={Dim::Fill(1.0)} pad={stave_core::ui::Edges { t: 0.0, r: 18.0, b: 0.0, l: 18.0 }}>
                <Column w={Dim::Fill(1.0)} pad=12 gap=6 bg={tokens::BG_1}
                        radius={tokens::RADIUS_MD} border={(1.0, tokens::LINE)}>
                    <Text class={CAPTION}>"TAIL"</Text>
                    {History { probe: "tail" }.node().width(Dim::Fill(1.0)).height(Dim::Px(56.0))}
                </Column>
            </Column>
            <Row w={Dim::Fill(1.0)} h=24 justify=Center>
                <Text class={CAPTION}>"S T A V E"</Text>
            </Row>
        </Column>
    }
}

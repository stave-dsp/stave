//! The opal editor as owned `ui!` markup — the whole window, a pure
//! function of params. Left: the GLOBAL card (algorithm selector,
//! feedback, velocity→brightness, output). Right: the OPERATORS —
//! four pages behind tabs (the eddy pattern), each carrying one
//! op's ratio/level/ADSR. The op pager is a ViewTabs over a VIEW
//! KEY — an op page is what you're LOOKING at, not what the
//! plugin sounds like, so it lives outside params, automation, and
//! the session entirely. Eddy's algo tabs stay param-bound: those
//! ARE sounds.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor::SlotShaping;
use stave_core::editor_kit::{
    card, wide_card, EnvelopeDisplay, Knob, ModRings, ModSources, Power, PresetBar, Selector,
    ShowWhen, ViewKey, ViewTabs, CELL_H, CELL_W,
};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::OpalParams;

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob {
        param: p,
        band: None,
    }
    .node()
    .size(CELL_W, CELL_H)
}

/// One LFO's full voice of controls (rate, rise, shape/sync/trig)
/// — two of these side by side are the MOD card's top half.
fn lfo_strip<'a>(
    caption: &'static str,
    rate: &'a dyn AnyParam,
    rise: &'a dyn AnyParam,
    shape: &'a dyn AnyParam,
    sync: &'a dyn AnyParam,
    trig: &'a dyn AnyParam,
) -> UiNode {
    stave_core::ui::column(alloc::vec![
        stave_core::ui::text(caption)
            .text_style(stave_core::editor_kit::CARD_CAPTION)
            .width(Dim::Px(204.0)),
        stave_core::ui::row(alloc::vec![knob(rate), knob(rise)]).gap(4.0),
        Selector {
            param: shape,
            labels: crate::params::LFO_SHAPES,
            pill: true
        }
        .node()
        .size(204.0, 26.0),
        Selector {
            param: sync,
            labels: crate::params::LFO_SYNC,
            pill: true
        }
        .node()
        .size(204.0, 26.0),
        Selector {
            param: trig,
            labels: crate::params::LFO_TRIG,
            pill: true
        }
        .node()
        .size(204.0, 26.0),
    ])
    .gap(6.0)
}

/// The operator pager's view key (editor-local page state).
const OP_PAGES: ViewKey = ViewKey(1);

/// One operator page: its ADSR PICTURE (the display is how ears
/// find sounds) over ratio/level + the four
/// envelope knobs, shown only while the view key reads `index`.
fn op_page(index: usize, display: UiNode, cells: Vec<UiNode>) -> UiNode {
    ui! {
        <Column w={Dim::Fill(1.0)} gap=8 show_view={(OP_PAGES, index)}>
            {display}
            <Row w={Dim::Fill(1.0)} gap=8 justify=Center>
                {stave_core::ui::row(cells).gap(8.0)}
            </Row>
        </Column>
    }
}

fn env_display<'a>(
    a: &'a dyn AnyParam,
    d: &'a dyn AnyParam,
    s: &'a dyn AnyParam,
    r: &'a dyn AnyParam,
) -> UiNode {
    EnvelopeDisplay {
        attack: a,
        decay: d,
        sustain: s,
        release: r,
    }
    .node()
    .width(Dim::Fill(1.0))
    .height(Dim::Px(64.0))
}

pub fn editor(p: &OpalParams) -> UiNode {
    let op_pages: UiNode = ui! {
        <Stack w={Dim::Fill(1.0)} h={Dim::Px(CELL_H + 72.0)}>
            {op_page(0,
                env_display(&p.attack1, &p.decay1, &p.sustain1, &p.release1),
                alloc::vec![
                    knob(&p.ratio1), knob(&p.level1), knob(&p.attack1),
                    knob(&p.decay1), knob(&p.sustain1), knob(&p.release1),
                ])}
            {op_page(1,
                env_display(&p.attack2, &p.decay2, &p.sustain2, &p.release2),
                alloc::vec![
                    knob(&p.ratio2), knob(&p.level2), knob(&p.attack2),
                    knob(&p.decay2), knob(&p.sustain2), knob(&p.release2),
                ])}
            {op_page(2,
                env_display(&p.attack3, &p.decay3, &p.sustain3, &p.release3),
                alloc::vec![
                    knob(&p.ratio3), knob(&p.level3), knob(&p.attack3),
                    knob(&p.decay3), knob(&p.sustain3), knob(&p.release3),
                ])}
            {op_page(3,
                env_display(&p.attack4, &p.decay4, &p.sustain4, &p.release4),
                alloc::vec![
                    knob(&p.ratio4), knob(&p.level4), knob(&p.attack4),
                    knob(&p.decay4), knob(&p.sustain4), knob(&p.release4),
                ])}
        </Stack>
    };
    let operators: UiNode = ui! {
        <Column w=648 pad=12 gap=10 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={stave_core::editor_kit::CARD_CAPTION}>"OPERATORS"</Text>
            {ViewTabs { view: OP_PAGES, labels: crate::params::OPS }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(24.0))}
            {op_pages}
        </Column>
    };
    let global: UiNode = ui! {
        <Column w=248 pad=12 gap=10 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={stave_core::editor_kit::CARD_CAPTION}>"GLOBAL"</Text>
            // The eight algorithm names never fit as chips (the
            // cramped-grid lesson) — a DROPDOWN says one at a time.
            {Selector { param: &p.algo, labels: crate::params::ALGOS, pill: true }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(26.0))}
            {crate::diagram::diagram_node(&p.algo)}
            <Row w={Dim::Fill(1.0)} gap=8 justify=Center>
                {knob(&p.feedback)}
                {knob(&p.vel_mod)}
            </Row>
        </Column>
    };
    ui! {
        <Column w=960 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"OPAL"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::presets(p) }
                    .node().size(176.0, 26.0)}
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"FM SYNTH"</Text>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>

            // Ring declaration: PITCH has no knob; the modulator
            // LEVELS ring their knobs (brightness routes show
            // where brightness lives); FEEDBACK rings feedback.
            {ModRings {
                slots: &[
                    (&p.routes[0].src, &p.routes[0].tgt, &p.routes[0].depth),
                    (&p.routes[1].src, &p.routes[1].tgt, &p.routes[1].depth),
                    (&p.routes[2].src, &p.routes[2].tgt, &p.routes[2].depth),
                    (&p.routes[3].src, &p.routes[3].tgt, &p.routes[3].depth),
                ],
                // Per-op targets are ORPHANS on purpose: their
                // knobs hide behind the OP pager, and a patch bay
                // whose inlets vanish with a page is unusable —
                // they get permanent labeled inlets in the bay
                // instead (rings only where the knob always shows).
                targets: &[
                    None,               // PITCH
                    None,               // LEVEL 2 (OP 2 page)
                    None,               // LEVEL 3
                    None,               // LEVEL 4
                    Some(&p.feedback),  // always visible
                    None,               // LEVEL 1 (OP 1 page)
                    None,               // RATIO 2 (OP 2 page)
                    Some(&p.lfo2_rate), // in the MOD card
                ],
                probes: &["mod 1", "mod 2", "mod 3", "mod 4"],
                extras: &[
                    SlotShaping { via: &p.routes[0].via, mode: &p.routes[0].mode, lag: &p.routes[0].lag, quant: &p.routes[0].quant },
                    SlotShaping { via: &p.routes[1].via, mode: &p.routes[1].mode, lag: &p.routes[1].lag, quant: &p.routes[1].quant },
                    SlotShaping { via: &p.routes[2].via, mode: &p.routes[2].mode, lag: &p.routes[2].lag, quant: &p.routes[2].quant },
                    SlotShaping { via: &p.routes[3].via, mode: &p.routes[3].mode, lag: &p.routes[3].lag, quant: &p.routes[3].quant },
                ],
            }.node()}
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 align=Stretch>
                {global}
                {operators}
            </Row>
            // The patch bay: LFO controls + source outlets in one
            // card; ringed knobs carry their own inlets; PITCH (no
            // knob) gets a labeled inlet in the bay.
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {wide_card("MOD", 760.0, alloc::vec![
                    stave_core::ui::row(alloc::vec![
                        lfo_strip("LFO 1", &p.lfo_rate, &p.lfo_rise,
                                  &p.lfo_shape, &p.lfo_sync, &p.lfo_trig),
                        lfo_strip("LFO 2", &p.lfo2_rate, &p.lfo2_rise,
                                  &p.lfo2_shape, &p.lfo2_sync, &p.lfo2_trig),
                    ])
                        .gap(28.0)
                        .justify(stave_core::ui::Justify::Center)
                        .width(Dim::Fill(1.0)),
                    ModSources {}.node().width(Dim::Fill(1.0)).height(Dim::Px(84.0)),
                ])}
                {card("OUTPUT", alloc::vec![knob(&p.output)], 1)}
            </Row>
        </Column>
    }
}

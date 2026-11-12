//! The dynamo editor as owned `ui!` markup — the whole window, a
//! pure function of params: a header, the envelope card (with its
//! PICTURE), the voice card. Cards come from the KIT.

extern crate alloc;

use stave_core::editor::SlotShaping;
use stave_core::editor_kit::{
    card, wide_card, EnvelopeDisplay, Knob, ModRings, ModSources, Power, Selector, CELL_H, CELL_W,
};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim};
use stave_core::UiNode;

use crate::params::DynamoParams;

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob {
        param: p,
        band: None,
    }
    .node()
    .size(CELL_W, CELL_H)
}

/// One MACRO column (the Atoms lesson): the macro knob with its
/// MOTION row directly beneath — source + depth of the pre-wired
/// slot that targets this macro. Ordinary params all the way down.
fn macro_col<'a>(
    macro_knob: &'a dyn AnyParam,
    motion_src: &'a dyn AnyParam,
    motion_depth: &'a dyn AnyParam,
) -> UiNode {
    stave_core::ui::column(alloc::vec![
        knob(macro_knob),
        Selector {
            param: motion_src,
            labels: crate::params::SOURCES,
            pill: true
        }
        .node()
        .size(92.0, 24.0),
        Knob {
            param: motion_depth,
            band: Some(1)
        }
        .node()
        .size(64.0, 52.0),
    ])
    .gap(6.0)
}

pub fn editor(p: &DynamoParams) -> UiNode {
    ui! {
        <Column w=1000 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"DYNAMO"</Text>
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"IMPOSSIBLE MACHINE"</Text>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>

            // The ring declaration (zero-size): which knob each
            // route target touches. PITCH has no knob (None);
            // LEVEL rings the output; DECAY rings decay.
            {ModRings {
                slots: &[
                    (&p.routes[0].src, &p.routes[0].tgt, &p.routes[0].depth),
                    (&p.routes[1].src, &p.routes[1].tgt, &p.routes[1].depth),
                    (&p.routes[2].src, &p.routes[2].tgt, &p.routes[2].depth),
                    (&p.routes[3].src, &p.routes[3].tgt, &p.routes[3].depth),
                    (&p.routes[4].src, &p.routes[4].tgt, &p.routes[4].depth),
                    (&p.routes[5].src, &p.routes[5].tgt, &p.routes[5].depth),
                    (&p.routes[6].src, &p.routes[6].tgt, &p.routes[6].depth),
                    (&p.routes[7].src, &p.routes[7].tgt, &p.routes[7].depth),
                    (&p.routes[8].src, &p.routes[8].tgt, &p.routes[8].depth),
                    (&p.routes[9].src, &p.routes[9].tgt, &p.routes[9].depth),
                ],
                // Targets: PITCH orphan; LEVEL/DECAY on their
                // knobs; the six MACROS ring the MACHINE card.
                targets: &[
                    None,
                    Some(&p.output),
                    Some(&p.decay),
                    Some(&p.force),
                    Some(&p.mass),
                    Some(&p.memory),
                    Some(&p.couple),
                    Some(&p.steam),
                    Some(&p.chaos),
                ],
                probes: &[
                    "mod 1", "mod 2", "mod 3", "mod 4", "mod 5",
                    "mod 6", "mod 7", "mod 8", "mod 9", "mod 10",
                ],
                extras: &[
                    SlotShaping { via: &p.routes[0].via, mode: &p.routes[0].mode, lag: &p.routes[0].lag, quant: &p.routes[0].quant },
                    SlotShaping { via: &p.routes[1].via, mode: &p.routes[1].mode, lag: &p.routes[1].lag, quant: &p.routes[1].quant },
                    SlotShaping { via: &p.routes[2].via, mode: &p.routes[2].mode, lag: &p.routes[2].lag, quant: &p.routes[2].quant },
                    SlotShaping { via: &p.routes[3].via, mode: &p.routes[3].mode, lag: &p.routes[3].lag, quant: &p.routes[3].quant },
                    SlotShaping { via: &p.routes[4].via, mode: &p.routes[4].mode, lag: &p.routes[4].lag, quant: &p.routes[4].quant },
                    SlotShaping { via: &p.routes[5].via, mode: &p.routes[5].mode, lag: &p.routes[5].lag, quant: &p.routes[5].quant },
                    SlotShaping { via: &p.routes[6].via, mode: &p.routes[6].mode, lag: &p.routes[6].lag, quant: &p.routes[6].quant },
                    SlotShaping { via: &p.routes[7].via, mode: &p.routes[7].mode, lag: &p.routes[7].lag, quant: &p.routes[7].quant },
                    SlotShaping { via: &p.routes[8].via, mode: &p.routes[8].mode, lag: &p.routes[8].lag, quant: &p.routes[8].quant },
                    SlotShaping { via: &p.routes[9].via, mode: &p.routes[9].mode, lag: &p.routes[9].lag, quant: &p.routes[9].quant },
                ],
            }.node()}
            // THE MACHINE — six macros, each with its MOTION row
            // (Atoms' per-knob motion as pre-wired route slots).
            <Row w={Dim::Fill(1.0)} pad=12 gap=14 justify=Center>
                {wide_card("MACHINE", 660.0, alloc::vec![
                    // THE LIVING CENTER: boiler needle, rotor
                    // spoke, waving wires, glowing bridge — every
                    // moving part is an engine probe.
                    crate::machine::diagram(),
                    stave_core::ui::row(alloc::vec![
                        macro_col(&p.force, &p.routes[4].src, &p.routes[4].depth),
                        macro_col(&p.mass, &p.routes[5].src, &p.routes[5].depth),
                        macro_col(&p.memory, &p.routes[6].src, &p.routes[6].depth),
                        macro_col(&p.couple, &p.routes[7].src, &p.routes[7].depth),
                        macro_col(&p.steam, &p.routes[8].src, &p.routes[8].depth),
                        macro_col(&p.chaos, &p.routes[9].src, &p.routes[9].depth),
                    ])
                        .gap(8.0)
                        .justify(stave_core::ui::Justify::Center)
                        .width(Dim::Fill(1.0)),
                ])}
            </Row>
            <Row w={Dim::Fill(1.0)} pad=12 gap=14 justify=Center>
                {wide_card("ENVELOPE", 4.0 * CELL_W + 24.0, alloc::vec![
                    EnvelopeDisplay {
                        attack: &p.attack,
                        decay: &p.decay,
                        sustain: &p.sustain,
                        release: &p.release,
                    }
                        .node()
                        .width(Dim::Fill(1.0))
                        .height(Dim::Px(54.0)),
                    stave_core::ui::row(alloc::vec![
                        knob(&p.attack), knob(&p.decay), knob(&p.sustain), knob(&p.release),
                    ])
                    .gap(0.0),
                ])}
                {card("ROTOR", alloc::vec![
                    knob(&p.sag),
                    Selector { param: &p.contact, labels: crate::params::CONTACTS, pill: false }
                        .node()
                        .size(CELL_W, CELL_H),
                    Selector { param: &p.poles, labels: crate::params::POLES, pill: false }
                        .node()
                        .size(CELL_W, CELL_H),
                    Selector { param: &p.wheel, labels: crate::params::WHEELS, pill: false }
                        .node()
                        .size(CELL_W, CELL_H),
                ], 2)}
                {card("WIRES", alloc::vec![
                    knob(&p.ring),
                    knob(&p.stiff),
                    knob(&p.width),
                    knob(&p.output),
                ], 2)}
            </Row>
            // The patch bay: the LFO's full voice (rate, rise,
            // shape/sync/trig) + source outlets in one card; every
            // ringed knob carries its own inlet. Drag a cable to
            // route — slot params underneath, as always.
            <Row w={Dim::Fill(1.0)} pad=12 gap=14 justify=Center>
                {card("LIFE", alloc::vec![
                    knob(&p.hum),
                    knob(&p.bearing),
                    knob(&p.clank),
                    knob(&p.crackle),
                    knob(&p.tension),
                    knob(&p.grit),
                ], 3)}
                {wide_card("LFO", 560.0, alloc::vec![
                    stave_core::ui::row(alloc::vec![
                        knob(&p.lfo_rate),
                        knob(&p.lfo_rise),
                        stave_core::ui::column(alloc::vec![
                            Selector { param: &p.lfo_shape, labels: crate::params::LFO_SHAPES, pill: true }
                                .node().size(104.0, 26.0),
                            Selector { param: &p.lfo_sync, labels: crate::params::LFO_SYNC, pill: true }
                                .node().size(104.0, 26.0),
                            Selector { param: &p.lfo_trig, labels: crate::params::LFO_TRIG, pill: true }
                                .node().size(104.0, 26.0),
                        ]).gap(6.0),
                    ])
                        .gap(16.0)
                        .justify(stave_core::ui::Justify::Center)
                        .width(Dim::Fill(1.0)),
                    ModSources {}.node().width(Dim::Fill(1.0)).height(Dim::Px(72.0)),
                ])}
            </Row>
        </Column>
    }
}

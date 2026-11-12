//! The chime editor as owned `ui!` markup — the whole window, a
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

use crate::params::ChimeParams;

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob {
        param: p,
        band: None,
    }
    .node()
    .size(CELL_W, CELL_H)
}

pub fn editor(p: &ChimeParams) -> UiNode {
    ui! {
        <Column w=700 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"CHIME"</Text>
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"POLY SYNTH"</Text>
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
                ],
                targets: &[None, Some(&p.output), Some(&p.decay)],
                probes: &["mod 1", "mod 2", "mod 3", "mod 4"],
                extras: &[
                    SlotShaping { via: &p.routes[0].via, mode: &p.routes[0].mode, lag: &p.routes[0].lag, quant: &p.routes[0].quant },
                    SlotShaping { via: &p.routes[1].via, mode: &p.routes[1].mode, lag: &p.routes[1].lag, quant: &p.routes[1].quant },
                    SlotShaping { via: &p.routes[2].via, mode: &p.routes[2].mode, lag: &p.routes[2].lag, quant: &p.routes[2].quant },
                    SlotShaping { via: &p.routes[3].via, mode: &p.routes[3].mode, lag: &p.routes[3].lag, quant: &p.routes[3].quant },
                ],
            }.node()}
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {wide_card("ENVELOPE", 4.0 * CELL_W + 24.0, alloc::vec![
                    EnvelopeDisplay {
                        attack: &p.attack,
                        decay: &p.decay,
                        sustain: &p.sustain,
                        release: &p.release,
                    }
                        .node()
                        .width(Dim::Fill(1.0))
                        .height(Dim::Px(64.0)),
                    stave_core::ui::row(alloc::vec![
                        knob(&p.attack), knob(&p.decay), knob(&p.sustain), knob(&p.release),
                    ])
                    .gap(0.0),
                ])}
                {card("VOICE", alloc::vec![
                    Selector { param: &p.shape, labels: crate::params::SHAPES, pill: false }
                        .node()
                        .size(CELL_W, CELL_H),
                    knob(&p.output),
                ], 2)}
            </Row>
            // The patch bay: the LFO's full voice (rate, rise,
            // shape/sync/trig) + source outlets in one card; every
            // ringed knob carries its own inlet. Drag a cable to
            // route — slot params underneath, as always.
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
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
                    ModSources {}.node().width(Dim::Fill(1.0)).height(Dim::Px(84.0)),
                ])}
            </Row>
        </Column>
    }
}

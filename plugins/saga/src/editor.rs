//! The saga editor as owned `ui!` markup — a pure function of
//! params, on the canonical 1024 × 720 canvas under the shell's top
//! bar. Laid out like the pedal: the PROGRAM card is the face (mode
//! tabs, the per-program legend saying what FEEDBACK / REGEN / X do
//! right now, the seven knobs + SYNC), TWEAK breaks the pedal's
//! one-knob menu out into five, DIVE / RISE is the held footswitch
//! with the live clock beside it, and the TAIL strip runs beneath.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{
    card, wide_card, History, Knob, PluginTopBar, PluginTopBarPort, Selector, ShowWhen, Tabs,
    CELL_H, CELL_W,
};
use stave_core::params::AnyParam;
use stave_core::ui::tokens;
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::{SagaParams, MOD_SHAPES, MODES, SYNCS};

const LEGEND: TextStyle =
    TextStyle { role: TypeRole::Tick, size: 9.0, kern: 1.2, color: tokens::INK_MUTED };

/// DIVE's two states, as the footswitch reads.
const DIVE: &[&str] = &["OFF", "HELD"];

/// What each program does, and what its three changeable knobs mean.
const LEGENDS: [(&str, &str); 5] = [
    ("REVERSE DELAY  →  REVERSE REVERB", "FDBK repeats · REGEN reverse verb · X how reversed"),
    ("REVERSE DELAY  →  OCTAVE-UP REVERB", "FDBK repeats · REGEN octave up · X decay"),
    ("REVERSE DELAY  →  OCTAVE-DOWN REVERB", "FDBK repeats · REGEN octave down · X decay"),
    ("REVERSE REVERB  →  FORWARD REVERB", "FDBK oct down · REGEN oct up · X decay · TIME stretch"),
    ("PITCH DELAY  →  PITCH DELAY", "FDBK delay one · REGEN delay two · X interval order"),
];

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob { param: p, band: None }.node().size(CELL_W, CELL_H)
}

fn legend(sel: &dyn AnyParam, index: usize) -> UiNode {
    let (title, keys) = LEGENDS[index];
    ui! {
        <Column w={Dim::Fill(1.0)} h=34 gap=4 show_when={(sel, index)}>
            <Text class={tokens::text::CAPTION}>{title}</Text>
            <Text class={LEGEND}>{keys}</Text>
        </Column>
    }
}

pub fn editor(p: &SagaParams) -> UiNode {
    let legends: Vec<UiNode> = (0..MODES.len()).map(|i| legend(&p.mode, i)).collect();
    let program = wide_card(
        "PROGRAM",
        4.0 * CELL_W + 24.0,
        alloc::vec![
            Tabs { param: &p.mode, labels: MODES }
                .node()
                .width(Dim::Fill(1.0))
                .height(Dim::Px(26.0)),
            stave_core::ui::stack(legends).width(Dim::Fill(1.0)).height(Dim::Px(34.0)),
            stave_core::ui::grid(4, alloc::vec![
                knob(&p.feedback),
                knob(&p.regen),
                knob(&p.mod_depth),
                knob(&p.mix),
                knob(&p.x),
                knob(&p.time),
                knob(&p.tone),
                Selector { param: &p.sync, labels: SYNCS, pill: false }
                    .node()
                    .size(CELL_W, CELL_H),
            ]),
        ],
    );

    let tweak = card(
        "TWEAK",
        alloc::vec![
            knob(&p.width),
            knob(&p.mod_rate),
            Selector { param: &p.mod_shape, labels: MOD_SHAPES, pill: false }
                .node()
                .size(CELL_W, CELL_H),
            knob(&p.shift),
            knob(&p.ramp),
        ],
        3,
    );

    let dive = wide_card(
        "DIVE / RISE",
        3.0 * CELL_W + 24.0,
        alloc::vec![ui! {
            <Row w={Dim::Fill(1.0)} gap=12 align=Center>
                {Selector { param: &p.dive, labels: DIVE, pill: false }.node().size(CELL_W, CELL_H)}
                {History { probe: "clock" }.node().width(Dim::Fill(1.0)).height(Dim::Px(56.0))}
            </Row>
        }],
    );

    ui! {
        <Column w={Dim::Px(tokens::EDITOR_CANVAS_WIDTH)}
                h={Dim::Px(tokens::EDITOR_CANVAS_HEIGHT)} bg={tokens::BG_0}>
            {PluginTopBar {
                presets: crate::presets::presets(p),
                input: None,
                output: Some(PluginTopBarPort::output(Some(&p.output))),
                bypass: Some(&p.bypass),
            }.node()}
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {program}
                <Column gap=16>
                    {tweak}
                    {dive}
                </Column>
            </Row>
            <Column w={Dim::Fill(1.0)} pad={stave_core::ui::Edges { t: 0.0, r: 18.0, b: 0.0, l: 18.0 }}>
                {wide_card("TAIL", tokens::EDITOR_CANVAS_WIDTH - 36.0, alloc::vec![
                    History { probe: "tail" }.node().width(Dim::Fill(1.0)).height(Dim::Px(150.0)),
                ])}
            </Column>
        </Column>
    }
}

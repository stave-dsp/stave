//! The Forge editor, OWNED here as `ui!` markup: a window-rooted
//! tree — top bar, I/O rails, the operating-point engine card, the
//! voicing card grid, footer. The framework solves and mounts;
//! nothing about this layout exists outside this file.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{Harmonics, Knob, Meter, Pad, Power, PresetBar, Selector, Topology};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::{TapPill, UiNode};

use crate::params::ForgeParams;

const RAIL_W: f64 = 60.0;
const CELL_W: f64 = 100.0;
/// Full grid-cell height (label + knob + value readout).
const CELL_H: f64 = 92.0;

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

/// A voicing card: tracked-caps caption over an N-column cell grid.
/// Stepped params render as cell dropdowns, continuous ones as knobs.
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

fn cell_selector(p: &dyn AnyParam) -> UiNode {
    Selector {
        param: p,
        labels: &[],
        pill: false,
    }
    .node()
    .size(CELL_W, CELL_H)
}

pub fn editor(p: &ForgeParams) -> UiNode {
    let stage1 = card(
        "STAGE 1",
        alloc::vec![
            knob(&p.drive),
            cell_selector(&p.character),
            knob(&p.edge),
            knob(&p.bias)
        ],
        4,
    );
    let topology = card(
        "TOPOLOGY",
        alloc::vec![
            cell_selector(&p.stack),
            cell_selector(&p.filter),
            cell_selector(&p.post),
            knob(&p.tone),
        ],
        4,
    );
    let texture = card(
        "TEXTURE",
        alloc::vec![
            cell_selector(&p.memory),
            knob(&p.depth),
            knob(&p.grit),
            knob(&p.iron),
        ],
        4,
    );
    let sub = card("SUB", alloc::vec![knob(&p.sub_mix), knob(&p.sub_mod)], 2);
    let stereo = card("STEREO", alloc::vec![knob(&p.width)], 1);
    let output = card("OUTPUT", alloc::vec![knob(&p.mix)], 1);

    let rail = |label: &'static str, meter_probe: &'static str, trim: &dyn AnyParam| -> UiNode {
        ui! {
            <Column w={Dim::Px(RAIL_W)} pad=2 gap=4 align=Center>
                <Text class={CAPTION}>{label}</Text>
                <Meter probe={meter_probe} w=10 h={Dim::Fill(1.0)}/>
                {Knob { param: trim, band: None }
                    .node()
                    .size(RAIL_W - 4.0, 24.0)
                    .margin_edges(stave_core::ui::Edges { t: 14.0, r: 0.0, b: 34.0, l: 0.0 })}
            </Column>
        }
    };

    let engine_card: UiNode = ui! {
        <Column w={Dim::Fill(1.0)} pad=12 gap=6 bg={tokens::BG_1}
                radius={tokens::RADIUS_MD} border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>"OPERATING POINT"</Text>
            <Row w={Dim::Fill(1.0)} gap=10 h=84>
                <Pad x={&p.bias} y={&p.drive} w={Dim::Fill(0.6)} h={Dim::Fill(1.0)}/>
                <Harmonics w={Dim::Fill(0.4)} h={Dim::Fill(1.0)}/>
            </Row>
            <Topology pills={topo_pills(p)} w={Dim::Fill(1.0)} h=36/>
        </Column>
    };

    ui! {
        <Column w=1100 bg={tokens::BG_0}>
            <Row h=42 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::presets(p) }
                    .node().size(176.0, 26.0)}
                <Spacer/>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} align=Stretch>
                {rail("IN", "io_in", &p.input)}
                <Column w={Dim::Fill(1.0)} pad=18 gap=16>
                    {engine_card}
                    <Row gap=16>
                        {stage1}
                        {topology}
                    </Row>
                    <Row gap=16>
                        {texture}
                        {sub}
                        {stereo}
                        {output}
                    </Row>
                </Column>
                {rail("OUT", "io_out", &p.output)}
            </Row>
            <Row w={Dim::Fill(1.0)} h=20 justify=Center>
                <Text class={CAPTION}>"S T A V E"</Text>
            </Row>
        </Column>
    }
}

/// The topology strip's pill declarations: stable tap wires,
/// labels derived from THE PARAMS' OWN step names — mode-keyed like
/// the bindings (screamer renames stage 1, push-pull suffixes the
/// character, SINGLE's hidden stage 2 comes from the engine simply
/// not publishing tap2).
fn topo_pills(p: &ForgeParams) -> alloc::vec::Vec<TapPill> {
    use stave_core::TapLabel::{Fixed, StepOf, StepSuffixed};
    let character = p.character.meta().id;
    let stack = p.stack.meta().id;
    let voice = p.filter.meta().id;
    let post = p.post.meta().id;
    alloc::vec![
        TapPill {
            tap: "tap0",
            label: StepOf(character),
            overrides: alloc::vec![
                (stack, 2, Fixed("SCREAM")),
                (stack, 3, StepSuffixed(character, " PP")),
            ],
        },
        TapPill {
            tap: "tap1",
            label: StepOf(voice),
            overrides: alloc::vec![]
        },
        TapPill {
            tap: "tap2",
            label: Fixed("STAGE 2"),
            overrides: alloc::vec![(stack, 2, Fixed("POST"))],
        },
        TapPill {
            tap: "tap3",
            label: StepOf(post),
            overrides: alloc::vec![]
        },
        TapPill {
            tap: "tap4",
            label: Fixed("GRIT"),
            overrides: alloc::vec![]
        },
        TapPill {
            tap: "tap5",
            label: Fixed("IRON"),
            overrides: alloc::vec![]
        },
    ]
}

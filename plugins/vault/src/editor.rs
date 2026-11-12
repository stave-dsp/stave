//! The vault editor as owned `ui!` markup — the whole window, a
//! pure function of params.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{Knob, Power, PresetBar, Selector, ShowWhen};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::VaultParams;

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

/// One polymorphic PAGE: a complete panel for one machine, shown
/// while the topology reads `index` (the eddy tabs pattern — a
/// whole coherent surface per machine, never holes in a shared
/// grid; the field's verdict on the hole approach: "just hiding
/// controls on switch is not very good").
/// Pages carry an EXPLICIT height: `Fill` inside a `Stack` solves
/// to ZERO (frame-dumped h=0 containers — the knobs still DRAW,
/// macOS does not clip, but hitTest culls anything outside its
/// parent's bounds: visible, unclickable — the ascend/fuse/dawn
/// bug).
const PAGE_H: f64 = 161.0;

fn page(p: &VaultParams, index: usize, content: UiNode) -> UiNode {
    ui! {
        <Column w={Dim::Fill(1.0)} h={Dim::Px(PAGE_H)} gap=14
                show_when={(&p.topology as &dyn AnyParam, index)}>
            {content}
        </Column>
    }
}

const SUBCAP: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 8.0,
    kern: 1.2,
    color: tokens::INK_MUTED,
};

pub fn editor(p: &VaultParams) -> UiNode {
    // THE MACHINE spine + THE VOICE: every dial the user touches
    // lives in EXACTLY ONE place, always visible, and the machine
    // reinterprets it (the Valhalla lesson, learned the hard way:
    // binding the same param on two pages leaves the second set
    // dead — the mount layer keys controls by param).
    // The field's grouping (2026-07-21): SPACE is where you are,
    // ENERGY is how the place holds sound, OUTPUT is the plumbing.
    let machine = card(
        "MACHINE",
        alloc::vec![Selector {
            param: &p.topology,
            labels: crate::params::TOPOLOGIES,
            pill: false
        }
        .node()
        .size(CELL_W, CELL_H),],
        1,
    );
    let space = card(
        "SPACE",
        alloc::vec![
            knob(&p.predelay),
            knob(&p.size),
            knob(&p.width),
            knob(&p.diffuse),
        ],
        4,
    );
    let energy = card(
        "ENERGY",
        alloc::vec![knob(&p.decay), knob(&p.bass), knob(&p.damp), knob(&p.tone),],
        4,
    );
    let output = card(
        "OUTPUT",
        alloc::vec![knob(&p.mix), knob(&p.duck), knob(&p.output),],
        3,
    );

    // The legend: what the shared dials MEAN on this machine.
    let legend = ui! {
        <Stack w={Dim::Fill(1.0)} h=16>
            <Row w={Dim::Fill(1.0)} justify=Center show_when={(&p.topology as &dyn AnyParam, 0)}>
                <Text class={SUBCAP}>"LOW ENERGY: HOW LONG THE BOTTOM RINGS · HIGH ENERGY: ABSORPTION, COMPOUNDS PER BOUNCE · TONE: THE WALLS' FINISH · DIFFUSE: SCATTER"</Text>
            </Row>
            <Row w={Dim::Fill(1.0)} justify=Center show_when={(&p.topology as &dyn AnyParam, 1)}>
                <Text class={SUBCAP}>"LOW ENERGY: THE BOTTOM'S RING · HIGH ENERGY: THE DAMPER · TONE: THE PICKUP · DIFFUSE: STRIKE SMEAR"</Text>
            </Row>
            <Row w={Dim::Fill(1.0)} justify=Center show_when={(&p.topology as &dyn AnyParam, 2)}>
                <Text class={SUBCAP}>"DECAY: HOW LONG THE MEMORY HOLDS · LOW/HIGH ENERGY: BEND THE HOLD · TONE: ITS BRIGHTNESS · DIFFUSE: SOFTEN THE STRIKE"</Text>
            </Row>
            <Row w={Dim::Fill(1.0)} justify=Center show_when={(&p.topology as &dyn AnyParam, 3)}>
                <Text class={SUBCAP}>"DECAY: THE BURST'S LENGTH · LOW ENERGY: THE BURST'S BOTTOM · TONE: BRIGHTNESS · DIFFUSE: DENSITY"</Text>
            </Row>
            <Row w={Dim::Fill(1.0)} justify=Center show_when={(&p.topology as &dyn AnyParam, 4)}>
                <Text class={SUBCAP}>"DECAY: THE FIELD'S HOLD · LOW/HIGH ENERGY: BEND IT · TONE: BRIGHTNESS · DIFFUSE: THE WASH"</Text>
            </Row>
        </Stack>
    };

    // The pages: ONLY what exists on one machine and not the other.
    let room_page = page(
        p,
        0,
        ui! {
            <Row w={Dim::Fill(1.0)} gap=14 justify=Center>
                {card("THE PLACE", alloc::vec![
                    knob(&p.distance), knob(&p.stand), knob(&p.early),
                    Selector { param: &p.material, labels: crate::params::MATERIALS, pill: false }
                        .node()
                        .size(CELL_W, CELL_H),
                ], 4)}
            </Row>
        },
    );
    let void_page = page(
        p,
        4,
        ui! {
            <Column w={Dim::Fill(1.0)} h={Dim::Fill(1.0)} gap=6 align=Center justify=Center>
                {card("THE VOID", alloc::vec![knob(&p.dawn)], 1)}
                <Text class={SUBCAP}>"DAWN: SOURCES RISE INTO THE FIELD INSTEAD OF SPLASHING AGAINST IT — NO WALLS ANYWHERE"</Text>
            </Column>
        },
    );
    let det_page = page(
        p,
        3,
        ui! {
            <Column w={Dim::Fill(1.0)} h={Dim::Fill(1.0)} gap=6 align=Center justify=Center>
                {card("THE CHAMBER", alloc::vec![knob(&p.fuse)], 1)}
                <Text class={SUBCAP}>"FUSE: 0 = INSTANT DETONATION, 1 = A SWELL THAT BUILDS INTO THE CUT — BETWEEN HITS, BLACK"</Text>
            </Column>
        },
    );
    let memory_page = page(
        p,
        2,
        ui! {
            <Column w={Dim::Fill(1.0)} h={Dim::Fill(1.0)} gap=6 align=Center justify=Center>
                {card("THE MEMORY", alloc::vec![knob(&p.ascend)], 1)}
                <Text class={SUBCAP}>"ASCEND: THE REMEMBERED CHORD CLIMBS OCTAVES AS IT SUSTAINS — SHIMMER WITH NO PITCH SHIFTER"</Text>
            </Column>
        },
    );
    let plate_page = page(
        p,
        1,
        ui! {
            <Column w={Dim::Fill(1.0)} h={Dim::Fill(1.0)} gap=6 align=Center justify=Center>
                {card("THE SHEET", alloc::vec![knob(&p.stiff)], 1)}
                <Text class={SUBCAP}>"THE WHIP: HOW HARD THE SHEET CHIRPS — A STEEL PLATE'S DEFINING VOICE"</Text>
            </Column>
        },
    );

    ui! {
        <Column w=1100 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"VAULT"</Text>
                <Spacer/>
                {PresetBar { presets: crate::presets::presets(p) }
                    .node().size(196.0, 26.0)}
                <Spacer/>
                <Text class={ty::TAGLINE} tag="bypass-tag">"A PLACE, NOT AN ALGORITHM"</Text>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} pad=14 gap=14 justify=Center>
                {machine}
                {space}
                {energy}
            </Row>
            <Column w={Dim::Fill(1.0)} gap=6 align=Center
                    pad={stave_core::ui::Edges { t: 0.0, r: 14.0, b: 8.0, l: 14.0 }}>
                <Row w={Dim::Fill(1.0)} gap=14 justify=Center>
                    {output}
                </Row>
                {legend}
            </Column>
            <Stack w={Dim::Fill(1.0)} h=175
                   pad={stave_core::ui::Edges { t: 0.0, r: 14.0, b: 14.0, l: 14.0 }}>
                {room_page}
                {plate_page}
                {memory_page}
                {det_page}
                {void_page}
            </Stack>
        </Column>
    }
}

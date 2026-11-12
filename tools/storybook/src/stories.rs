//! The stories — one `ui!` tree, navigated by the `ViewTabs` the
//! storybook exists to showcase (the design system browses itself).
//! Each page shows a component family in its variants with captions.
//!
//! Cards come from the KIT; `swatch()`/`space_bar()` stay local: they are stories
//! about tokens, not components.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{
    preset, wide_card, EnvelopeDisplay, History, Knob, Meter, Pad, PluginTopBar, PluginTopBarPort,
    Power, PresetBar, SceneDisplay, Selector, ShowWhen, Tabs, Toggle, ViewKey, ViewTabs, Wave,
    CELL_H, CELL_W,
};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Color, Dim, TextStyle, TypeRole};
use stave_core::UiNode;
use stave_ui::scene::{pt, Path, Scene, SceneInputs, Stroke};

use crate::params::StoryParams;

/// The top navigation pager.
pub const NAV: ViewKey = ViewKey(0);
pub const NAV_PAGES: &[&str] = &[
    "KNOBS",
    "CONTROLS",
    "PAGERS",
    "DISPLAYS",
    "TELEMETRY",
    "TOKENS",
    "PRESETS",
];

/// The nested ViewTabs demo on the PAGERS page.
const DEMO_VIEWS: ViewKey = ViewKey(1);
const DEMO_VIEW_PAGES: &[&str] = &["FIRST", "SECOND", "THIRD"];

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

fn story_presets(p: &StoryParams) -> Vec<stave_core::Preset> {
    alloc::vec![
        preset(
            "FACTORY / CLEAN / WARM",
            &[
                (&p.gain_a, 6.0),
                (&p.gain_b, -3.0),
                (&p.gain_c, 2.0),
                (&p.mix, 0.7),
                (&p.mode, 1.0),
            ],
        ),
        preset(
            "FACTORY / DRIVEN / PUSHED",
            &[
                (&p.gain_a, 14.0),
                (&p.gain_b, 9.0),
                (&p.gain_c, 18.0),
                (&p.mix, 1.0),
                (&p.mode, 2.0),
            ],
        ),
        preset(
            "FACTORY / TEXTURE / GLASS EDGE",
            &[
                (&p.gain_a, 10.0),
                (&p.gain_b, 2.0),
                (&p.gain_c, 12.0),
                (&p.mix, 0.82),
                (&p.mode, 0.0),
            ],
        ),
        preset(
            "ARTIST / RYAN / NIGHT DRIVE",
            &[
                (&p.gain_a, 16.0),
                (&p.gain_b, 6.0),
                (&p.gain_c, 20.0),
                (&p.mix, 0.9),
                (&p.mode, 2.0),
            ],
        ),
        preset(
            "USER / RECENT / DRAFT 01",
            &[
                (&p.gain_a, 4.0),
                (&p.gain_b, 1.0),
                (&p.gain_c, 8.0),
                (&p.mix, 0.55),
                (&p.mode, 1.0),
            ],
        ),
    ]
}

/// A story card: fixed width (the collapsed-chips lesson), free row
/// body — thin sugar over the KIT's wide_card.
fn card(title: &'static str, body: Vec<UiNode>) -> UiNode {
    wide_card(
        title,
        780.0,
        alloc::vec![stave_core::ui::row(body).gap(12.0)],
    )
}

fn knob(p: &dyn AnyParam, band: Option<usize>) -> UiNode {
    Knob { param: p, band }.node().size(CELL_W, CELL_H)
}

fn swatch(name: &'static str, c: Color) -> UiNode {
    ui! {
        <Column gap=4>
            <Column w=72 h=40 bg={c} radius={tokens::RADIUS_SM}
                    border={(1.0, tokens::LINE)}/>
            <Text class={CAPTION}>{name}</Text>
        </Column>
    }
}

fn space_bar(name: &'static str, w: f64) -> UiNode {
    ui! {
        <Row gap=8>
            <Column w={Dim::Px(w)} h=14 bg={tokens::BG_2} radius={tokens::RADIUS_XS}
                    border={(1.0, tokens::LINE)}/>
            <Text class={CAPTION}>{name}</Text>
        </Row>
    }
}

fn page(index: usize, body: Vec<UiNode>) -> UiNode {
    ui! {
        <Column w={Dim::Fill(1.0)} gap=16 show_view={(NAV, index)}>
            {stave_core::ui::column(body).gap(16.0)}
        </Column>
    }
}

pub fn tree(p: &StoryParams) -> UiNode {
    // ---- KNOBS ------------------------------------------------------
    let knobs = page(
        0,
        alloc::vec![
            card(
                "ACCENT BANDS — band 0 / 1 / 2 / default",
                alloc::vec![
                    knob(&p.gain_a, Some(0)),
                    knob(&p.gain_b, Some(1)),
                    knob(&p.gain_c, Some(2)),
                    knob(&p.mix, None),
                ]
            ),
            card(
                "TAPERS — linear dB · log Hz (drag to feel the taper)",
                alloc::vec![knob(&p.gain_a, Some(0)), knob(&p.freq, Some(1)),]
            ),
        ],
    );

    // ---- CONTROLS ---------------------------------------------------
    let controls = page(
        1,
        alloc::vec![
            card(
                "TOGGLES — solo role · mute role · power chip",
                alloc::vec![
                    Toggle {
                        param: &p.solo,
                        solo: true,
                        band: Some(0)
                    }
                    .node()
                    .size(72.0, 32.0),
                    Toggle {
                        param: &p.mute,
                        solo: false,
                        band: Some(1)
                    }
                    .node()
                    .size(72.0, 32.0),
                    Power { param: &p.power }.node().size(24.0, 24.0),
                ]
            ),
            card(
                "SELECTOR — nameplate · pill",
                alloc::vec![
                    Selector {
                        param: &p.mode,
                        labels: crate::params::MODES,
                        pill: false
                    }
                    .node()
                    .size(CELL_W, CELL_H),
                    Selector {
                        param: &p.mode,
                        labels: crate::params::MODES,
                        pill: true
                    }
                    .node()
                    .size(120.0, 26.0),
                ]
            ),
            card(
                "XY PAD — bias × drive",
                alloc::vec![Pad {
                    x: &p.pad_x,
                    y: &p.pad_y
                }
                .node()
                .size(180.0, 120.0),]
            ),
        ],
    );

    // ---- PAGERS -----------------------------------------------------
    let param_pages: UiNode = ui! {
        <Stack w={Dim::Fill(1.0)} h=40>
            <Row show_when={(&p.pane, 0)}><Text class={ty::SECTION}>"ALPHA PAGE — a param turned me (automatable)"</Text></Row>
            <Row show_when={(&p.pane, 1)}><Text class={ty::SECTION}>"BETA PAGE — hosts can automate this flip"</Text></Row>
            <Row show_when={(&p.pane, 2)}><Text class={ty::SECTION}>"GAMMA PAGE — saved with the session"</Text></Row>
        </Stack>
    };
    let view_pages: UiNode = ui! {
        <Stack w={Dim::Fill(1.0)} h=40>
            <Row show_view={(DEMO_VIEWS, 0)}><Text class={ty::SECTION}>"FIRST — a ViewKey turned me (chrome only)"</Text></Row>
            <Row show_view={(DEMO_VIEWS, 1)}><Text class={ty::SECTION}>"SECOND — no param, no automation, no session"</Text></Row>
            <Row show_view={(DEMO_VIEWS, 2)}><Text class={ty::SECTION}>"THIRD — gap 8, closed by type (E1)"</Text></Row>
        </Stack>
    };
    let pagers = page(
        2,
        alloc::vec![
            // Fixed interior width: a Fill inside an intrinsically
            // sized card degenerates to zero (the collapsed-chips bug —
            // project cards are fixed-width for the same reason).
            card(
                "PARAM TABS — pages that ARE sounds (eddy's algorithms)",
                alloc::vec![ui! {
                    <Column w=760 gap=8>
                        {Tabs { param: &p.pane, labels: crate::params::PANES }
                            .node().width(Dim::Fill(1.0)).height(Dim::Px(24.0))}
                        {param_pages}
                    </Column>
                }]
            ),
            card(
                "VIEW TABS — pages that are VIEWS (opal's operators)",
                alloc::vec![ui! {
                    <Column w=760 gap=8>
                        {ViewTabs { view: DEMO_VIEWS, labels: DEMO_VIEW_PAGES }
                            .node().width(Dim::Fill(1.0)).height(Dim::Px(24.0))}
                        {view_pages}
                    </Column>
                }]
            ),
        ],
    );

    // ---- DISPLAYS ---------------------------------------------------
    /// A BESPOKE picture through the OPEN kind: three gain
    /// params drawn as bars — ~20 lines, zero framework changes.
    /// This is where every project display STARTS; the kit is for
    /// second-consumer graduates (the envelope above is one; opal's
    /// algorithm diagram lives in opal the same way this lives here).
    fn gain_bars(inp: &SceneInputs<'_>) -> Scene {
        let mut sc = Scene::new();
        let n = inp.params.len() as f64;
        let bar_w = (inp.w - 8.0 * (n - 1.0)) / n;
        sc.stroke(
            Path::polyline(&[pt(0.0, inp.h - 1.0), pt(inp.w, inp.h - 1.0)]),
            Stroke::new(inp.palette.axis, 1.0),
        );
        for (i, &db) in inp.params.iter().enumerate() {
            let t = ((db + 24.0) / 48.0).clamp(0.0, 1.0); // ±24 dB range
            let h = t * (inp.h - 4.0);
            let x = i as f64 * (bar_w + 8.0);
            sc.solid(
                Path::rect(pt(x, inp.h - 1.0 - h), bar_w, h),
                inp.palette.accent,
            );
        }
        sc
    }
    let displays = page(
        3,
        alloc::vec![
            card(
                "ENVELOPE DISPLAY — a pure function of four knobs (drag them)",
                alloc::vec![ui! {
                    <Column w=620 gap=8>
                        {EnvelopeDisplay {
                            attack: &p.attack, decay: &p.decay,
                            sustain: &p.sustain, release: &p.release,
                        }.node().width(Dim::Fill(1.0)).height(Dim::Px(72.0))}
                        <Row w={Dim::Fill(1.0)} gap=8 justify=Center>
                            {knob(&p.attack, Some(0))}
                            {knob(&p.decay, Some(0))}
                            {knob(&p.sustain, Some(0))}
                            {knob(&p.release, Some(0))}
                        </Row>
                    </Column>
                }]
            ),
            card(
                "OPEN SCENE KIND — a bespoke picture in ~20 local lines",
                alloc::vec![ui! {
                    <Column w=360 gap=8>
                        {SceneDisplay {
                            params: &[&p.gain_a, &p.gain_b, &p.gain_c],
                            probes: &[],
                            build: gain_bars,
                            drag: None,
                        }.node().width(Dim::Fill(1.0)).height(Dim::Px(90.0))}
                        <Row w={Dim::Fill(1.0)} gap=8 justify=Center>
                            {knob(&p.gain_a, Some(0))}
                            {knob(&p.gain_b, Some(1))}
                            {knob(&p.gain_c, Some(2))}
                        </Row>
                    </Column>
                }]
            ),
        ],
    );

    // ---- TELEMETRY --------------------------------------------------
    let telemetry = page(
        4,
        alloc::vec![
            card(
                "METER — probe-fed, animated by the story driver",
                alloc::vec![Meter { probe: "level" }.node().size(40.0, 120.0),]
            ),
            card(
                "WAVE — published cycle + live phase cursor",
                alloc::vec![Wave { probe: "phase" }.node().size(320.0, 96.0),]
            ),
            card(
                "HISTORY — one number per tick, client-owned ring",
                alloc::vec![History { probe: "env" }.node().size(320.0, 72.0),]
            ),
        ],
    );

    // ---- TOKENS -----------------------------------------------------
    let tokens_page = page(
        5,
        alloc::vec![
            card(
                "SURFACES + INK",
                alloc::vec![
                    swatch("BG_0", tokens::BG_0),
                    swatch("BG_1", tokens::BG_1),
                    swatch("BG_2", tokens::BG_2),
                    swatch("LINE", tokens::LINE),
                    swatch("INK_MUTED", tokens::INK_MUTED),
                ]
            ),
            card(
                "TYPE ROLES",
                alloc::vec![ui! {
                    <Column gap=8>
                        <Text class={ty::WORDMARK}>"WORDMARK"</Text>
                        <Text class={ty::SECTION}>"SECTION"</Text>
                        <Text class={ty::TAGLINE}>"TAGLINE"</Text>
                        <Text class={ty::CAPTION}>"CAPTION"</Text>
                        <Text class={ty::CARD_TITLE}>"CARD TITLE"</Text>
                    </Column>
                }]
            ),
            card(
                "SPACING SCALE",
                alloc::vec![ui! {
                    <Column gap=6>
                        {space_bar("SPACE_XS 4", tokens::SPACE_XS * 8.0)}
                        {space_bar("SPACE_SM 8", tokens::SPACE_SM * 8.0)}
                        {space_bar("SPACE_MD 12", tokens::SPACE_MD * 8.0)}
                        {space_bar("SPACE_LG 18", tokens::SPACE_LG * 8.0)}
                        {space_bar("SPACE_XL 24", tokens::SPACE_XL * 8.0)}
                    </Column>
                }]
            ),
        ],
    );

    // ---- PRESETS ----------------------------------------------------
    let presets = page(
        6,
        alloc::vec![
            card(
                "PRESET BAR — typed catalog, applies via gestures",
                alloc::vec![ui! {
                    <Row w={Dim::Px(tokens::EDITOR_PRESET_BAR_WIDTH)}
                         h={Dim::Px(tokens::EDITOR_TOP_BAR_CONTROL_HEIGHT)}>
                        {PresetBar { presets: story_presets(p) }.node()
                            .size(tokens::EDITOR_PRESET_BAR_WIDTH,
                                  tokens::EDITOR_TOP_BAR_CONTROL_HEIGHT)}
                    </Row>
                },]
            ),
            card(
                "HOW TO READ THIS PAGE",
                alloc::vec![ui! {
                    <Column gap=6>
                        <Text class={CAPTION}>"PICK A PRESET ABOVE — THE KNOBS PAGE MOVES."</Text>
                        <Text class={CAPTION}>"EVERY WRITE WENT THROUGH THE SAME GESTURE PATH A HOST SEES."</Text>
                    </Column>
                }]
            ),
        ],
    );

    ui! {
        <Column w={Dim::Px(tokens::EDITOR_CANVAS_WIDTH)}
                h={Dim::Px(tokens::EDITOR_CANVAS_HEIGHT)} bg={tokens::BG_0}>
            {PluginTopBar {
                presets: story_presets(p),
                input: Some(PluginTopBarPort::input(Some(&p.input_gain))),
                output: Some(PluginTopBarPort::output(Some(&p.output_gain))),
                bypass: None,
            }.node()}
            <Column w={Dim::Fill(1.0)} pad=18 gap=14>
                {ViewTabs { view: NAV, labels: NAV_PAGES }
                    .node().width(Dim::Fill(1.0)).height(Dim::Px(26.0))}
                // Pages OVERLAP in one rect (the eddy Stack pattern) —
                // stacked vertically they'd solve below the window.
                <Stack w={Dim::Fill(1.0)} h=580>
                    {knobs}
                    {controls}
                    {pagers}
                    {displays}
                    {telemetry}
                    {tokens_page}
                    {presets}
                </Stack>
            </Column>
        </Column>
    }
}

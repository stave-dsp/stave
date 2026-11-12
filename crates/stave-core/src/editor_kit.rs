//! Generic editor components: prop-structs with `.node()`,
//! consumed from `ui!` markup — `<Knob param={&p.drive} band={Some(0)}
//! w=86 h=40/>`. NOTHING project-shaped lives here: card recipes,
//! footers and captions are project markup; this file is only the
//! bridge from typed params to Widget leaves.

extern crate alloc;
use alloc::vec::Vec;

use crate::editor::{
    KnobAccent, KnobParts, MeterStyle, Preset, SlotShaping, SpectrumKeys, TransferKeys, UiNode,
    Widget,
};
use crate::params::AnyParam;

pub struct Knob<'a> {
    pub param: &'a dyn AnyParam,
    pub band: Option<usize>,
}
impl Knob<'_> {
    pub fn node(self) -> UiNode {
        self.node_with(KnobParts::FULL)
    }

    /// Compose only the decorations a layout needs around the same control.
    pub fn node_with(self, parts: KnobParts) -> UiNode {
        self.node_with_accent(parts, KnobAccent::ThemeAccent)
    }

    /// Select the accent treatment while retaining the shared knob renderer,
    /// parameter binding, and interaction behavior.
    pub fn node_with_accent(self, parts: KnobParts, accent: KnobAccent) -> UiNode {
        stave_ui::widget(Widget::Knob {
            id: self.param.meta().id,
            band: self.band,
            parts,
            accent,
        })
    }
}

pub struct Toggle<'a> {
    pub param: &'a dyn AnyParam,
    pub solo: bool,
    pub band: Option<usize>,
}
impl Toggle<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Toggle {
            id: self.param.meta().id,
            solo: self.solo,
            band: self.band,
        })
    }
}

pub struct Selector<'a> {
    pub param: &'a dyn AnyParam,
    pub labels: &'static [&'static str],
    /// Compact header pill vs grid-cell nameplate style.
    pub pill: bool,
}
impl Selector<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Selector {
            id: self.param.meta().id,
            labels: self.labels,
            pill: self.pill,
        })
    }
}

pub struct Meter {
    pub probe: &'static str,
}

/// Periodic-waveform display (the LFO view): the engine publishes
/// one cycle via `publish_curve` and a 0..1 phase on `probe`.
pub struct Wave {
    pub probe: &'static str,
}
impl Wave {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Wave { probe: self.probe })
    }
}

/// Segmented tab bar over a stepped param — pair with `show_when`
/// pages stacked below it. Labels usually = the param's step names.
pub struct Tabs<'a> {
    pub param: &'a dyn AnyParam,
    pub labels: &'static [&'static str],
}
impl Tabs<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Tabs {
            id: self.param.meta().id,
            labels: self.labels,
        })
    }
}

/// Scrolling recent-history strip of a probe (the echo-tail view):
/// the engine publishes one 0..1 number per block; the UI keeps the
/// ring and scrolls it.
pub struct History {
    pub probe: &'static str,
}
impl History {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::History { probe: self.probe })
    }
}

/// Conditional visibility, typed: `page.show_when(&p.algo, 2)` shows
/// the subtree only while that stepped param reads 2. In markup:
/// `<Column show_when={(&p.algo, 2)}>…</Column>` (import this trait).
/// Applies to containers — the platform host realizes the node as a
/// real view and toggles it from the live control value, so host
/// automation flips pages just like a click on the `Tabs` bar.
pub trait ShowWhen: Sized {
    fn show_when(self, param: &dyn AnyParam, index: usize) -> Self;
    /// Tuple form for `ui!` attrs (the `border_t`/`shadow_t` pattern).
    fn show_when_t(self, t: (&dyn AnyParam, usize)) -> Self {
        self.show_when(t.0, t.1)
    }
    /// Page visibility bound to a VIEW key (editor-local, never the
    /// session): `show_view={(OP_PAGES, 2)}`.
    fn show_view(self, view: ViewKey, index: usize) -> Self;
    fn show_view_t(self, t: (ViewKey, usize)) -> Self {
        self.show_view(t.0, t.1)
    }
}

/// A view-state handle: which page of a purely-visual pager shows.
/// Editor-local by TYPE — no param behind it, so it cannot automate,
/// save, or appear in a preset (gap 8's structural close). Projects
/// pick small ids, unique within one editor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ViewKey(pub u16);

/// Tab bar over a view key — the pager for pages that are VIEWS,
/// not sounds (opal's operator pages). For pages that ARE sounds
/// (eddy's algorithms) use [`Tabs`]: those should automate.
pub struct ViewTabs {
    pub view: ViewKey,
    pub labels: &'static [&'static str],
}
impl ViewTabs {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::ViewTabs {
            view: self.view.0,
            labels: self.labels,
        })
    }
}
impl ShowWhen for UiNode {
    fn show_view(self, view: ViewKey, index: usize) -> Self {
        self.when_key(
            crate::editor::VIEW_WHEN_BIT | u32::from(view.0),
            index as u16,
        )
    }
    fn show_when(self, param: &dyn AnyParam, index: usize) -> Self {
        self.when_key(param.meta().id.0, index as u16)
    }
}
impl Meter {
    pub fn node(self) -> UiNode {
        self.node_with(MeterStyle::Dots)
    }

    /// Select the meter's visual treatment without introducing a separate
    /// widget type. Compose multiple meters for multichannel displays.
    pub fn node_with(self, style: MeterStyle) -> UiNode {
        stave_ui::widget(Widget::Meter {
            probe: self.probe,
            style,
        })
    }
}

/// Operating-point pad (doubles as the phosphor scope).
pub struct Pad<'a> {
    pub x: &'a dyn AnyParam,
    pub y: &'a dyn AnyParam,
}
impl Pad<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Pad {
            x: self.x.meta().id,
            y: self.y.meta().id,
        })
    }
}

/// The mod-ring declaration: pass the route slot params, the
/// target→knob map, and the per-slot live probes. Targeted knobs
/// light their rings; everything else is automatic.
pub struct ModRings<'a> {
    /// (source, target, depth) params per slot.
    pub slots: &'a [(&'a dyn AnyParam, &'a dyn AnyParam, &'a dyn AnyParam)],
    /// Target index → the knob it rings (None = no knob).
    pub targets: &'a [Option<&'a dyn AnyParam>],
    /// Per-slot live source values (engine-published, ±1).
    pub probes: &'static [&'static str],
    /// Per-slot shaping params (via, mode, lag, quant) — click a
    /// cable and the INSPECTOR edits these. Empty slice = none.
    pub extras: &'a [SlotShaping<&'a dyn AnyParam>],
}
impl ModRings<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::ModRings {
            slots: self
                .slots
                .iter()
                .map(|(s, t, d)| (s.meta().id, t.meta().id, d.meta().id))
                .collect(),
            targets: self
                .targets
                .iter()
                .map(|t| t.map(|p| p.meta().id))
                .collect(),
            probes: self.probes.to_vec(),
            extras: self.extras.iter().map(|e| e.map(|p| p.meta().id)).collect(),
        })
    }
}

/// The modulation PATCH BAY row: one
/// OUTLET jack per source (labels from the src param's own steps)
/// plus labeled INLET jacks for targets with no knob (PITCH).
/// Ringed knobs grow their inlets automatically from the paired
/// [`ModRings`] declaration; routes are dragged as cables and stay
/// ordinary slot params underneath.
pub struct ModSources {}
impl ModSources {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::ModSources)
    }
}

/// A bespoke picture (the OPEN kind): declare the bindings,
/// supply the pure build fn, done — no framework changes. This is
/// where project displays START; the kit is for second-consumer
/// graduates (EnvelopeDisplay below is one).
pub struct SceneDisplay<'a> {
    pub params: &'a [&'a dyn AnyParam],
    pub probes: &'static [&'static str],
    pub build: stave_ui::scene::SceneBuild,
    /// The optional editor half — None for read-only pictures.
    pub drag: Option<stave_ui::scene::SceneDrag>,
}
impl SceneDisplay<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Scene {
            params: self.params.iter().map(|p| p.meta().id).collect(),
            probes: self.probes.to_vec(),
            build: self.build,
            drag: self.drag,
            drag_pair: None,
        })
    }

    /// As `node`, with XY handles: `pair` is tried on every grab (a
    /// node on a plane moves its x and y params in one gesture);
    /// grabs it declines fall through to `drag`.
    pub fn node_xy(self, pair: stave_ui::scene::SceneDragPair) -> UiNode {
        let mut n = self.node();
        if let stave_ui::Kind::Widget(Widget::Scene { drag_pair, .. }) = &mut n.kind {
            *drag_pair = Some(pair);
        }
        n
    }
}

/// The ADSR display — kit-level because it earned it (every
/// synth; two consumers at birth), and itself just sugar over the
/// open Scene kind: four params in, `scenes::envelope` out.
pub struct EnvelopeDisplay<'a> {
    pub attack: &'a dyn AnyParam,
    pub decay: &'a dyn AnyParam,
    pub sustain: &'a dyn AnyParam,
    pub release: &'a dyn AnyParam,
}

fn envelope_build(inp: &stave_ui::scene::SceneInputs<'_>) -> stave_ui::scene::Scene {
    let a = inp.palette.accent;
    stave_ui::scenes::envelope::scene(
        &stave_ui::scenes::envelope::EnvState {
            attack_ms: inp.params[0] as f32,
            decay_ms: inp.params[1] as f32,
            sustain: inp.params[2] as f32,
            release_ms: inp.params[3] as f32,
            origin: stave_ui::scene::pt(1.0, 1.0),
            w: inp.w - 2.0,
            h: inp.h - 2.0,
        },
        &stave_ui::scenes::envelope::EnvStyle {
            line: a,
            fill: stave_ui::style::Color::rgba(a.r, a.g, a.b, 0.14),
            axis: inp.palette.axis,
        },
    )
}

impl EnvelopeDisplay<'_> {
    pub fn node(self) -> UiNode {
        SceneDisplay {
            params: &[self.attack, self.decay, self.sustain, self.release],
            probes: &[],
            build: envelope_build,
            drag: Some(stave_ui::scenes::envelope::drag),
        }
        .node()
    }
}

pub struct Harmonics {}
impl Harmonics {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Harmonics)
    }
}

pub struct Topology {
    pub pills: alloc::vec::Vec<crate::editor::TapPill>,
}
impl Topology {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Topology(self.pills))
    }
}

pub struct PresetBar {
    pub presets: Vec<Preset>,
}
impl PresetBar {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::PresetBar {
            presets: self.presets,
        })
    }
}

/// Two live probes representing a stereo signal. Probe names are explicit so
/// projects with project-owned telemetry do not need adapter widgets.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StereoMeterBinding {
    pub left_probe: &'static str,
    pub right_probe: &'static str,
}

impl StereoMeterBinding {
    pub const INPUT: Self = Self {
        left_probe: "io_in_l",
        right_probe: "io_in_r",
    };
    pub const OUTPUT: Self = Self {
        left_probe: "io_out_l",
        right_probe: "io_out_r",
    };
}

/// One side of the canonical plugin top bar. Metering and gain control are
/// independent: processors may expose a meter without inventing an input or
/// output trim parameter, while instruments may omit the input side entirely.
pub struct PluginTopBarPort<'a> {
    pub label: &'static str,
    pub level: Option<&'a dyn AnyParam>,
    pub meter: StereoMeterBinding,
}

impl<'a> PluginTopBarPort<'a> {
    pub fn input(level: Option<&'a dyn AnyParam>) -> Self {
        Self {
            label: "INPUT",
            level,
            meter: StereoMeterBinding::INPUT,
        }
    }

    pub fn output(level: Option<&'a dyn AnyParam>) -> Self {
        Self {
            label: "OUTPUT",
            level,
            meter: StereoMeterBinding::OUTPUT,
        }
    }

    fn meter_node(&self) -> UiNode {
        stave_ui::row(alloc::vec![
            Meter {
                probe: self.meter.left_probe,
            }
            .node_with(MeterStyle::Segments)
            .size(4.0, 44.0),
            Meter {
                probe: self.meter.right_probe,
            }
            .node_with(MeterStyle::Segments)
            .size(4.0, 44.0),
        ])
        .size(10.0, 44.0)
        .gap(2.0)
        .align(stave_ui::Align::Center)
    }

    fn control_node(&self) -> Option<UiNode> {
        let parameter = self.level?;
        let label_style = stave_ui::TextStyle {
            role: stave_ui::TypeRole::Tick,
            size: 8.0,
            kern: 1.2,
            color: stave_ui::tokens::SHELL_CONTROL_INK,
        };
        Some(
            stave_ui::column(alloc::vec![
                stave_ui::text(self.label).text_style(label_style),
                Knob {
                    param: parameter,
                    band: None,
                }
                .node_with_accent(KnobParts::CONTROL_ONLY, KnobAccent::Neutral)
                .size(36.0, 36.0),
            ])
            .size(44.0, 52.0)
            .gap(0.0)
            .align(stave_ui::Align::Center)
            .justify(stave_ui::Justify::Center),
        )
    }

    fn node(&self, meter_first: bool) -> UiNode {
        let mut children = Vec::new();
        if meter_first {
            children.push(self.meter_node());
        }
        if let Some(control) = self.control_node() {
            children.push(control);
        }
        if !meter_first {
            children.push(self.meter_node());
        }
        stave_ui::row(children)
            .size(110.0, 52.0)
            .gap(8.0)
            .align(stave_ui::Align::Center)
    }
}

/// Canonical 64-point plugin chrome. Projects provide their typed preset list
/// and only the I/O bindings they actually own; the shell owns proportions,
/// navigation, metering treatment, and neutral trim presentation.
pub struct PluginTopBar<'a> {
    pub presets: Vec<Preset>,
    pub input: Option<PluginTopBarPort<'a>>,
    pub output: Option<PluginTopBarPort<'a>>,
    pub bypass: Option<&'a dyn AnyParam>,
}

impl PluginTopBar<'_> {
    pub fn node(self) -> UiNode {
        let left_content = self
            .input
            .as_ref()
            .map(|port| port.node(true))
            .unwrap_or_else(|| stave_ui::spacer().size(110.0, 52.0));
        let left = stave_ui::row(alloc::vec![left_content])
            .size(150.0, 52.0)
            .align(stave_ui::Align::Center);

        let mut right_children = Vec::new();
        if let Some(bypass) = self.bypass {
            right_children.push(Power { param: bypass }.node().size(28.0, 28.0));
        }
        if let Some(output) = self.output.as_ref() {
            right_children.push(output.node(false));
        }
        let right = stave_ui::row(right_children)
            .size(150.0, 52.0)
            .gap(8.0)
            .align(stave_ui::Align::Center)
            .justify(stave_ui::Justify::End);

        let bar = stave_ui::row(alloc::vec![
            left,
            stave_ui::spacer(),
            PresetBar {
                presets: self.presets,
            }
            .node()
            .size(
                stave_ui::tokens::EDITOR_PRESET_BAR_WIDTH,
                stave_ui::tokens::EDITOR_TOP_BAR_CONTROL_HEIGHT,
            ),
            stave_ui::spacer(),
            right,
        ])
        .width(stave_ui::Dim::Fill(1.0))
        .height(stave_ui::Dim::Px(
            stave_ui::tokens::EDITOR_TOP_BAR_HEIGHT - 1.0,
        ))
        .pad_edges(stave_ui::Edges {
            t: 0.0,
            r: 20.0,
            b: 0.0,
            l: 20.0,
        })
        .gap(8.0)
        .align(stave_ui::Align::Center);

        stave_ui::column(alloc::vec![
            bar,
            stave_ui::shape(stave_ui::Shape::Line)
                .width(stave_ui::Dim::Fill(1.0))
                .height(stave_ui::Dim::Px(1.0))
                .bg(stave_ui::tokens::LINE),
        ])
        .width(stave_ui::Dim::Fill(1.0))
        .height(stave_ui::Dim::Px(stave_ui::tokens::EDITOR_TOP_BAR_HEIGHT))
    }
}

pub struct Power<'a> {
    pub param: &'a dyn AnyParam,
}
impl Power<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::Power(self.param.meta().id))
    }
}

/// The transfer plot; `style`/`knee` optional, GR by probe name.
pub struct Plot<'a> {
    pub threshold: &'a dyn AnyParam,
    pub ratio: &'a dyn AnyParam,
    pub makeup: &'a dyn AnyParam,
    pub style: Option<(&'a dyn AnyParam, &'static [&'static str])>,
    pub knee: Option<&'a dyn AnyParam>,
    pub gr_probe: &'static str,
    pub band: usize,
}
impl Plot<'_> {
    pub fn node(self) -> UiNode {
        stave_ui::widget(Widget::TransferPlot {
            keys: TransferKeys {
                threshold: self.threshold.meta().id,
                ratio: self.ratio.meta().id,
                makeup: self.makeup.meta().id,
                style: self.style.map(|(p, _)| p.meta().id),
                knee: self.knee.map(|p| p.meta().id),
                gr_probe: self.gr_probe,
                style_labels: self.style.map(|(_, l)| l).unwrap_or(&[]),
            },
            band: self.band,
        })
    }
}

/// The analyser strip; slices of params per overlay family.
pub struct Spectrum<'a> {
    pub splits: &'a [&'a dyn AnyParam],
    pub thresholds: &'a [&'a dyn AnyParam],
    pub sc_hpfs: &'a [&'a dyn AnyParam],
    pub gr_probes: &'static [&'static str],
}
impl Spectrum<'_> {
    pub fn node(self) -> UiNode {
        let ids = |ps: &[&dyn AnyParam]| -> Vec<crate::params::ParamId> {
            ps.iter().map(|p| p.meta().id).collect()
        };
        stave_ui::widget(Widget::Spectrum(SpectrumKeys {
            splits: ids(self.splits),
            thresholds: ids(self.thresholds),
            sc_hpfs: ids(self.sc_hpfs),
            gr_probes: self.gr_probes.to_vec(),
        }))
    }
}

/// The easy preset declaration: `preset("TWEED", &[(&p.drive, 5.5),
/// (&p.character, 1.0), …])` — typed params in, wire ids out.
/// The house grid cell (knob + label + readout footprint).
pub const CELL_W: f64 = 100.0;
pub const CELL_H: f64 = 92.0;

/// The card caption style (tracked caps over the card body).
pub const CARD_CAPTION: crate::ui::TextStyle = crate::ui::TextStyle {
    role: crate::ui::TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: crate::ui::tokens::INK_MUTED,
};

/// The section card: tracked-caps caption over an
/// N-column cell grid, FIXED width (a Fill inside an intrinsic card
/// degenerates to zero — the storybook's collapsed-chips bug, made
/// unrepresentable here).
pub fn card(caption: &'static str, cells: alloc::vec::Vec<UiNode>, cols: u16) -> UiNode {
    let w = f64::from(cols) * CELL_W + 24.0;
    crate::ui::column(alloc::vec![
        crate::ui::text(caption).text_style(CARD_CAPTION),
        crate::ui::grid(cols, cells),
    ])
    .width(crate::ui::Dim::Px(w))
    .pad(12.0)
    .gap(6.0)
    .bg(crate::ui::tokens::BG_1)
    .radius(crate::ui::tokens::RADIUS_MD)
    .border(1.0, crate::ui::tokens::LINE)
}

/// A fixed-width card with a free-form column body (pagers,
/// displays — anything not on the cell grid).
pub fn wide_card(caption: &'static str, w: f64, body: alloc::vec::Vec<UiNode>) -> UiNode {
    let mut children = alloc::vec![crate::ui::text(caption).text_style(CARD_CAPTION)];
    children.extend(body);
    crate::ui::column(children)
        .width(crate::ui::Dim::Px(w))
        .pad(12.0)
        .gap(10.0)
        .bg(crate::ui::tokens::BG_1)
        .radius(crate::ui::tokens::RADIUS_MD)
        .border(1.0, crate::ui::tokens::LINE)
}

pub fn preset(name: &'static str, pairs: &[(&dyn AnyParam, f64)]) -> Preset {
    Preset {
        name,
        values: pairs.iter().map(|(p, v)| (p.meta().id, *v)).collect(),
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use crate::params::{FloatParam, Range, Taper};

    fn widgets<'a>(node: &'a UiNode, found: &mut Vec<&'a Widget>) {
        if let stave_ui::Kind::Widget(widget) = &node.kind {
            found.push(widget);
        }
        for child in &node.children {
            widgets(child, found);
        }
    }

    /// `show_when` carries the param's id and the page index into the
    /// node — the key the platform host resolves and the editor bindings
    /// verify (an unresolvable page would silently never show).
    #[test]
    fn show_when_marks_the_page_with_the_param_key() {
        let p = FloatParam::new(7, "x", 0.0, Range::new(0.0, 1.0, Taper::Linear));
        let node = stave_ui::column(alloc::vec::Vec::new()).show_when(&p, 2);
        assert_eq!(node.when, Some((7, 2)));
    }

    #[test]
    fn plugin_top_bar_preserves_typed_bindings_and_explicit_probe_names() {
        let input = FloatParam::new(1, "input", 0.0, Range::new(-24.0, 24.0, Taper::Linear));
        let output = FloatParam::new(2, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear));
        let bypass = FloatParam::new(3, "bypass", 0.0, Range::new(0.0, 1.0, Taper::Linear));
        let declared = preset("FACTORY / CLEAN", &[(&input, 3.0), (&output, -2.0)]);

        let tree = PluginTopBar {
            presets: alloc::vec![declared.clone()],
            input: Some(PluginTopBarPort {
                label: "SEND",
                level: Some(&input),
                meter: StereoMeterBinding {
                    left_probe: "send_l",
                    right_probe: "send_r",
                },
            }),
            output: Some(PluginTopBarPort::output(Some(&output))),
            bypass: Some(&bypass),
        }
        .node();

        let mut found = Vec::new();
        widgets(&tree, &mut found);
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::PresetBar { presets } if presets == &alloc::vec![declared.clone()]
        )));
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::Knob { id, accent: KnobAccent::Neutral, .. } if *id == input.meta().id
        )));
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::Knob { id, accent: KnobAccent::Neutral, .. } if *id == output.meta().id
        )));
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::Meter {
                probe: "send_l",
                style: MeterStyle::Segments
            }
        )));
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::Meter {
                probe: "send_r",
                style: MeterStyle::Segments
            }
        )));
        assert!(found.iter().any(|widget| matches!(
            widget,
            Widget::Power(id) if *id == bypass.meta().id
        )));
    }
}

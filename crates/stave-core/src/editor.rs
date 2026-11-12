//! `EditorTree`: the
//! editor as a DECLARED component tree. Everything the framework's
//! generic editor used to hardcode per project (multiband band cards,
//! crossover footer, spectrum keying, forge engine strip) is data
//! here, by ParamId. Parity with the hardcoded layouts is pinned in
//! `stave-test/tests/editor_layout.rs` until the 7.4 flip deletes
//! them.

extern crate alloc;
use alloc::vec::Vec;

use crate::params::ParamId;

/// Params that drive a card's transfer-curve plot (the GR probe stays
/// a probe — probes are a debug-bus concept, not params).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferKeys {
    pub threshold: ParamId,
    pub ratio: ParamId,
    pub makeup: ParamId,
    pub style: Option<ParamId>,
    pub knee: Option<ParamId>,
    /// The gain-reduction PROBE feeding the live operating-point dot
    /// (probes are plugin-published telemetry; the name is its API).
    pub gr_probe: &'static str,
    /// Labels for the style selector (declaration data: chrome-
    /// fidelity params are often continuous floats with no wire
    /// steps).
    pub style_labels: &'static [&'static str],
}

/// Params the spectrum strip reads for overlays: crossover splits,
/// per-band thresholds, per-band sidechain HPFs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpectrumKeys {
    pub splits: Vec<ParamId>,
    pub thresholds: Vec<ParamId>,
    pub sc_hpfs: Vec<ParamId>,
    /// Per-band gain-reduction probes for the overlay readouts.
    pub gr_probes: Vec<&'static str>,
}

/// Leaf-widget payload for the box layer: what a `Node`
/// can mount, by ParamId — `stave-ui` stays ignorant of params.
/// `UiNode::when` keys with this bit set are VIEW keys (editor-local
/// page state), not param ids; `Params::validate` reserves the range.
pub const VIEW_WHEN_BIT: u32 = 0x8000_0000;

/// Optional decorations around the shared knob control. Layouts can compose
/// their own labels and readouts while retaining the same knob renderer and
/// interaction behavior.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KnobParts {
    pub label: bool,
    pub value: bool,
}

impl KnobParts {
    pub const FULL: Self = Self {
        label: true,
        value: true,
    };
    pub const CONTROL_ONLY: Self = Self {
        label: false,
        value: false,
    };
}

/// Accent treatment for an unbanded knob. Band-colored knobs continue to use
/// their declared band; shell compositions can choose a neutral treatment.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KnobAccent {
    ThemeAccent,
    Neutral,
}

/// Visual treatment for the shared level-meter renderer. Channel count is a
/// layout concern: stereo meters are composed from two ordinary meters.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MeterStyle {
    Dots,
    Segments,
}

/// A modulation slot's shaping params, as the cable inspector edits
/// them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SlotShaping<T> {
    pub via: T,
    pub mode: T,
    pub lag: T,
    pub quant: T,
}

impl<T> SlotShaping<T> {
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> SlotShaping<U> {
        SlotShaping {
            via: f(self.via),
            mode: f(self.mode),
            lag: f(self.lag),
            quant: f(self.quant),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Widget {
    /// Design-system knob; `band` picks the accent palette slot.
    Knob {
        id: ParamId,
        band: Option<usize>,
        parts: KnobParts,
        accent: KnobAccent,
    },
    /// Positional-role toggle (solo = accent tint, mute = warn);
    /// `band` picks the accent palette slot.
    Toggle {
        id: ParamId,
        solo: bool,
        band: Option<usize>,
    },
    /// Dropdown selector; empty `labels` = use the control's steps.
    /// `pill` = compact header style; false = grid-cell nameplate.
    Selector {
        id: ParamId,
        labels: &'static [&'static str],
        pill: bool,
    },
    /// Per-band transfer plot + live GR dot; `band` = accent slot.
    TransferPlot { keys: TransferKeys, band: usize },
    /// The header power/bypass chip (host-visible bypass param).
    Power(ParamId),
    /// The spectrum analyser strip with declared overlays.
    Spectrum(SpectrumKeys),
    /// Level meter fed by a plugin-published probe.
    Meter {
        probe: &'static str,
        style: MeterStyle,
    },
    /// Operating-point pad / phosphor scope (x = bias, y = drive).
    Pad { x: ParamId, y: ParamId },
    /// Harmonic-ladder view (probe-published curve).
    Harmonics,
    /// Live topology strip: pill declarations consume the stable tap
    /// wires + the params' OWN step names (labels derive from
    /// declarations, never from wire-name parsing or engine tables).
    Topology(Vec<TapPill>),
    /// Factory-preset picker backed by the plugin's typed declarations.
    PresetBar { presets: Vec<Preset> },
    /// Periodic-waveform display: one cycle of the bus-published
    /// curve (`publish_curve`) with a live phase cursor fed by
    /// `probe` (0..1 through the cycle). The LFO view.
    Wave { probe: &'static str },
    /// Segmented tab bar bound to a stepped param — the selector that
    /// drives `show_when` pages. The param is host-visible, so the
    /// tab choice automates and saves like any other control.
    Tabs {
        id: ParamId,
        labels: &'static [&'static str],
    },
    /// Segmented tab bar bound to a VIEW key — pure editor chrome
    /// (which page you're LOOKING at, not what the plugin sounds
    /// like). View state never reaches the session:
    /// no param, no automation, no state chunk, by construction.
    ViewTabs {
        view: u16,
        labels: &'static [&'static str],
    },
    /// Scrolling recent-history strip of a probe (0..1): each UI tick
    /// samples `latest` into a client-side ring — the engine publishes
    /// one number, never a buffer. The echo-tail / envelope view.
    History { probe: &'static str },
    /// Draggable modulation-source chips: drag one onto a
    /// targeted knob to create a route — sugar writing the SAME slot
    /// params the matrix table edits. Reads the editor's ModRings
    /// declaration (chip labels = the source enum's step names;
    /// drop targets = the rings' target map). Zero payload: one
    /// declaration powers the table, the rings, and the drag.
    ModSources,
    /// The mod-ring declaration: which KNOB each route
    /// target touches, so targeted knobs wear the ring (depth arc +
    /// live dot — the Vital lesson). Zero-size: it declares, the
    /// knobs render. `targets[k]` = the knob param for target index
    /// k (None = no knob to ring, e.g. chime's PITCH); `probes[i]` =
    /// the engine-published live SOURCE value of slot i (±1).
    ModRings {
        slots: Vec<(ParamId, ParamId, ParamId)>,
        targets: Vec<Option<ParamId>>,
        probes: Vec<&'static str>,
        /// Per-slot shaping params — the cable INSPECTOR's bindings
        /// Empty = no shaping declared; the inspector
        /// simply doesn't open.
        extras: Vec<SlotShaping<ParamId>>,
    },
    /// The OPEN display kind: the project (or the kit)
    /// supplies a pure picture function; the framework owns the
    /// bindings, the edge-triggered repaint, and the renderer.
    /// Bespoke visuals START HERE, project-local — promotion to a
    /// named kit component happens when a second consumer exists,
    /// never before (the earning criterion, structural).
    Scene {
        params: Vec<ParamId>,
        probes: Vec<&'static str>,
        build: stave_ui::scene::SceneBuild,
        /// Optional pure interaction: drags write the declared
        /// params through ordinary gestures. None = display-only.
        drag: Option<stave_ui::scene::SceneDrag>,
        /// Optional two-param interaction (XY handles); tried before
        /// `drag` on every grab.
        drag_pair: Option<stave_ui::scene::SceneDragPair>,
    },
}

/// One pill of the topology strip: which tap feeds its level
/// and how its label derives from declared params.
#[derive(Clone, Debug, PartialEq)]
pub struct TapPill {
    /// Stable tap wire name ("tap0"…). Carries VALUES only — labels
    /// never ride in wire names.
    pub tap: &'static str,
    pub label: TapLabel,
    /// Mode-keyed overrides: when `selector`'s current step equals
    /// the index, that label wins (later entries beat earlier).
    pub overrides: Vec<(ParamId, u32, TapLabel)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TapLabel {
    Fixed(&'static str),
    /// The current step name of a stepped param.
    StepOf(ParamId),
    /// Step name with a suffix (e.g. character + " PP").
    StepSuffixed(ParamId, &'static str),
}

/// The composable editor node — `stave-ui`'s tree with Stave leaves.
pub type UiNode = stave_ui::Node<Widget>;

/// A factory preset — DECLARED against typed params (see
/// `editor_kit::preset`): plain values keyed by `ParamId`.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub values: Vec<(ParamId, f64)>,
}

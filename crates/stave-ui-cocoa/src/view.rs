use core::ffi::c_void;
use core::ptr;

extern crate alloc;
use alloc::{boxed::Box, sync::Arc, vec::Vec};

use super::*;
use crate::theme;
use stave_core::DebugBus;

// ============================================================================
// PluginEditorView — per-instance state owning the sliders and timer.
// ============================================================================

/// One per band card. The view is an NSImageView whose CGImage we
/// rewrite each tick; the control indices and probe index let us
/// re-derive the curve shape + operating point without searching the
/// bus by name every frame.
use crate::widgets::{
    history::HistoryView, tabs::TabsState, transfer::TransferCurve, wave::WaveView,
};

pub struct PluginEditorView {
    pub(crate) bus: Arc<DebugBus>,
    /// Project-OWNED content tree (7.5c) — solved + mounted verbatim.
    pub(crate) ui: Option<stave_core::UiNode>,
    /// Spectrum strip width in points (set at mount; repaint uses it).
    pub(crate) spectrum_strip_w: f64,
    /// (control_index, NSSlider, NSTextField for value readout).
    pub(crate) sliders: Vec<(usize, Id, Id)>,
    /// Per-band transfer curves (compressor input→output plot) rendered
    /// into an NSImageView and refreshed each tick with the current
    /// threshold/ratio/makeup + a live operating-point dot driven by
    /// the band's GR probe.
    pub(crate) transfer_curves: Vec<TransferCurve>,
    pub(crate) waves: Vec<WaveView>,
    /// Probe-history strips (echo tails, envelopes).
    pub(crate) histories: Vec<HistoryView>,
    /// Tab bars + their `show_when` pages (see widgets::tabs).
    pub(crate) tabs: TabsState,
    /// Per-band card NSView + SOLO control index, used to dim non-soloed
    /// cards and corresponding spectrum regions when any band is soloed.
    pub(crate) band_cards: [Option<(Id, usize)>; 3],
    /// SOLO/MUTE buttons: (control_index, NSButton). Polled each tick;
    /// bus is authoritative — we push bus → button.setState. User
    /// clicks flow through `toggle_target` below back into the bus.
    pub(crate) toggle_buttons: Vec<(usize, Id)>,
    /// Style-cycle buttons (control_index, NSButton). Click rotates the
    /// param value 0→1→2→3→0; tick rewrites the title to the active
    /// style name (VCA / FET / OPTO / VMU).
    pub(crate) style_buttons: Vec<(usize, Id)>,
    /// Saturation type cycle buttons. Same shape as style_buttons but
    /// titled with TUBE / TAPE / XSTR / CLIP.
    pub(crate) sat_type_buttons: Vec<(usize, Id)>,
    /// Generic stepped-control dropdowns (control_index, NSPopUpButton)
    /// — any stepped param renders as one; items are its step names.
    pub(crate) stepped_dropdowns: Vec<(usize, Id)>,
    /// Phosphor-scope trail (engine view): recent capture snapshots of
    /// (shaper-domain input, chain output) pairs, newest first. Only
    /// used when the plugin owns the capture.
    pub(crate) scope_trail: core::cell::RefCell<ScopeTrail>,
    /// Harmonic-ladder ghost peaks (decaying max-hold per harmonic).
    pub(crate) harm_ghost: core::cell::RefCell<[f64; 8]>,
    /// Topology-strip freshness: (last hits, ticks since change) per
    /// probe slot — pills drop out when their probe goes stale.
    pub(crate) topo_age: core::cell::RefCell<[(u64, u32); stave_core::debug::MAX_PROBES]>,
    /// Topology pill declarations from the owned tree: stable
    /// tap wires + param-derived labels. Empty = no strip mounted.
    pub(crate) topo_pills: core::cell::RefCell<alloc::vec::Vec<stave_core::TapPill>>,
    /// One NSObject of class StaveToggleTarget per editor — the click
    /// target for every SOLO/MUTE button. Holds our self_ptr in an
    /// ivar so its `click:` method can find the bus.
    pub(crate) toggle_target: Id,
    /// Optional spectrum analyser image view + cached scratch buffers.
    pub(crate) spectrum_view: Id,
    /// Persistent (smoothed) dB values per bin so the spectrum doesn't
    /// strobe at 30 Hz.
    pub(crate) spectrum_smooth: Vec<f32>,
    /// Indices of low/high crossover controls (for drawing markers).
    pub(crate) spectrum_low_xover_ctrl: Option<usize>,
    pub(crate) spectrum_high_xover_ctrl: Option<usize>,
    /// Per-band threshold control indices, used to overlay the
    /// compressor threshold as a horizontal line on the spectrum.
    pub(crate) spectrum_lo_thresh_ctrl: Option<usize>,
    pub(crate) spectrum_mid_thresh_ctrl: Option<usize>,
    pub(crate) spectrum_hi_thresh_ctrl: Option<usize>,
    /// Per-band sidechain HPF cutoff controls (used to draw a small
    /// dashed vertical line on the spectrum at the HPF freq so the
    /// user can see what's being filtered out of detection).
    pub(crate) spectrum_lo_sc_hpf_ctrl: Option<usize>,
    pub(crate) spectrum_mid_sc_hpf_ctrl: Option<usize>,
    pub(crate) spectrum_hi_sc_hpf_ctrl: Option<usize>,
    /// Live per-band GR readout overlays on the spectrum.
    pub(crate) spectrum_lo_gr_label: Id,
    pub(crate) spectrum_mid_gr_label: Id,
    pub(crate) spectrum_hi_gr_label: Id,
    pub(crate) spectrum_lo_gr_probe: Option<usize>,
    pub(crate) spectrum_mid_gr_probe: Option<usize>,
    pub(crate) spectrum_hi_gr_probe: Option<usize>,
    /// Sample rate snapshot used for log-freq mapping.
    pub(crate) spectrum_sample_rate: f32,
    /// CFRunLoopTimer / NSTimer used to poll. Kept alive so it doesn't drop.
    pub(crate) timer: Id,
    /// Wordmark + tagline labels — when bypass flips on, the tick loop
    /// dims the wordmark layer and rewrites the tagline so the brand
    /// itself signals the bypass state (Baby-Audio pattern).
    pub(crate) bypass_wordmark: Id,
    pub(crate) bypass_tagline: Id,
    pub(crate) bypass_ci: Option<usize>,
    /// Last rendered bypass state, used to skip redundant attributed-
    /// string rebuilds in the tick loop.
    pub(crate) bypass_last_on: bool,
    /// (ci, NSView*=StaveKnob, NSTextField*=value label, last_bits) for
    /// every custom knob in the editor. Tick polls the bus, calls
    /// setNeedsDisplay on the knob when value changes, and updates the
    /// value label string.
    pub(crate) knobs: Vec<(usize, Id, Id, u64)>,
    /// Frame I/O meters: (probe_idx, StaveMeter view) redrawn by the timer.
    pub(crate) meters: Vec<(usize, Id)>,
    /// Top-bar power button (glows when active, dims when bypassed).
    pub(crate) bypass_button: Id,
    /// Factory presets: (name, [(control index, plain value)]).
    pub(crate) presets: Vec<(alloc::string::String, Vec<(usize, f64)>)>,
    /// Scene-built display widgets: envelopes, algo diagrams.
    pub(crate) scene_widgets: crate::widgets::scene_widget::SceneWidgets,
    /// The mod-ring declaration + computed rings, if any.
    pub(crate) rings: Option<crate::widgets::rings::RingsState>,
    /// Themed dropdown fields + the (at most one) open overlay menu.
    pub(crate) dropdowns: crate::widgets::select_model::DropdownState,
    /// Patch-cable modulation: jacks, the overlay, a drag in flight.
    pub(crate) cables: crate::widgets::cables::CableState,
    /// The cable inspector (route shaping), when open.
    pub(crate) inspector: Option<crate::widgets::inspector::InspectorState>,
    /// Active drag anchor while the user is rotating a knob. Set on
    /// mouseDown:, read on mouseDragged:, cleared by mouseUp: (the
    /// drag-anchor's first slot is the control index).
    pub(crate) knob_drag_anchor: Option<(usize, f64, f64)>,
    /// Target instance pointer used by the timer (Self ptr, leaked into ObjC).
    pub(crate) self_ptr: *mut PluginEditorView,
    pub(crate) binding: super::binding::EditorBinding,
    /// A design-system face, when the plugin declares one (it replaces
    /// the `ui` tree).
    pub(crate) face: Option<Box<crate::face::FaceEditor>>,
}

impl PluginEditorView {
    pub fn new(
        bus: Arc<DebugBus>,
        table: Arc<dyn stave_core::params::Params>,
        host: Box<dyn super::binding::EditorHost>,
    ) -> Box<Self> {
        let mut me = Box::new(Self {
            bus,
            sliders: Vec::new(),
            transfer_curves: Vec::new(),
            waves: Vec::new(),
            histories: Vec::new(),
            tabs: TabsState::default(),
            band_cards: [None, None, None],
            toggle_buttons: Vec::new(),
            style_buttons: Vec::new(),
            sat_type_buttons: Vec::new(),
            stepped_dropdowns: Vec::new(),
            scope_trail: core::cell::RefCell::new(ScopeTrail {
                last_block: 0,
                frames: alloc::collections::VecDeque::new(),
            }),
            harm_ghost: core::cell::RefCell::new([0.0; 8]),
            topo_pills: core::cell::RefCell::new(alloc::vec::Vec::new()),
            topo_age: core::cell::RefCell::new(
                [(0u64, 1_000_000u32); stave_core::debug::MAX_PROBES],
            ),
            toggle_target: ptr::null_mut(),
            spectrum_view: ptr::null_mut(),
            spectrum_smooth: alloc::vec![-90.0_f32; 256],
            spectrum_low_xover_ctrl: None,
            spectrum_high_xover_ctrl: None,
            spectrum_lo_thresh_ctrl: None,
            spectrum_mid_thresh_ctrl: None,
            spectrum_hi_thresh_ctrl: None,
            spectrum_lo_sc_hpf_ctrl: None,
            spectrum_mid_sc_hpf_ctrl: None,
            spectrum_hi_sc_hpf_ctrl: None,
            spectrum_lo_gr_label: ptr::null_mut(),
            spectrum_mid_gr_label: ptr::null_mut(),
            spectrum_hi_gr_label: ptr::null_mut(),
            spectrum_lo_gr_probe: None,
            spectrum_mid_gr_probe: None,
            spectrum_hi_gr_probe: None,
            spectrum_sample_rate: 48_000.0,
            timer: ptr::null_mut(),
            bypass_wordmark: ptr::null_mut(),
            bypass_tagline: ptr::null_mut(),
            bypass_ci: None,
            bypass_last_on: false,
            knobs: Vec::new(),
            meters: Vec::new(),
            bypass_button: ptr::null_mut(),
            presets: Vec::new(),
            scene_widgets: Default::default(),
            rings: None,
            dropdowns: Default::default(),
            cables: Default::default(),
            inspector: None,
            knob_drag_anchor: None,
            self_ptr: ptr::null_mut(),
            binding: super::binding::EditorBinding { table, host },
            ui: None,
            spectrum_strip_w: 0.0,
            face: None,
        });
        me.self_ptr = &mut *me as *mut Self;
        me
    }

    pub fn bus_for_size(&self) -> &Arc<DebugBus> {
        &self.bus
    }

    /// Install the plugin's owned tree. Called at createView.
    pub fn set_ui(&mut self, ui: Option<stave_core::UiNode>) {
        self.ui = ui;
    }

    /// Build the slider widgets as subviews of `parent_ns_view`.
    /// Bridge between VST3 attached() and Cocoa.
    ///
    /// # Safety
    /// Main thread; `parent_ns_view` is null or a live NSView.
    pub unsafe fn attach(&mut self, parent_ns_view: Id) {
        if parent_ns_view.is_null() {
            return;
        }
        if self.param_count() == 0 {
            return;
        }
        if unsafe { self.attach_face(parent_ns_view) } {
            unsafe { self.start_timer() };
            return;
        }
        // Dark background on the host's NSView + create a flipped
        // (top-left-origin) container so our layout maths is intuitive.
        let parent_ns_view = unsafe {
            msg_void_int(parent_ns_view, sel(c"setWantsLayer:"), 1);
            let layer = msg_id(parent_ns_view, sel(c"layer"));
            if !layer.is_null() {
                // Deep base fill (in case the gradient ever fails to size),
                // then the modern slate gradient + vignette on top of it.
                let cg_color = make_cg_color(
                    theme::BG_GRAD_BOTTOM.0,
                    theme::BG_GRAD_BOTTOM.1,
                    theme::BG_GRAD_BOTTOM.2,
                    1.0,
                );
                let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                    core::mem::transmute(objc_msgSend as *const ());
                f(layer, sel(c"setBackgroundColor:"), cg_color);
                let bounds: CGRect = {
                    let g: unsafe extern "C" fn(Id, Sel) -> CGRect =
                        core::mem::transmute(objc_msgSend as *const ());
                    g(parent_ns_view, sel(c"bounds"))
                };
                install_background(layer, bounds);
            }
            let frame: CGRect = {
                let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
                    core::mem::transmute(objc_msgSend as *const ());
                f(parent_ns_view, sel(c"bounds"))
            };
            let flipped_cls = flipped_view_class();
            let raw = msg_id(flipped_cls, sel(c"alloc"));
            let container = msg_id_rect(raw, sel(c"initWithFrame:"), frame);
            msg_void_int(container, sel(c"setAutoresizingMask:"), 18); // width|height fill
            msg_void_id(parent_ns_view, sel(c"addSubview:"), container);
            container
        };
        if unsafe { super::mount::try_attach_owned(self, parent_ns_view) } {
            return;
        }
        // No owned tree: the bare dark container.
        unsafe { self.start_timer() };
    }
}

impl Drop for PluginEditorView {
    fn drop(&mut self) {
        unsafe { self.detach() };
    }
}

//! `Tabs` + `show_when` pages — the tabbed-view mechanism. The bar
//! is a row of house-styled chip buttons writing a stepped control;
//! pages are container views registered with (control, index) and
//! flipped by `setHidden:` from the live bus value each tick — so
//! host automation and preset loads turn pages exactly like a click.

extern crate alloc;
use alloc::vec::Vec;

use crate::objc::Id;
use crate::view::PluginEditorView;
use crate::*;

/// Everything the tab mechanism registers at mount, one view field.
#[derive(Default)]
pub(crate) struct TabsState {
    pub(crate) bars: Vec<TabBar>,
    pub(crate) pages: Vec<Page>,
    /// Editor-local page state (ViewTabs/show_view), keyed by view
    /// key. Never serialized — view state stays out of sessions by
    /// construction.
    pub(crate) view_state: alloc::collections::BTreeMap<u16, i64>,
}

/// What a bar or page follows: a live CONTROL (params — automation
/// and preset loads turn the page) or a VIEW key (editor-local
/// chrome, never the session).
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum Bind {
    Control(usize),
    View(u16),
}

/// One mounted tab bar: the binding and its chip buttons in segment
/// order. `last` caches the styled value (restyle on edge).
pub(crate) struct TabBar {
    pub(crate) bind: Bind,
    pub(crate) buttons: Vec<Id>,
    pub(crate) labels: &'static [&'static str],
    pub(crate) last: i64,
}

/// One `show_when`/`show_view` page: hidden unless its binding
/// reads `index`.
pub(crate) struct Page {
    pub(crate) view: Id,
    pub(crate) bind: Bind,
    pub(crate) index: u16,
}

/// Chip button tags carry (page index << 16 | key); the key's bit
/// 15 marks a VIEW binding (control indices sit far below 0x8000).
/// One i64 is all NSControl gives us; `tab_picked` reverses this.
pub(crate) const TAB_TAG_SHIFT: i64 = 16;
pub(crate) const TAB_VIEW_BIT: i64 = 0x8000;

/// Lay the bar: equal-width chips across `rect` with a 6pt gap.
pub(crate) unsafe fn add_tab_bar(
    v: &mut PluginEditorView,
    parent: Id,
    bind: Bind,
    rect: CGRect,
    labels: &'static [&'static str],
) {
    let key: i64 = match bind {
        Bind::Control(ci) => ci as i64,
        Bind::View(k) => TAB_VIEW_BIT | i64::from(k),
    };
    unsafe {
        let n = labels.len().max(1);
        let gap = 6.0;
        let w = (rect.size.width - gap * (n as f64 - 1.0)) / n as f64;
        let mut buttons = Vec::with_capacity(n);
        for (i, label) in labels.iter().enumerate() {
            let cls = class(c"NSButton");
            let raw = msg_id(cls, sel(c"alloc"));
            let r = CGRect {
                origin: CGPoint {
                    x: rect.origin.x + i as f64 * (w + gap),
                    y: rect.origin.y,
                },
                size: CGSize {
                    width: w,
                    height: rect.size.height,
                },
            };
            let btn = msg_id_rect(raw, sel(c"initWithFrame:"), r);
            msg_void_int(btn, sel(c"setButtonType:"), 2);
            msg_void_int(btn, sel(c"setBordered:"), 0);
            msg_void_int(btn, sel(c"setWantsLayer:"), 1);
            let lyr = msg_id(btn, sel(c"layer"));
            if !lyr.is_null() {
                msg_void_double(lyr, sel(c"setCornerRadius:"), 4.0);
                msg_void_double(lyr, sel(c"setBorderWidth:"), 1.0);
            }
            msg_void_int(btn, sel(c"setTag:"), ((i as i64) << TAB_TAG_SHIFT) | key);
            if !v.toggle_target.is_null() {
                msg_void_id(btn, sel(c"setTarget:"), v.toggle_target);
                let f_sel: unsafe extern "C" fn(Id, Sel, Sel) =
                    core::mem::transmute(objc_msgSend as *const ());
                f_sel(btn, sel(c"setAction:"), sel(c"tabPicked:"));
            }
            style_chip(btn, label, false);
            msg_void_id(parent, sel(c"addSubview:"), btn);
            buttons.push(btn);
        }
        // last = impossible value so the first tick styles the bar.
        v.tabs.bars.push(TabBar {
            bind,
            buttons,
            labels,
            last: -1,
        });
    }
}

/// Chip styling: active = accent ink + accent ring on the panel
/// ground; inactive = muted ink, hairline ring, transparent.
unsafe fn style_chip(btn: Id, label: &str, active: bool) {
    unsafe {
        let (ink, ring, ring_a, bg_a) = if active {
            (theme::ACCENT, theme::ACCENT, 0.9, 1.0)
        } else {
            (theme::INK_MUTED, theme::LINE, 0.8, 0.0)
        };
        let lyr = msg_id(btn, sel(c"layer"));
        if !lyr.is_null() {
            msg_void_ptr(
                lyr,
                sel(c"setBackgroundColor:"),
                make_cg_color(theme::BG_2.0, theme::BG_2.1, theme::BG_2.2, bg_a),
            );
            msg_void_ptr(
                lyr,
                sel(c"setBorderColor:"),
                make_cg_color(ring.0, ring.1, ring.2, ring_a),
            );
        }
        button_set_attributed_title(
            btn,
            label,
            ns_font_role(theme::role_label()),
            ns_color_token(ink),
            1.4,
        );
    }
}

/// The tick half: restyle bars on value edges; flip page visibility
/// from the live control value (bus is authoritative, as everywhere).
pub(crate) fn update_tabs(v: &mut PluginEditorView) {
    let read = |v: &PluginEditorView, bind: Bind| -> i64 {
        match bind {
            Bind::Control(ci) => v.plain(ci) as i64,
            Bind::View(k) => v.tabs.view_state.get(&k).copied().unwrap_or(0),
        }
    };
    // Split borrows: read state first, then style.
    for bi in 0..v.tabs.bars.len() {
        let cur = read(v, v.tabs.bars[bi].bind);
        let bar = &mut v.tabs.bars[bi];
        if cur != bar.last {
            bar.last = cur;
            for (i, &btn) in bar.buttons.iter().enumerate() {
                let label = bar.labels.get(i).copied().unwrap_or("");
                unsafe { style_chip(btn, label, i as i64 == cur) };
            }
        }
    }
    for pi in 0..v.tabs.pages.len() {
        let cur = read(v, v.tabs.pages[pi].bind);
        let page = &v.tabs.pages[pi];
        unsafe {
            msg_void_int(
                page.view,
                sel(c"setHidden:"),
                (cur != page.index as i64) as i64,
            )
        };
    }
}

impl PluginEditorView {
    /// Set a ViewTabs page programmatically (storybook page cycling,
    /// tests). The tick styles bars and flips pages from this state.
    pub fn set_view_page(&mut self, key: u16, index: i64) {
        self.tabs.view_state.insert(key, index);
    }
}

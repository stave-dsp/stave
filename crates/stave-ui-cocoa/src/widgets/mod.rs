//! Platform widget leaves — one file per widget. These are the
//! `Kind::Widget` escape hatches; each decomposes into
//! Shape/Glyph/Text compositions as the primitive layer matures.

pub(crate) mod cables;
pub(crate) mod cg;
pub(crate) mod dropdown;
pub(crate) mod engine;
pub(crate) mod history;
pub(crate) mod inspector;
pub(crate) mod knob;
pub(crate) mod knob_cell;
pub(crate) mod meter;
pub(crate) mod meter_cell;
pub(crate) mod pad;
pub(crate) mod power;
pub(crate) mod preset_bar;
pub(crate) mod rings;
pub(crate) mod scene_render;
pub(crate) mod scene_widget;
pub(crate) mod select_model;
pub(crate) mod select_render;
pub(crate) mod spectrum;
pub(crate) mod tabs;
pub(crate) mod targets;
pub(crate) mod toggle;
pub(crate) mod transfer;
pub(crate) mod wave;

//! stave's plugin host as a library: load VST3 plugins, drive them, and
//! measure them (through `stave-measure`). `stave-bench` (agents) and
//! `stave-play` (humans) are thin apps over this.
//!
//! ```text
//! plugin/    VST3 hosting → LoadedPlugin
//! instance   the Instance trait: the seam everything measures through
//! adapt      Instance ↔ System, and any stave-dsp Stage as an Instance
//! control    one parameter model (Control), lookup by name
//! modules    stave-dsp modules by name, loadable like plugins
//! note       note events and schedules
//! render     Driver: run an Instance from a Signal + notes
//! loaded     what a tool opens: a bundle or module:<name>
//! response   frequency response by log sweep (H1)
//! bench/     the bench: phases, reports, CLI parsing
//! validate/  the validation suite: one file per check
//! ```

pub mod adapt;
pub mod bench;
pub mod config;
pub mod control;
pub mod instance;
pub mod loaded;
pub mod modules;
pub mod note;
pub mod plugin;
pub mod render;
pub mod response;
pub mod validate;

#[cfg(test)]
pub(crate) mod testing;

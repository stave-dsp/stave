//! stave's plugin host as a library: load VST3 plugins, drive them, and
//! measure them (through `stave-measure`). `stave-tools` makes each
//! capability a command; the `stave` CLI and MCP server (agents) and
//! `stave-play` (humans) present them.
//!
//! ```text
//! plugin/    VST3 hosting → LoadedPlugin
//! au/        Audio Unit hosting on macOS → AuInstance (offline)
//! instance   the Instance trait: the seam everything measures through
//! adapt      Instance ↔ System, and any stave-dsp Stage as an Instance
//! param      one parameter model (Param, core's words), lookup by name
//! stages     stave-dsp stages by name, loadable like plugins
//! note       note schedules (of core's events)
//! counters   this thread's CPU time and instruction count
//! drive      Driver: run an Instance from a Signal + notes
//! opened     what a tool opens: a bundle, au:<name> or stage:<name>
//! response   frequency response by log sweep (H1)
//! bench/     the bench: phases, reports, the describe dump
//! validate/  the validation suite: one file per check
//! wav        WAV files in and out (planar f32)
//! ```

pub mod adapt;
#[cfg(target_os = "macos")]
pub mod au;
pub mod bench;
pub mod config;
pub mod counters;
pub mod instance;
pub mod opened;
pub mod param;
pub mod stages;
pub mod note;
pub mod plugin;
pub mod drive;
pub mod response;
pub mod validate;
pub mod wav;

#[cfg(test)]
pub(crate) mod testing;

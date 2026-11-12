//! The identity dump agents start from: what the plugin is, how it
//! loaded, its shape, and every parameter.

use crate::param::Param;
use crate::instance::{Instance, Layout, TapReading, Tail};
use crate::plugin::{HostStats, PluginInfo};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Description {
    /// Present for a hosted VST3 (identity + load trace).
    pub plugin: Option<PluginInfo>,
    pub host: Option<HostStats>,
    pub layout: Layout,
    pub latency: u32,
    pub tail: Tail,
    pub params: Vec<Param>,
    pub presets: Vec<String>,
    pub taps: Vec<TapReading>,
}

pub fn describe(inst: &dyn Instance) -> Description {
    Description {
        plugin: None,
        host: None,
        layout: inst.layout(),
        latency: inst.latency(),
        tail: inst.tail(),
        params: inst.params(),
        presets: crate::instance::presets(inst),
        taps: inst.taps(),
    }
}

/// [`describe`], plus the VST3 identity and load trace.
pub fn describe_plugin(p: &crate::plugin::LoadedPlugin) -> Description {
    Description {
        plugin: Some(p.info().clone()),
        host: Some(p.stats()),
        ..describe(p)
    }
}

/// Whatever a tool opened: a plugin with its identity, or a catalog stage.
pub fn describe_opened(loaded: &crate::opened::Opened) -> Description {
    match loaded {
        crate::opened::Opened::Plugin(p) => describe_plugin(p),
        crate::opened::Opened::Stage(m) => describe(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Delay, Gain};

    #[test]
    fn describes_shape_and_surface() {
        let g = Gain::new(2, 2).with_preset("warm", &[]);
        let d = describe(&g);
        assert_eq!(
            (d.layout.inputs, d.params.len()),
            (2, 3),
            "gain, bypass, program"
        );
        assert_eq!(d.presets, ["warm"]);
        assert!(d.plugin.is_none());
    }

    #[test]
    fn latency_and_tail_come_from_the_instance() {
        let d = describe(&Delay::new(1, 9));
        assert_eq!((d.latency, d.tail), (9, Tail::Samples(9)));
    }
}

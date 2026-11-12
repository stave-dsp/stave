//! A stave plugin's telemetry, read from the host side: taps and log
//! messages. Present only in stave-built plugins (`stave_telemetry_for`).

use stave_telemetry::Telemetry;

use crate::instance::{PluginMessage, Probe};
use stave_core::diagnostics::Severity;

/// Borrowed telemetry, valid while its plugin instance lives.
#[derive(Copy, Clone)]
pub(crate) struct TelemetryRef<'a>(pub &'a Telemetry);

impl<'a> TelemetryRef<'a> {
    pub fn probes(self) -> Vec<Probe> {
        self.0
            .taps()
            .into_iter()
            .map(|t| Probe {
                name: t.name,
                latest: t.latest,
                min: t.min,
                max: t.max,
            })
            .collect()
    }

    pub fn messages(self, from: u64) -> Vec<PluginMessage> {
        self.0
            .messages()
            .read_from(from)
            .filter_map(|(index, severity, text)| {
                Some(PluginMessage {
                    index,
                    severity: Severity::from_u8(severity)?,
                    text,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_telemetry::{Tap, TapId};

    #[test]
    fn probes_carry_latest_min_and_max() {
        let telemetry = Telemetry::new();
        let cpu = Tap::register(&telemetry, TapId::new("cpu"));
        for v in [0.5, -1.0, 2.0, 0.25] {
            cpu.set(v);
        }
        let p = &TelemetryRef(&telemetry).probes()[0];
        assert_eq!(
            (p.name.as_str(), p.latest, p.min, p.max),
            ("cpu", 0.25, -1.0, 2.0)
        );
    }

    #[test]
    fn empty_telemetry_has_nothing() {
        let telemetry = Telemetry::new();
        assert!(TelemetryRef(&telemetry).probes().is_empty());
    }
}

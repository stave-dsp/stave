//! stave's debug bus, read from the host side: probes and log messages.
//! Present only in stave-built plugins (`stave_debug_bus_for`).

use std::sync::atomic::Ordering;

use stave_debug::DebugBus;

use crate::instance::{PluginMessage, Probe};
use stave_core::diagnostics::Severity;

/// A borrowed bus, valid while its plugin instance lives.
#[derive(Copy, Clone)]
pub(crate) struct BusRef<'a>(pub &'a DebugBus);

fn f(bits: &std::sync::atomic::AtomicU64) -> f64 {
    f64::from_bits(bits.load(Ordering::Relaxed))
}

fn used(n: &std::sync::atomic::AtomicU16, cap: usize) -> usize {
    (n.load(Ordering::Acquire) as usize).min(cap)
}

impl<'a> BusRef<'a> {
    pub fn probes(self) -> Vec<Probe> {
        let b = self.0;
        b.probes[..used(&b.probes_used, b.probes.len())]
            .iter()
            .map(|p| Probe {
                name: p.name_str().to_string(),
                latest: f(&p.latest),
                min: f(&p.min),
                max: f(&p.max),
            })
            .collect()
    }

    pub fn messages(self, from: u64) -> Vec<PluginMessage> {
        self.0
            .messages
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
    use std::sync::atomic::AtomicU16;

    #[test]
    fn probes_carry_latest_min_and_max() {
        let bus = DebugBus::new();
        let slot = AtomicU16::new(u16::MAX);
        for v in [0.5, -1.0, 2.0, 0.25] {
            bus.probe_f64(&slot, "cpu", v);
        }
        let p = &BusRef(&bus).probes()[0];
        assert_eq!(
            (p.name.as_str(), p.latest, p.min, p.max),
            ("cpu", 0.25, -1.0, 2.0)
        );
    }

    #[test]
    fn empty_bus_has_nothing() {
        let bus = DebugBus::new();
        let b = BusRef(&bus);
        assert!(b.probes().is_empty());
    }
}

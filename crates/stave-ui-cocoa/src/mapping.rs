extern crate alloc;
use alloc::sync::Arc;

use stave_core::DebugBus;

// ============================================================================
// Normalize / denormalize honouring log skew.
// ============================================================================

/// A param's current value as text with units.
pub(crate) fn format_value(p: &dyn stave_core::params::AnyParam) -> alloc::string::String {
    let v = p.plain();
    let unit = p.meta().unit.label();
    // Sentinel unit "tube" → show the tube-type name instead of an index,
    // so a tube-selector control reads "12AX7" rather than "2.000".
    if unit == "tube" {
        const TUBES: [&str; 5] = ["12AX7", "12AT7", "12AU7", "6DJ8", "6SN7"];
        let i = (v.round().max(0.0) as usize).min(TUBES.len() - 1);
        return alloc::string::String::from(TUBES[i]);
    }
    // Sentinel unit "mem" → memory-type name instead of an index.
    if unit == "mem" {
        const MEMS: [&str; 4] = ["TUBE BIAS", "SAG", "OPTO", "TAPE"];
        let i = (v.round().max(0.0) as usize).min(MEMS.len() - 1);
        return alloc::string::String::from(MEMS[i]);
    }
    // Percent is a 0..=1 plain value rendered 0–100% (params::Unit).
    if unit == "%" {
        return alloc::format!("{:.0}%", v * 100.0);
    }
    let body = if v.abs() >= 10_000.0 {
        alloc::format!("{:.0}", v)
    } else if v.abs() >= 100.0 {
        alloc::format!("{:.1}", v)
    } else if v.abs() >= 10.0 {
        alloc::format!("{:.2}", v)
    } else {
        alloc::format!("{:.3}", v)
    };
    if unit.is_empty() {
        body
    } else {
        alloc::format!("{} {}", body, unit)
    }
}

/// Find a probe by name, RESERVING its slot (at 0.0) if the engine has
/// not published it yet — for displays that must draw before the first
/// audio block (a pedal face opened on a stopped transport). The engine
/// later writes the same slot by name. None only if the bus is full.
pub(crate) fn find_or_reserve_probe(bus: &Arc<DebugBus>, name: &'static str) -> Option<usize> {
    if let Some(i) = find_probe(bus, name) {
        return Some(i);
    }
    bus.probe_f64(&core::sync::atomic::AtomicU16::new(u16::MAX), name, 0.0);
    find_probe(bus, name)
}

pub(crate) fn find_probe(bus: &Arc<DebugBus>, name: &str) -> Option<usize> {
    let used = bus.probes_used.load(core::sync::atomic::Ordering::Acquire) as usize;
    bus.probes[..used.min(stave_core::debug::MAX_PROBES)]
        .iter()
        .position(|p| p.name_str() == name)
}

// Pure-math FFT used by the spectrum view (moved from view.rs —
// mapping.rs is the math module).
pub(crate) fn fft_radix2(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n < 2 || (n & (n - 1)) != 0 {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    // Cooley-Tukey butterflies.
    let mut size = 2usize;
    while size <= n {
        let half = size / 2;
        let table_step = n / size;
        for i in (0..n).step_by(size) {
            for k in 0..half {
                let wk = (k * table_step) as f32 * (-2.0 * core::f32::consts::PI / n as f32);
                let w_re = wk.cos();
                let w_im = wk.sin();
                let t_re = w_re * re[i + k + half] - w_im * im[i + k + half];
                let t_im = w_re * im[i + k + half] + w_im * re[i + k + half];
                re[i + k + half] = re[i + k] - t_re;
                im[i + k + half] = im[i + k] - t_im;
                re[i + k] += t_re;
                im[i + k] += t_im;
            }
        }
        size <<= 1;
    }
}

use super::*;
use core::sync::atomic::{AtomicU16, Ordering};

#[test]
fn inline_str_roundtrip_truncation_and_unset() {
    let s: InlineStr<8> = InlineStr::zeroed();
    assert_eq!(s.as_str(), "");
    assert!(!s.is_set());
    s.set("drive level"); // 11 bytes into capacity 8 → 7 + nul
    assert_eq!(s.as_str(), "drive l");
    assert!(s.is_set());
}

#[test]
fn probe_register_and_update() {
    let bus = DebugBus::new();
    let slot = AtomicU16::new(u16::MAX);
    bus.probe_f64(&slot, "phase", 0.25);
    bus.probe_f64(&slot, "phase", -1.5);
    assert_eq!(bus.probes_used.load(Ordering::Acquire), 1);
    let p = &bus.probes[0];
    assert_eq!(p.name_str(), "phase");
    assert_eq!(f64::from_bits(p.latest.load(Ordering::Relaxed)), -1.5);
    assert_eq!(f64::from_bits(p.min.load(Ordering::Relaxed)), -1.5);
    assert_eq!(f64::from_bits(p.max.load(Ordering::Relaxed)), 0.25);
    assert_eq!(p.hits.load(Ordering::Relaxed), 2);
}

#[test]
fn warm_slot_cache_does_not_starve_second_bus() {
    let shared_cache = AtomicU16::new(u16::MAX); // one callsite
    let bus_a = DebugBus::new();
    let bus_b = DebugBus::new();
    bus_a.probe_f64(&shared_cache, "drive", 3.0);
    // Warm cache, fresh bus: must register on B, not silently no-op.
    bus_b.probe_f64(&shared_cache, "drive", 9.0);
    assert_eq!(bus_a.probes_used.load(Ordering::Acquire), 1);
    assert_eq!(bus_b.probes_used.load(Ordering::Acquire), 1);
    assert_eq!(
        f64::from_bits(bus_a.probes[0].latest.load(Ordering::Relaxed)),
        3.0
    );
    assert_eq!(
        f64::from_bits(bus_b.probes[0].latest.load(Ordering::Relaxed)),
        9.0
    );
}

#[test]
fn concurrent_scan_during_registration() {
    let bus = DebugBus::new();
    let reader_bus = alloc::sync::Arc::clone(&bus);
    let names = ["alpha", "beta", "gamma", "delta"];
    let reader = std::thread::spawn(move || {
        for _ in 0..200 {
            let used = reader_bus.probes_used.load(Ordering::Acquire) as usize;
            for i in 0..used {
                let n = reader_bus.probes[i].name_str();
                assert!(n.is_empty() || names.contains(&n), "torn name: {n:?}");
            }
        }
    });
    for (i, n) in names.iter().enumerate() {
        let slot = AtomicU16::new(u16::MAX);
        bus.probe_f64(&slot, n, i as f64);
    }
    reader.join().unwrap();
    for (i, n) in names.iter().enumerate() {
        assert_eq!(bus.probes[i].name_str(), *n);
    }
}

/// S13: a name longer than a slot holds is stored truncated; every later
/// lookup must compare in the same truncated form, or each block reserves
/// a fresh slot until the bus is full.
#[test]
fn overlong_probe_name_occupies_exactly_one_slot() {
    let bus = DebugBus::new();
    let name = "a_forty_byte_probe_name_that_overflows!!";
    assert_eq!(name.len(), 40);
    // A fresh per-callsite cache each block (worst case: many instances /
    // a cold cache) plus a warm one.
    let warm = AtomicU16::new(u16::MAX);
    for i in 0..(MAX_PROBES * 2) {
        let cold = AtomicU16::new(u16::MAX);
        bus.probe_f64(&cold, name, i as f64);
        bus.probe_f64(&warm, name, i as f64);
    }
    assert_eq!(bus.probes_used.load(Ordering::Acquire), 1);
    assert_eq!(bus.probes[0].name_str(), &name[..NAME_BYTES - 1]);
    assert_eq!(
        bus.probes[0].hits.load(Ordering::Relaxed),
        (MAX_PROBES * 4) as u64
    );
}

#[test]
fn overlong_breakpoint_name_occupies_exactly_one_slot() {
    let bus = DebugBus::new();
    let name = "a_forty_byte_breakpoint_name_overflows!!";
    for _ in 0..(MAX_BREAKPOINTS * 2) {
        let cold = AtomicU16::new(u16::MAX);
        bus.breakpoint_check(&cold, name, false);
    }
    assert_eq!(bus.breakpoints_used.load(Ordering::Acquire), 1);
}

#[test]
fn inline_str_truncates_on_char_boundary() {
    // 'é' is 2 bytes; capacity 7 would split the 4th 'é'.
    assert_eq!(InlineStr::<8>::truncate("éééé"), "ééé");
    let s: InlineStr<8> = InlineStr::zeroed();
    s.set("éééé");
    assert_eq!(s.as_str(), "ééé");
    assert!(s.matches("éééé"));
    assert!(s.matches("ééé"));
    assert!(!s.matches("éé"));
}

#[test]
fn literal_probe_macro_compiles_within_limit() {
    let bus = DebugBus::new();
    probe!(bus, "exactly_thirty_one_bytes_long__", 1.0);
    assert_eq!(bus.probes[0].name_str().len(), NAME_BYTES - 1);
}

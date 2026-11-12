use super::*;
use crate::inline_str::InlineStr;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU16, Ordering};

const SPIN: TapId = TapId::new("spin");
const LEVEL: TapId = TapId::new("level");

fn used(t: &Telemetry) -> u16 {
    t.taps_used.load(Ordering::Acquire)
}

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
fn a_tap_publishes_latest_min_max_and_hits() {
    let t = Telemetry::new();
    let spin = Tap::register(&t, SPIN);
    assert_eq!(t.value(SPIN), None, "registered but not yet published");
    assert_eq!(spin.get(), 0.0);
    for v in [0.25, -1.5, 2.0] {
        spin.set(v);
    }
    assert_eq!(t.value(SPIN), Some(2.0));
    assert_eq!(spin.get(), 2.0);
    let r = &t.taps()[0];
    assert_eq!(
        (r.name.as_str(), r.latest, r.min, r.max, r.hits),
        ("spin", 2.0, -1.5, 2.0, 3)
    );
    t.reset_taps();
    let r = &t.taps()[0];
    assert_eq!((r.hits, r.min, r.max), (0, f64::INFINITY, f64::NEG_INFINITY));
}

#[test]
fn registering_an_id_twice_shares_its_slot() {
    let t = Telemetry::new();
    let writer = Tap::register(&t, SPIN);
    let reader = Tap::register(&t, SPIN);
    let other = Tap::register(&t, LEVEL);
    assert_eq!(used(&t), 2);
    writer.set(0.5);
    other.set(-3.0);
    assert_eq!(reader.get(), 0.5);
    assert_eq!(t.value(LEVEL), Some(-3.0));
}

#[test]
fn a_detached_tap_drops_writes() {
    let tap = Tap::detached();
    assert!(!tap.is_attached());
    tap.set(1.0);
    assert_eq!(tap.get(), 0.0);
}

#[test]
fn a_full_block_detaches_further_taps() {
    let t = Telemetry::new();
    // MAX_TAPS distinct one-character names, sliced from a 'static pool.
    const POOL: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-";
    for i in 0..MAX_TAPS {
        Tap::register(&t, TapId::new(&POOL[i..i + 1])).set(1.0);
    }
    let late = Tap::register(&t, TapId::new("late"));
    assert!(!late.is_attached());
    late.set(1.0);
    assert_eq!(t.value(TapId::new("late")), None);
}

#[test]
fn probes_and_taps_share_one_namespace() {
    let t = Telemetry::new();
    probe!(t, "spin", 0.75);
    assert_eq!(t.value(SPIN), Some(0.75), "a reader finds a probe by its TapId");
    let tap = Tap::register(&t, SPIN);
    tap.set(0.5);
    assert_eq!(used(&t), 1);
    probe!(t, "spin", 0.25);
    assert_eq!(tap.get(), 0.25);
}

#[test]
fn tap_ids_compare_by_hash_and_differ_by_name() {
    assert_eq!(SPIN, TapId::new("spin"));
    assert_ne!(SPIN, LEVEL);
    assert_eq!(SPIN.name(), "spin");
}

#[test]
fn warm_slot_cache_does_not_starve_second_block() {
    let shared_cache = AtomicU16::new(u16::MAX); // one callsite
    let a = Telemetry::new();
    let b = Telemetry::new();
    a.probe_f64(&shared_cache, "drive", 3.0);
    // Warm cache, fresh block: must register on B, not silently no-op.
    b.probe_f64(&shared_cache, "drive", 9.0);
    let drive = TapId::new("drive");
    assert_eq!((used(&a), used(&b)), (1, 1));
    assert_eq!((a.value(drive), b.value(drive)), (Some(3.0), Some(9.0)));
}

#[test]
fn concurrent_scan_during_registration() {
    let t = Telemetry::new();
    let reader_t = Arc::clone(&t);
    let names = ["alpha", "beta", "gamma", "delta"];
    let reader = std::thread::spawn(move || {
        for _ in 0..if cfg!(miri) { 20 } else { 200 } {
            for s in reader_t.tap_slots() {
                let n = s.name.as_str();
                assert!(n.is_empty() || names.contains(&n), "torn name: {n:?}");
            }
        }
    });
    for (i, n) in names.iter().enumerate() {
        Tap::register(&t, TapId::new(n)).set(i as f64);
    }
    reader.join().unwrap();
    for (i, n) in names.iter().enumerate() {
        assert_eq!(t.taps()[i].name, *n);
        assert_eq!(t.value(TapId::new(n)), Some(i as f64));
    }
}

/// S13: a name longer than a slot holds is stored truncated; every later
/// lookup must compare in the same truncated form, or each block reserves
/// a fresh slot until the block is full.
#[test]
fn overlong_probe_name_occupies_exactly_one_slot() {
    let t = Telemetry::new();
    let name = "a_forty_byte_probe_name_that_overflows!!";
    assert_eq!(name.len(), 40);
    // A fresh per-callsite cache each block (worst case: many instances /
    // a cold cache) plus a warm one.
    let warm = AtomicU16::new(u16::MAX);
    // Under Miri a few rounds already prove the slot is found again.
    let rounds = if cfg!(miri) { 8 } else { MAX_TAPS * 2 };
    for i in 0..rounds {
        let cold = AtomicU16::new(u16::MAX);
        t.probe_f64(&cold, name, i as f64);
        t.probe_f64(&warm, name, i as f64);
    }
    assert_eq!(used(&t), 1);
    let r = &t.taps()[0];
    assert_eq!(r.name, &name[..NAME_BYTES - 1]);
    assert_eq!(r.hits, (rounds * 2) as u64);
}

#[test]
fn overlong_breakpoint_name_occupies_exactly_one_slot() {
    let t = Telemetry::new();
    let name = "a_forty_byte_breakpoint_name_overflows!!";
    for _ in 0..(MAX_BREAKPOINTS * 2) {
        let cold = AtomicU16::new(u16::MAX);
        t.breakpoint_check(&cold, name, false);
    }
    assert_eq!(t.breakpoints().count(), 1);
}

#[test]
fn an_armed_breakpoint_pauses_and_counts() {
    let t = Telemetry::new();
    assert!(!brk_if!(t, "gate", true), "unarmed: registers, never mutes");
    let bp = t.breakpoints().next().unwrap();
    assert_eq!(bp.name(), "gate");
    bp.set_armed(true);
    t.tick_block();
    assert!(brk_if!(t, "gate", true));
    assert!(t.is_paused());
    let bp = t.breakpoints().next().unwrap();
    assert_eq!((bp.hits(), bp.last_hit_block()), (1, 1));
    t.reset_breakpoint_hits();
    assert_eq!(t.breakpoints().next().unwrap().hits(), 0);
}

#[test]
fn capture_and_curve_round_trip() {
    let t = Telemetry::new();
    assert_eq!(t.capture(), Capture::default());
    t.write_capture(&[&[0.5, -0.5], &[1.0, 0.0]], 7);
    assert_eq!(
        t.capture(),
        Capture {
            channels: 2,
            frames: 2,
            block: 7
        }
    );
    let mut out = [0.0; 4];
    assert_eq!(t.read_capture_channel(1, &mut out), (2, 7));
    assert_eq!(&out[..2], &[1.0, 0.0]);
    assert!(t.curve().is_empty());
    t.publish_curve(&[0.0, 0.5, 1.0]);
    assert_eq!(t.curve(), [0.0, 0.5, 1.0]);
}

/// Found by the simulated host: a host's null output channel reaches
/// the capture as an empty slice; it captures silence, not a panic.
#[test]
fn a_short_capture_channel_reads_silence() {
    let t = Telemetry::new();
    t.write_capture(&[&[0.5, -0.5], &[]], 3);
    let mut out = [9.0; 2];
    assert_eq!(t.read_capture_channel(1, &mut out), (2, 3));
    assert_eq!(out, [0.0, 0.0]);
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
    let t = Telemetry::new();
    probe!(t, "exactly_thirty_one_bytes_long__", 1.0);
    assert_eq!(t.taps()[0].name.len(), NAME_BYTES - 1);
}

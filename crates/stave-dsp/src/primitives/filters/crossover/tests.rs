use super::*;
use crate::primitives::spectral::{Complex, Fft};
use crate::units::{db, hz};

const ORDERS: [Order; 4] = [Order::Lr2, Order::Lr4, Order::Lr6, Order::Lr8];
const LEN: usize = 1 << 16;

/// The magnitude (dB) per bin of a real impulse response, `LEN` long.
fn spectrum_db(h: &[f64]) -> Vec<f64> {
    let fft = Fft::<f64>::new(h.len());
    let mut bins = vec![Complex::new(0.0, 0.0); h.len() / 2 + 1];
    fft.forward_real(h, &mut bins);
    bins.iter().map(|z| 20.0 * z.norm().max(1e-300).log10()).collect()
}

/// Each band's impulse response and the recombined one.
fn impulses<const N: usize>(x: &mut Crossover<N>, rate: f32) -> (Vec<Vec<f64>>, Vec<f64>) {
    x.prepare(rate);
    let mut bands = vec![vec![0.0; LEN]; N];
    let mut sum = vec![0.0; LEN];
    for i in 0..LEN {
        let b = x.split(if i == 0 { 1.0 } else { 0.0 });
        for (k, v) in b.iter().enumerate() {
            bands[k][i] = f64::from(*v);
            sum[i] += f64::from(*v);
        }
    }
    (bands, sum)
}

fn bin(f: f64, rate: f32) -> usize {
    (f * LEN as f64 / f64::from(rate)).round() as usize
}

/// Worst deviation from flat (dB) of the recombined bands, 20 Hz–20 kHz.
fn worst_flatness<const N: usize>(order: Order, corners: &[f32], rate: f32) -> f64 {
    let corners: Vec<Hz> = corners.iter().map(|&c| hz(c)).collect();
    let mut x = Crossover::<N>::linkwitz_riley(order, &corners);
    let (_, sum) = impulses(&mut x, rate);
    let s = spectrum_db(&sum);
    let top = 20_000.0f64.min(0.45 * f64::from(rate));
    s[bin(20.0, rate)..=bin(top, rate)]
        .iter()
        .fold(0.0f64, |w, v| w.max(v.abs()))
}

#[test]
#[cfg_attr(miri, ignore)]
fn linkwitz_riley_bands_sum_flat() {
    let mut worst = 0.0f64;
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for order in ORDERS {
            worst = worst.max(worst_flatness::<2>(order, &[1_000.0], rate));
            worst = worst.max(worst_flatness::<3>(order, &[250.0, 2_500.0], rate));
            worst = worst.max(worst_flatness::<4>(order, &[120.0, 800.0, 5_000.0], rate));
            worst = worst.max(worst_flatness::<6>(
                order,
                &[80.0, 250.0, 800.0, 2_500.0, 8_000.0],
                rate,
            ));
        }
    }
    assert!(worst < 0.002, "recombined {worst:.5} dB from flat");

    // A two-band corner reads −6.02 dB on both sides.
    for order in ORDERS {
        let mut x = Crossover::<2>::linkwitz_riley(order, &[hz(1_000.0)]);
        let (bands, _) = impulses(&mut x, 48_000.0);
        for band in &bands {
            let g = spectrum_db(band)[bin(1_000.0, 48_000.0)];
            // Bin centres sit within 0.4 Hz of the corner.
            assert!((g + 6.0206).abs() < 0.01, "{order:?} corner {g:.4} dB");
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn linear_phase_bands_sum_to_the_delay() {
    let mut x = Crossover::<4>::linear_phase(
        &[hz(120.0), hz(800.0), hz(5_000.0)],
        hz(200.0),
        db(80.0),
    );
    let (_, sum) = impulses(&mut x, 48_000.0);
    let d = x.latency() as usize;
    // Kaiser: (80 − 7.95)/(14.36 · 200/48 000) + 1 → 1 207 taps.
    assert_eq!(d, 603);
    let worst = sum
        .iter()
        .enumerate()
        .map(|(i, &v)| (v - if i == d { 1.0 } else { 0.0 }).abs())
        .fold(0.0f64, f64::max);
    assert!(worst < 2e-6, "recombined impulse off by {worst:e}");
}

/// The worst level (dB) any band reaches two octaves beyond its edges.
fn worst_leak<const N: usize>(x: &mut Crossover<N>, rate: f32) -> f64 {
    let (bands, _) = impulses(x, rate);
    let corners = x.corners();
    let mut worst = f64::NEG_INFINITY;
    for (i, band) in bands.iter().enumerate() {
        let s = spectrum_db(band);
        let mut probe = |f: f64| {
            if (20.0..0.45 * f64::from(rate)).contains(&f) {
                worst = worst.max(s[bin(f, rate)]);
            }
        };
        if i > 0 {
            probe(f64::from(corners[i - 1].get()) / 4.0);
        }
        if i + 1 < N {
            probe(f64::from(corners[i].get()) * 4.0);
        }
    }
    worst
}

#[test]
#[cfg_attr(miri, ignore)]
fn bands_are_isolated() {
    let corners = [hz(120.0), hz(800.0), hz(5_000.0)];
    for (order, n) in ORDERS.iter().zip(1..) {
        let mut x = Crossover::<4>::linkwitz_riley(*order, &corners);
        let leak = worst_leak(&mut x, 48_000.0);
        let budget = -(24.0 * f64::from(n) - 1.0);
        eprintln!("{order:?}: worst leak {leak:.1} dB");
        assert!(leak < budget, "{order:?} leaks {leak:.1} dB (budget {budget})");
    }
    // Linear phase: a corner's stopband starts half the transition
    // away; probe a whole transition away.
    let mut x = Crossover::<4>::linear_phase(&corners, hz(200.0), db(80.0));
    let (bands, _) = impulses(&mut x, 48_000.0);
    let mut worst = f64::NEG_INFINITY;
    for (i, band) in bands.iter().enumerate() {
        let s = spectrum_db(band);
        for (k, c) in corners.iter().enumerate() {
            let f = f64::from(c.get());
            // Band i lies above corner i − 1 and below corner i.
            let outside = if k < i { f - 200.0 } else { f + 200.0 };
            if k + 1 == i || k == i {
                worst = worst.max(s[bin(outside, 48_000.0)]);
            }
        }
    }
    eprintln!("linear phase: worst leak {worst:.1} dB");
    assert!(worst < -80.0, "linear phase leaks {worst:.1} dB");
}

#[test]
#[cfg_attr(miri, ignore)]
fn moving_a_corner_keeps_the_sum_flat() {
    let mut x = Crossover::<3>::linkwitz_riley(Order::Lr4, &[hz(250.0), hz(2_500.0)]);
    x.prepare(48_000.0);
    x.set_corner(0, hz(400.0));
    x.set_corner(1, hz(4_000.0));
    assert_eq!(x.corners()[..2], [hz(400.0), hz(4_000.0)]);
    let (_, sum) = impulses(&mut x, 48_000.0);
    let s = spectrum_db(&sum);
    let worst = s[bin(20.0, 48_000.0)..bin(20_000.0, 48_000.0)]
        .iter()
        .fold(0.0f64, |w, v| w.max(v.abs()));
    assert!(worst < 0.002, "{worst}");
}

#[test]
fn tails_cover_the_ring() {
    let mut x = Crossover::<3>::linkwitz_riley(Order::Lr8, &[hz(100.0), hz(1_000.0)]);
    let (_, sum) = impulses(&mut x, 48_000.0);
    let Some(n) = x.tail().to_samples() else { panic!() };
    let last = sum.iter().rposition(|v| v.abs() > 1e-6).unwrap();
    assert!(last <= n as usize, "rang {last}, reported {n}");
}

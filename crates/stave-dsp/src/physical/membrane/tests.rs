use super::*;
use crate::physical::probe::{cents, frequency, hann, t60};
use crate::units::{hz, seconds};

const FS: f32 = 48_000.0;

/// Abramowitz & Stegun table 9.5: `j_{m,n}`, m = 0…3, n = 1…4.
const TABLE: [[f64; 4]; 4] = [
    [2.404_825_557_695_773, 5.520_078_110_286_311, 8.653_727_912_911_012, 11.791_534_439_014_28],
    [3.831_705_970_207_512, 7.015_586_669_815_619, 10.173_468_135_062_72, 13.323_691_936_314_22],
    [5.135_622_301_840_683, 8.417_244_140_399_865, 11.619_841_172_149_06, 14.795_951_782_351_26],
    [6.380_161_895_923_984, 9.761_023_129_981_67, 13.015_200_721_698_43, 16.223_466_160_318_77],
];

fn ring(t60: f64) -> Loss {
    Loss::two_point(hz(0.0), seconds(t60), hz(1.0), seconds(t60)).unwrap()
}

fn drum(loss: Loss) -> Membrane {
    let mut m = Membrane::new(Drumhead::tuned(0.15, 0.26, hz(100.0)), loss);
    m.set_bandwidth(hz(1_500.0));
    m.prepare(FS);
    m
}

fn impulse(m: &mut Membrane, len: usize) -> Vec<f64> {
    m.reset();
    (0..len).map(|i| f64::from(m.tick(if i == 0 { 1.0 } else { 0.0 }))).collect()
}

#[test]
fn bessel_zeros_match_the_tables() {
    let mut zeros = Vec::new();
    bessel::zeros(6, 17.0, &mut zeros);
    let mut worst = 0.0f64;
    for (m, row) in TABLE.iter().enumerate() {
        for (n, want) in row.iter().enumerate() {
            let got = zeros.iter().find(|z| z.0 as usize == m && z.1 as usize == n + 1).expect("zero found").2;
            worst = worst.max((got - want).abs());
        }
    }
    eprintln!("Bessel zeros: worst {worst:e} from A&S 9.5");
    assert!(worst < 1e-12, "{worst:e}");
    // Sorted, and none missing below 17: J_0 and J_1 have five there,
    // J_2 and J_3 four, J_4 and J_5 three.
    assert!(zeros.windows(2).all(|w| w[0].2 <= w[1].2));
    assert_eq!(zeros.len(), 5 + 5 + 4 + 4 + 3 + 3);
}

#[test]
#[cfg_attr(miri, ignore)]
fn mode_ratios_ring_as_bessel_zeros_say() {
    let mut m = drum(ring(10.0));
    let f01 = m.modes()[0].hz();
    // The mode list against the table: (m, n) in rising order.
    let order = [(0, 1), (1, 1), (2, 1), (0, 2), (3, 1), (1, 2)];
    for (mode, (mm, nn)) in m.modes().iter().zip(order) {
        assert_eq!((mode.m, mode.n), (mm, nn));
        let want = TABLE[mm as usize][nn as usize - 1] / TABLE[0][0];
        assert!((mode.hz() / f01 / want - 1.0).abs() < 1e-12, "{mode:?}");
    }
    // And the sound: each mode's ring, by its phase advance over 1 s.
    let fs = f64::from(FS);
    let y = impulse(&mut m, 3 * FS as usize);
    let mut worst = 0.0f64;
    for mode in &m.modes()[..10] {
        let damped = (mode.natural * mode.natural - mode.sigma * mode.sigma).sqrt() / core::f64::consts::TAU;
        let got = frequency(&y, fs, damped, 4_800, 4_800 + FS as usize, FS as usize);
        worst = worst.max(cents(got, damped).abs());
    }
    eprintln!("Membrane: modes ring within {worst:.5} cents of j_mn c/a");
    assert!(worst < 0.01, "{worst} cents");
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_centre_strike_rings_only_axisymmetric_modes() {
    let mut m = drum(ring(5.0));
    m.set_drive(Polar::new(0.0, 0.0));
    let fs = f64::from(FS);
    let y = impulse(&mut m, FS as usize);
    let at = |f: f64| hann(&y, fs, f, 0, FS as usize).0;
    let modes = m.modes().to_vec();
    let axis = modes.iter().filter(|x| x.m == 0).map(|x| at(x.hz())).fold(f64::INFINITY, f64::min);
    let off = modes.iter().filter(|x| x.m > 0).take(6).map(|x| at(x.hz())).fold(0.0, f64::max);
    eprintln!("Centre strike: weakest (0, n) {:.1} dB, strongest (m > 0, n) {:.1} dB", 20.0 * axis.log10(), 20.0 * off.log10());
    assert!(off < 1e-6 && off < 1e-3 * axis, "m > 0 modes ring at {off:e} against {axis:e}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn decay_follows_the_loss_law() {
    let loss = Loss::two_point(hz(100.0), seconds(3.0), hz(1_000.0), seconds(0.8)).unwrap();
    let mut m = drum(loss);
    let fs = f64::from(FS);
    let y = impulse(&mut m, 3 * FS as usize);
    let mut worst = 0.0f64;
    for mode in m.modes().iter().take(8) {
        let want = loss.t60(mode.hz()).get();
        let len = 19_200;
        let got = t60(&y, fs, mode.hz(), len, 2_400, 2_400 + ((0.8 * want * fs) as usize).max(3 * len));
        worst = worst.max((got / want - 1.0).abs());
    }
    eprintln!("Membrane decay: worst {:.3} % from the loss law", 100.0 * worst);
    assert!(worst < 0.005, "{:.3} %", 100.0 * worst);
}

#[test]
fn block_and_tick_agree_bit_for_bit() {
    let mut a = drum(ring(1.0));
    let mut b = a.clone();
    let input: Vec<f32> = (0..1_000).map(|i| (((i * 7919) % 211) as f32 / 105.0 - 1.0) * 0.5).collect();
    let ticked: Vec<f32> = input.iter().map(|&x| a.tick(x)).collect();
    let mut blocked = input.clone();
    for chunk in blocked.chunks_mut(37) {
        b.process(chunk);
    }
    assert_eq!(ticked, blocked);
    assert!(a.scale() > 0.0 && a.tail().to_samples().is_some());
}

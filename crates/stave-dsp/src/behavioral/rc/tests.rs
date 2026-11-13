use super::*;

/// The diode preset's conduction law, written out (the stage tabulates it).
fn conduction(u: f64, th: f64, h: f64) -> f64 {
    let t = u / th;
    t.signum() * (t.abs().powf(h) / (1.0 + t.abs().powf(h - 1.0))) + 0.05 * u
}

#[test]
fn series_stage_matches_backward_euler_and_the_ode() {
    // τ·dv/dt = i(x − v), output x − v, on a driven 200 Hz sine at 48 kHz.
    // Against (1) backward Euler on the analytic law, each step solved
    // by bisection to 1e-13: the tabulated curve and the capped Newton
    // add at most 1e-4; (2) the continuous ODE (RK4 at 64 substeps per
    // sample, input linearly interpolated): backward Euler's own error
    // at k = dt/τ ≈ 0.02 stays under 0.03 of full scale.
    let (fs, th, h, tau_ms) = (48_000.0f64, 0.5, 4.0, 1.0);
    let config = RcClipperConfig { tau_ms: tau_ms as f32, threshold: th as f32, hardness: h as f32 };
    let mut rc = NonlinearRc::diode_clipper(&config);
    rc.prepare(fs as f32);
    let k = 1.0 / (tau_ms * 1e-3 * fs);
    let (mut v_be, mut v_ode) = (0.0f64, 0.0f64);
    let (mut worst_be, mut worst_ode) = (0.0f64, 0.0f64);
    let x_at = |n: f64| 2.0 * (core::f64::consts::TAU * 200.0 * n / fs).sin();
    for n in 0..4_800 {
        let x = x_at(n as f64) as f32 as f64;
        let y = f64::from(rc.tick(x as f32));

        // Backward Euler: v − v0 − k·i(x − v) = 0, increasing in v.
        let v0 = v_be;
        let (mut lo, mut hi) = (v0 - 10.0, v0 + 10.0);
        while hi - lo > 1e-13 {
            let mid = 0.5 * (lo + hi);
            if mid - v0 - k * conduction(x - mid, th, h) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        v_be = 0.5 * (lo + hi);
        worst_be = worst_be.max((y - (x - v_be)).abs());

        // The ODE from the previous sample to this one.
        let sub = 64;
        let dt = 1.0 / (tau_ms * 1e-3 * fs * sub as f64);
        let dv = |v: f64, s: f64| conduction(x_at(n as f64 - 1.0 + s) - v, th, h);
        for j in 0..sub {
            let s = j as f64 / sub as f64;
            let ds = 1.0 / sub as f64;
            let k1 = dv(v_ode, s);
            let k2 = dv(v_ode + 0.5 * dt * k1, s + 0.5 * ds);
            let k3 = dv(v_ode + 0.5 * dt * k2, s + 0.5 * ds);
            let k4 = dv(v_ode + dt * k3, s + ds);
            v_ode += dt / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
        }
        worst_ode = worst_ode.max((y - (x - v_ode)).abs());
    }
    assert!(worst_be < 1e-4, "vs backward Euler: {worst_be:.2e}");
    assert!(worst_ode < 0.03, "vs the ODE: {worst_ode:.3}");
}

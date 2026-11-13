use super::*;

/// The loop's opening: output at the input's falling zero crossing minus
/// output at its rising one, over the last of `cycles` triangle periods
/// of `period` samples at amplitude 1, with the output's 5 Hz DC blocker
/// undone (its phase lead would otherwise shift the crossings by an
/// amount that depends on the speed).
fn loop_width(depth: f32, period: usize, cycles: usize) -> f64 {
    let mut h = Hysteresis::new(2.0, depth);
    h.prepare(48_000.0);
    let r = (-core::f64::consts::TAU * 5.0 / 48_000.0).exp();
    let (mut pre, mut last) = (0.0f64, 0.0f64);
    let (mut up, mut down) = (0.0, 0.0);
    for i in 0..period * cycles {
        let t = (i % period) as f32 / period as f32;
        // 0 → 1 → −1 → 0 over one period.
        let x = if t < 0.25 { 4.0 * t } else if t < 0.75 { 2.0 - 4.0 * t } else { 4.0 * t - 4.0 };
        let out = f64::from(h.tick(x));
        // The blocker's inverse, (1 − r·z⁻¹)/(1 − z⁻¹).
        pre += out - r * last;
        last = out;
        let y = pre;
        if i >= period * (cycles - 1) {
            if i % period == 0 {
                up = y;
            } else if i % period == period / 2 {
                down = y;
            }
        }
    }
    // Rising through zero the core still holds negative remanence.
    down - up
}

#[test]
fn the_loop_opens_with_depth_and_follows_the_path_not_the_speed() {
    // The behavioural Jiles–Atherton split: remanence opens a loop (the
    // falling branch sits above the rising one at x = 0), deeper is
    // wider, and the loop is a function of the input's path, not its
    // speed: the same triangle at half the speed opens the same loop
    // within 2 % (the irreversible part moves per unit of input
    // travelled, not per second).
    let shallow = loop_width(0.1, 960, 6);
    let deep = loop_width(0.9, 960, 6);
    assert!(shallow > 0.0, "no loop at depth 0.1: {shallow}");
    assert!(deep > 1.5 * shallow, "depth 0.9 opened {deep}, depth 0.1 {shallow}");
    let slow = loop_width(0.9, 1_920, 6);
    assert!(
        (slow - deep).abs() < 0.02 * deep,
        "loop changed with speed: {deep} at 50 Hz, {slow} at 25 Hz"
    );
}

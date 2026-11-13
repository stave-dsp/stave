//! Bessel functions of the first kind and their zeros, for the
//! membrane's modes.
//!
//! `J_0 … J_M` at once by Miller's backward recurrence (Abramowitz &
//! Stegun 9.12; the recurrence `J_{k−1} = (2k/x) J_k − J_{k+1}` run down
//! from far above both `M` and `x`, normalised by
//! `J_0 + 2 Σ J_{2k} = 1`), accurate to a few ulps of the largest order's
//! scale. Zeros by sign changes on a half-unit grid (consecutive zeros
//! of any `J_m` are more than 2.9 apart) refined by safeguarded Newton
//! with `J_m′ = J_{m−1} − (m/x) J_m`.

/// `J_0(x) … J_{out.len()−1}(x)` into `out` (x ≥ 0).
pub(super) fn bessel_j(x: f64, out: &mut [f64]) {
    let orders = out.len();
    if orders == 0 {
        return;
    }
    if x < 1e-12 {
        out.fill(0.0);
        out[0] = 1.0;
        return;
    }
    // Start well above both the highest order and x.
    let top = orders.max(x as usize) + 20 + (40.0 * (orders as f64).max(x)).sqrt() as usize;
    let top = top + top % 2;
    let (mut above, mut here) = (0.0f64, 1e-300f64);
    let mut norm = 0.0f64;
    for k in (1..=top).rev() {
        let below = 2.0 * k as f64 / x * here - above;
        above = here;
        here = below;
        // `here` is now J_{k−1} (unnormalised).
        if k - 1 < orders {
            out[k - 1] = here;
        }
        if (k - 1) % 2 == 0 && k - 1 > 0 {
            norm += 2.0 * here;
        }
        if here.abs() > 1e250 {
            // Rescale everything kept so far.
            let s = 1e-250;
            above *= s;
            here *= s;
            norm *= s;
            for v in out.iter_mut().skip(k - 1) {
                *v *= s;
            }
        }
    }
    norm += here;
    for v in out.iter_mut() {
        *v /= norm;
    }
}

/// `J_m(x)` and `J_m′(x)`, through `scratch` (length ≥ m + 2).
fn j_and_slope(m: usize, x: f64, scratch: &mut [f64]) -> (f64, f64) {
    let s = &mut scratch[..m + 2];
    bessel_j(x, s);
    let j = s[m];
    let slope = if m == 0 { -s[1] } else { s[m - 1] - m as f64 / x * j };
    (j, slope)
}

/// Every zero `j_{m,n}` of `J_0 … J_{orders−1}` below `limit`, as
/// `(m, n, j_{m,n})`, in rising order of `j`. `scratch` must hold at
/// least `orders + 2` values (no allocation besides `out`'s growth).
pub(super) fn zeros(orders: usize, limit: f64, out: &mut Vec<(u32, u32, f64)>) {
    out.clear();
    if orders == 0 || limit <= 0.0 {
        return;
    }
    let mut prev = vec![0.0f64; orders + 2];
    let mut here = vec![0.0f64; orders + 2];
    let mut scratch = vec![0.0f64; orders + 2];
    let mut count = vec![0u32; orders];
    const STEP: f64 = 0.5;
    let mut x0 = 0.25;
    bessel_j(x0, &mut prev);
    while x0 < limit {
        let x1 = x0 + STEP;
        bessel_j(x1, &mut here);
        for m in 0..orders {
            let (a, b) = (prev[m], here[m]);
            // J_m(x) for m ≥ 1 starts at a zero at x = 0: skip it.
            if a == 0.0 || a.signum() == b.signum() {
                continue;
            }
            let (mut lo, mut hi) = (x0, x1);
            let mut x = 0.5 * (lo + hi);
            for _ in 0..60 {
                let (j, slope) = j_and_slope(m, x, &mut scratch);
                if j == 0.0 {
                    break;
                }
                if j.signum() == a.signum() {
                    lo = x;
                } else {
                    hi = x;
                }
                let newton = x - j / slope;
                let next = if newton > lo && newton < hi { newton } else { 0.5 * (lo + hi) };
                if (next - x).abs() < 1e-15 * x {
                    x = next;
                    break;
                }
                x = next;
            }
            count[m] += 1;
            if x < limit {
                out.push((m as u32, count[m], x));
            }
        }
        core::mem::swap(&mut prev, &mut here);
        x0 = x1;
    }
    out.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
}

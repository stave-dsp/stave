//! The pointwise nonlinearities trained networks use, by the names
//! PyTorch and the `.nam` format give them. Crate-private: a network
//! file names its activations; authors never pick one.
//!
//! Applied a slice at a time, so each loop is one formula the compiler
//! can vectorise. `tanh` and the logistic sigmoid go through [`exp`], a
//! branch-free f64 exponential: accurate to ~1e-10 relative, far below
//! an f32 ulp, so results round to within one ulp of the true value
//! (the platform `tanhf` is a scalar call and dominated the cost).

/// One pointwise activation. Formulas are PyTorch's (`torch.nn.*`), the
/// definition the networks were trained with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Activation {
    /// `tanh x`.
    Tanh,
    /// `clamp(x, min, max)`.
    Hardtanh { min: f32, max: f32 },
    /// `max(x, 0)`.
    Relu,
    /// `x` for `x ≥ 0`, `slope · x` below.
    LeakyRelu { slope: f32 },
    /// Leaky ReLU with a learned slope, one per channel or one shared
    /// (a single entry).
    Prelu { slopes: Vec<f32> },
    /// `1 / (1 + e^−x)`.
    Sigmoid,
    /// `x · sigmoid(x)` (also called swish).
    Silu,
    /// `x · clamp(x + 3, 0, 6) / 6`.
    Hardswish,
    /// `x / (1 + |x|)`.
    Softsign,
    /// Identity inside `[min, max]`, slopes `min_slope` / `max_slope`
    /// outside.
    LeakyHardtanh {
        min: f32,
        max: f32,
        min_slope: f32,
        max_slope: f32,
    },
}

impl Activation {
    /// `dst[j] = f(src[j])`, channel `j` (only [`Prelu`](Self::Prelu)
    /// reads the channel). The slices may be the same length only.
    #[inline]
    pub(crate) fn apply_into(&self, src: &[f32], dst: &mut [f32]) {
        let dst = &mut dst[..src.len()];
        let each = |dst: &mut [f32], f: &dyn Fn(f32) -> f32| {
            for (d, &x) in dst.iter_mut().zip(src) {
                *d = f(x);
            }
        };
        match self {
            Activation::Tanh => tanh_into(src, dst),
            Activation::Sigmoid => sigmoid_into(src, dst),
            Activation::Silu => {
                sigmoid_into(src, dst);
                for (d, &x) in dst.iter_mut().zip(src) {
                    *d *= x;
                }
            }
            Activation::Hardtanh { min, max } => each(dst, &|x| x.max(*min).min(*max)),
            Activation::Relu => each(dst, &|x| x.max(0.0)),
            Activation::LeakyRelu { slope } => each(dst, &|x| if x >= 0.0 { x } else { slope * x }),
            Activation::Prelu { slopes } => {
                for (j, (d, &x)) in dst.iter_mut().zip(src).enumerate() {
                    let s = if slopes.len() == 1 {
                        slopes[0]
                    } else {
                        slopes[j]
                    };
                    *d = if x >= 0.0 { x } else { s * x };
                }
            }
            Activation::Hardswish => each(dst, &|x| x * (x + 3.0).clamp(0.0, 6.0) / 6.0),
            Activation::Softsign => each(dst, &|x| x / (1.0 + x.abs())),
            Activation::LeakyHardtanh {
                min,
                max,
                min_slope,
                max_slope,
            } => each(dst, &|x| {
                if x < *min {
                    (x - min) * min_slope + min
                } else if x > *max {
                    (x - max) * max_slope + max
                } else {
                    x
                }
            }),
        }
    }

    /// Channels this activation accepts (`None`: any count).
    /// The activation on the first `c` channels (a per-channel PReLU
    /// keeps its first `c` slopes; the rest are channel-blind).
    pub(crate) fn sliced(&self, c: usize) -> Self {
        match self {
            Activation::Prelu { slopes } if slopes.len() > 1 => Activation::Prelu {
                slopes: slopes[..c.min(slopes.len())].to_vec(),
            },
            other => other.clone(),
        }
    }

    pub(crate) fn channels(&self) -> Option<usize> {
        match self {
            Activation::Prelu { slopes } if slopes.len() > 1 => Some(slopes.len()),
            _ => None,
        }
    }
}

/// `e^y` for `|y| ≤ 700`, branch-free: `2^n · e^r` with `r = y − n·ln 2`,
/// `|r| ≤ ln 2 / 2`, and `e^r` by its Taylor series to `r^11`
/// (truncation below 3e-13 relative). Inputs outside the range clamp.
#[inline]
pub(crate) fn exp(y: f64) -> f64 {
    // ln 2 split (fdlibm): the high part has trailing zero bits, so n·LN2_HI is exact.
    const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
    const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
    let y = y.clamp(-700.0, 700.0);
    let n = (y * std::f64::consts::LOG2_E).round();
    let r = (y - n * LN2_HI) - n * LN2_LO;
    let mut p = 1.0 / 39_916_800.0;
    for c in [
        1.0 / 3_628_800.0,
        1.0 / 362_880.0,
        1.0 / 40_320.0,
        1.0 / 5_040.0,
        1.0 / 720.0,
        1.0 / 120.0,
        1.0 / 24.0,
        1.0 / 6.0,
        0.5,
        1.0,
        1.0,
    ] {
        p = p * r + c;
    }
    // 2^n by its exponent bits; |n| ≤ 1010 stays in the normal range.
    p * f64::from_bits(((n as i64 + 1023) as u64) << 52)
}

/// `tanh x`: `(e^{2x} − 1) / (e^{2x} + 1)` in f64, clamped where f32
/// tanh is already ±1; near zero, where `e^{2x} − 1` would cancel, its
/// series `x − x³/3 + 2x⁵/15` (truncation below 1e-15 relative for
/// `|x| < 0.01`). Both are computed and one selected, so the loop
/// stays branch-free.
#[inline]
pub(crate) fn tanh(x: f32) -> f32 {
    let v = f64::from(x.clamp(-10.0, 10.0));
    let e = exp(2.0 * v);
    let ratio = (e - 1.0) / (e + 1.0);
    let v2 = v * v;
    let series = v * (1.0 - v2 * (1.0 / 3.0 - v2 * (2.0 / 15.0)));
    (if v.abs() < 0.01 { series } else { ratio }) as f32
}

/// [`tanh`] of every element.
#[inline]
pub(crate) fn tanh_into(src: &[f32], dst: &mut [f32]) {
    for (d, &x) in dst.iter_mut().zip(src) {
        *d = tanh(x);
    }
}

/// `1 / (1 + e^−x)` for every element, in f64.
#[inline]
pub(crate) fn sigmoid_into(src: &[f32], dst: &mut [f32]) {
    for (d, &x) in dst.iter_mut().zip(src) {
        *d = (1.0 / (1.0 + exp(-f64::from(x)))) as f32;
    }
}

/// In-place forms for the recurrent cells.
#[inline]
pub(crate) fn tanh_in_place(xs: &mut [f32]) {
    for x in xs {
        *x = tanh(*x);
    }
}

#[inline]
pub(crate) fn sigmoid_in_place(xs: &mut [f32]) {
    for x in xs {
        *x = (1.0 / (1.0 + exp(-f64::from(*x)))) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(a: &Activation, x: f32, channel: usize) -> f32 {
        let mut src = vec![0.0; channel + 1];
        src[channel] = x;
        let mut dst = vec![0.0; channel + 1];
        a.apply_into(&src, &mut dst);
        dst[channel]
    }

    #[test]
    fn formulas_match_their_definitions() {
        assert_eq!(one(&Activation::Relu, -2.0, 0), 0.0);
        assert_eq!(one(&Activation::LeakyRelu { slope: 0.1 }, -2.0, 0), -0.2);
        assert_eq!(
            one(
                &Activation::Hardtanh {
                    min: -0.5,
                    max: 0.7
                },
                3.0,
                0
            ),
            0.7
        );
        assert_eq!(one(&Activation::Softsign, 1.0, 0), 0.5);
        assert!((one(&Activation::Silu, 1.0, 0) - 0.731_058_6).abs() < 1e-6);
        assert_eq!(one(&Activation::Hardswish, -4.0, 0), 0.0);
        assert_eq!(one(&Activation::Hardswish, 4.0, 0), 4.0);
        let lh = Activation::LeakyHardtanh {
            min: -1.0,
            max: 1.0,
            min_slope: 0.1,
            max_slope: 0.2,
        };
        assert!((one(&lh, 2.0, 0) - 1.2).abs() < 1e-6);
        assert!((one(&lh, -2.0, 0) + 1.1).abs() < 1e-6);
        let p = Activation::Prelu {
            slopes: vec![0.5, 0.25],
        };
        assert_eq!(one(&p, -1.0, 1), -0.25);
        assert_eq!(p.channels(), Some(2));
    }

    #[test]
    fn exp_is_accurate_far_below_an_f32_ulp() {
        let mut worst = 0.0f64;
        let mut y = -80.0;
        while y < 80.0 {
            worst = worst.max((exp(y) / y.exp() - 1.0).abs());
            y += 0.013_7;
        }
        assert!(worst < 1e-12, "{worst:e}");
        assert_eq!(exp(0.0), 1.0);
        assert!(exp(-1e9) >= 0.0 && exp(1e9).is_finite());
    }

    #[test]
    fn tanh_and_sigmoid_round_within_one_ulp() {
        // Against f64's own functions, rounded to f32: at most one ulp
        // apart, across the whole range and at the clamps.
        let xs: Vec<f32> = (-12_000..=12_000)
            .map(|i| i as f32 * 1e-3)
            .chain([1e-30, -1e-7, 50.0])
            .collect();
        let mut t = vec![0.0; xs.len()];
        let mut s = vec![0.0; xs.len()];
        tanh_into(&xs, &mut t);
        sigmoid_into(&xs, &mut s);
        for (i, &x) in xs.iter().enumerate() {
            let ulps = |got: f32, want: f64| {
                let want = want as f32;
                (got.to_bits() as i64 - want.to_bits() as i64).abs()
            };
            assert!(ulps(t[i], f64::from(x).tanh()) <= 1, "tanh({x}) = {}", t[i]);
            assert!(
                ulps(s[i], 1.0 / (1.0 + (-f64::from(x)).exp())) <= 1,
                "sigmoid({x}) = {}",
                s[i]
            );
        }
        assert_eq!(t[t.len() - 1], 1.0);
        let mut back = xs.clone();
        tanh_in_place(&mut back);
        assert_eq!(back, t);
        let mut back = xs.clone();
        sigmoid_in_place(&mut back);
        assert_eq!(back, s);
    }
}

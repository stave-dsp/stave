use super::*;
use crate::primitives::fixed::Overflow;
use crate::primitives::random::SplitMix64;

#[test]
fn delay_datapath_matches_an_integer_reference() {
    for overflow in [Overflow::Saturate, Overflow::Wrap] {
        let mut d = Delay::new(Machine::SP1200, 0.01, overflow, 8);
        d.set_samples(37);
        d.set_feedback(1.4);
        // An 8-bit Q2.6 word: 1.4 truncates to 89/64.
        assert_eq!(d.feedback(), 89.0 / 64.0);
        let g = 89i64;
        let size = (0.01 * Machine::SP1200.rate_hz).ceil() as usize;
        let mut memory = vec![0i64; size];
        let mut write = 0usize;
        let mut r = SplitMix64::new(4);
        for _ in 0..5000 {
            let x = (r.next_u64() >> 53) as i64 - 1024;
            let read = (write + size - 37) % size;
            let delayed = memory[read];
            let fit = |v: i64| match overflow {
                Overflow::Saturate => v.clamp(-2048, 2047),
                Overflow::Wrap => (v + 2048).rem_euclid(4096) - 2048,
            };
            // The product back in the word, then the sum.
            memory[write] = fit(x + fit((delayed * g).div_euclid(64)));
            write = (write + 1) % size;
            assert_eq!(i64::from(d.step_codes(x as i32)), delayed, "{overflow:?}");
        }
    }
}

#[test]
fn truncated_feedback_sticks_one_lsb_below_zero() {
    let mut d = Delay::new(Machine::SP1200, 0.01, Overflow::Saturate, 8);
    d.set_samples(10);
    d.set_feedback(0.5);
    // A negative impulse: −100, −50, −25, −13, −7, −4, −2, −1, −1, …
    let mut repeats = Vec::new();
    for n in 0..400 {
        let out = d.step_codes(if n == 0 { -100 } else { 0 });
        if n % 10 == 0 && n > 0 {
            repeats.push(out);
        }
    }
    assert_eq!(&repeats[..9], &[-100, -50, -25, -13, -7, -4, -2, -1, -1]);
    assert!(repeats.iter().skip(8).all(|&r| r == -1), "{repeats:?}");
    // From above, truncation decays to zero.
    d.reset();
    let mut last = 1;
    for n in 0..400 {
        last = d.step_codes(if n == 0 { 100 } else { 0 });
    }
    assert_eq!(last, 0);
}

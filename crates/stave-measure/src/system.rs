//! The one shape every measurement drives: mono samples in, mono samples
//! out, in blocks. Any `FnMut(f32) -> f32` is a system; hosts adapt
//! plugins and DSP modules to it, so a test written once runs on all of
//! them.

pub trait System {
    /// `output[i]` is the response to `input[i]`; equal lengths.
    fn process(&mut self, input: &[f32], output: &mut [f32]);

    fn tick(&mut self, x: f32) -> f32 {
        let mut y = [0.0];
        self.process(&[x], &mut y);
        y[0]
    }
}

impl<F: FnMut(f32) -> f32> System for F {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (y, &x) in output.iter_mut().zip(input) {
            *y = self(x);
        }
    }

    fn tick(&mut self, x: f32) -> f32 {
        self(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Doubler;
    impl System for Doubler {
        fn process(&mut self, input: &[f32], output: &mut [f32]) {
            output.iter_mut().zip(input).for_each(|(y, x)| *y = 2.0 * x);
        }
    }

    #[test]
    fn closures_and_block_systems_are_interchangeable() {
        let mut halve = |x: f32| 0.5 * x;
        let mut out = [0.0; 3];
        halve.process(&[2.0, 4.0, 6.0], &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert_eq!(Doubler.tick(3.0), 6.0);
    }
}

//! **Sequence** — kernel form #2: serial
//! composition as a static tuple over any `impl Stage`, no framework
//! kind enum at the gate. Owns the serial invariants: prepare cascades,
//! latency sums, tail sums (unbounded absorbs), block form
//! dispatches each element once per block. For topologies that MORPH
//! at runtime (kind swaps, slots) use `structures::chain::Chain` —
//! that is this form elaborated, not a rival.

use crate::kernel::node::Stage;

pub struct Serial<T>(pub T);

macro_rules! impl_serial {
    ($($T:ident . $idx:tt),+) => {
        impl<$($T: Stage),+> Stage for Serial<($($T,)+)> {
            fn prepare(&mut self, rate: f32) {
                $(self.0.$idx.prepare(rate);)+
            }
            fn reset(&mut self) {
                $(self.0.$idx.reset();)+
            }
            #[inline]
            fn tick(&mut self, x: f32) -> f32 {
                let y = x;
                $(let y = self.0.$idx.tick(y);)+
                y
            }
            fn process_block(&mut self, io: &mut [f32]) {
                $(self.0.$idx.process_block(io);)+
            }
            fn latency_samples(&self) -> u32 {
                0 $(+ self.0.$idx.latency_samples())+
            }
            fn tail_samples(&self) -> Option<u32> {
                let mut t: u32 = 0;
                $(t = t.checked_add(self.0.$idx.tail_samples()?)?;)+
                Some(t)
            }
        }
    };
}

impl_serial!(A.0, B.1);
impl_serial!(A.0, B.1, C.2);
impl_serial!(A.0, B.1, C.2, D.3);
impl_serial!(A.0, B.1, C.2, D.3, E.4);
impl_serial!(A.0, B.1, C.2, D.3, E.4, F.5);
impl_serial!(A.0, B.1, C.2, D.3, E.4, F.5, G.6);
impl_serial!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7);

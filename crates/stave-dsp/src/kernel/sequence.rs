//! **Serial** — static serial composition as a tuple over any
//! `impl Stage`. Owns the serial invariants: prepare cascades, latency
//! sums, tail sums (infinite absorbs), the block form dispatches each
//! element once per block. For topologies that change at runtime
//! (kind swaps, addressable slots) use [`Chain`](crate::Chain) — this
//! form elaborated, not a rival.

use crate::kernel::node::{Stage, Tail};

#[derive(Clone)]
pub struct Serial<T>(pub T);

macro_rules! impl_serial {
    ($($T:ident . $idx:tt),+) => {
        impl<$($T: Stage),+> Stage for Serial<($($T,)+)> {
            fn prepare(&mut self, sample_rate: f32) {
                $(self.0.$idx.prepare(sample_rate);)+
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
            fn process(&mut self, block: &mut [f32]) {
                $(self.0.$idx.process(block);)+
            }
            fn latency(&self) -> u32 {
                0 $(+ self.0.$idx.latency())+
            }
            fn tail(&self) -> Tail {
                Tail::None $(.then(self.0.$idx.tail()))+
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

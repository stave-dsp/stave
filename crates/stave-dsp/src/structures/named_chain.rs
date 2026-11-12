//! `chain!` — a named-topology chain: the declaration's SHAPE is the
//! signal path, read top to bottom, and every slot is a typed, named
//! handle. The expansion is plain Rust a reader can click through —
//! a struct holding a [`Chain`](crate::Chain) plus one `StageId`
//! field per slot, an accessor method per slot, and a
//! [`Stage`](crate::Stage) impl so the whole thing nests inside
//! `Oversampled2x`/`Serial` like any node. Never an operator DSL.
//!
//! ```ignore
//! stave_dsp::chain! {
//!     /// The amp channel, in signal order.
//!     pub struct AmpChain {
//!         primary: Curve = CurveStage::from_fn(-12.0, 12.0, 0.0, shapes::knee(1.0)),
//!         voicing: Filter = Biquad::new(FilterSpec::HighShelf { hz: 900.0, db: 0.0 }),
//!     }
//! }
//!
//! let mut ch = AmpChain::new();
//! ch.voicing().set(spec);          // typed access, no kind ritual
//! matrix_route_to(ch.primary);     // the same name IS the StageId
//! ```
//!
//! Slot access rules:
//! * `ch.slot` (field) — the slot's `StageId`, for routes, swaps,
//!   and sense reads.
//! * `ch.slot()` (method) — `&mut` the slot's declared stage type.
//!   PANICS (naming the slot) if the slot currently holds another
//!   kind — use it on slots you never swap; swap-aware code asks
//!   `as_chain_mut().stage_mut(id)` and matches, because there the
//!   question "which kind is it right now?" is real.

/// Maps a slot's declared kind keyword to its stage type. Extend when
/// [`StageKind`](crate::StageKind) grows a variant.
#[doc(hidden)]
#[macro_export]
macro_rules! chain_cell {
    (Curve) => {
        $crate::CurveStage
    };
    (Filter) => {
        $crate::Biquad
    };
    (Slew) => {
        $crate::Slew
    };
    (Hysteresis) => {
        $crate::Hysteresis
    };
    (Rc) => {
        $crate::NonlinearRc
    };
    (Coupler) => {
        $crate::Coupler
    };
}

#[macro_export]
macro_rules! chain {
    (
        $(#[$meta:meta])*
        $vis:vis struct $Name:ident {
            $( $(#[$fmeta:meta])* $slot:ident : $Kind:ident = $init:expr ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $Name {
            chain: $crate::Chain<{ 0usize $( + { let _ = stringify!($slot); 1 } )+ }>,
            $( $(#[$fmeta])* $vis $slot : $crate::StageId, )+
        }

        impl $Name {
            /// Build the declared topology, slots in declaration order.
            $vis fn new() -> Self {
                let mut chain = $crate::Chain::new();
                $(
                    let $slot = chain
                        .push($crate::StageKind::$Kind($init))
                        .expect("the chain's capacity is its slot count");
                )+
                Self { chain, $( $slot, )+ }
            }

            /// The underlying sequence — topology data, sense
            /// endpoints, feedback wiring.
            #[inline]
            $vis fn as_chain(
                &self,
            ) -> &$crate::Chain<{ 0usize $( + { let _ = stringify!($slot); 1 } )+ }> {
                &self.chain
            }
            #[inline]
            $vis fn as_chain_mut(
                &mut self,
            ) -> &mut $crate::Chain<{ 0usize $( + { let _ = stringify!($slot); 1 } )+ }> {
                &mut self.chain
            }

            $(
                $(#[$fmeta])*
                /// Typed access to this slot's declared stage. Panics
                /// if the slot was swapped to another kind — use on
                /// slots you never swap.
                #[inline]
                $vis fn $slot(&mut self) -> &mut $crate::chain_cell!($Kind) {
                    match self.chain.stage_mut(self.$slot) {
                        Some($crate::StageKind::$Kind(stage)) => stage,
                        _ => panic!(concat!(
                            "slot `", stringify!($slot),
                            "` no longer holds its declared ",
                            stringify!($Kind),
                            " — swap-aware code must match on stage_mut"
                        )),
                    }
                }
            )+
        }

        impl $crate::Stage for $Name {
            #[inline]
            fn prepare(&mut self, sample_rate: f32) {
                $crate::Stage::prepare(&mut self.chain, sample_rate);
            }
            #[inline]
            fn reset(&mut self) {
                $crate::Stage::reset(&mut self.chain);
            }
            #[inline]
            fn tick(&mut self, x: f32) -> f32 {
                $crate::Stage::tick(&mut self.chain, x)
            }
            #[inline]
            fn process(&mut self, block: &mut [f32]) {
                $crate::Stage::process(&mut self.chain, block);
            }
            fn latency(&self) -> u32 {
                $crate::Stage::latency(&self.chain)
            }
            fn tail(&self) -> $crate::Tail {
                $crate::Stage::tail(&self.chain)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::behavioral::shapes;
    use crate::{Biquad, CurveStage, FilterSpec, Stage, StageKind, SwapPoint};

    crate::chain! {
        struct TestChain {
            drive: Curve = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(1.5)),
            tone: Filter = Biquad::new(FilterSpec::HighShelf { hz: 1_000.0, db: 0.0 }),
        }
    }

    #[test]
    fn named_slots_are_ids_and_typed_access() {
        let mut ch = TestChain::new();
        ch.prepare(48_000.0);
        // Field = the id; the topology names match declaration order.
        let topo: Vec<_> = ch.as_chain().topology().collect();
        assert_eq!(topo, vec![(ch.drive, "curve"), (ch.tone, "filter")]);
        // Method = the typed stage, no kind ritual.
        ch.tone().set(FilterSpec::HighShelf {
            hz: 2_000.0,
            db: 3.0,
        });
        // Ticks like any Stage (nests in regions).
        let y = ch.tick(0.25);
        assert!(y.is_finite());
        // Sense endpoints work through the id.
        assert_eq!(ch.as_chain().sense(ch.tone), y);
    }

    #[test]
    #[should_panic(expected = "slot `drive` no longer holds")]
    fn typed_access_panics_loud_after_a_swap() {
        let mut ch = TestChain::new();
        ch.prepare(48_000.0);
        let mut bench = SwapPoint::parking(StageKind::Bypass);
        let drive_id = ch.drive;
        assert!(bench.engage(ch.as_chain_mut(), drive_id, |s| matches!(
            s,
            StageKind::Bypass
        )));
        let _ = ch.drive(); // swapped away — must scream, not lie
    }
}

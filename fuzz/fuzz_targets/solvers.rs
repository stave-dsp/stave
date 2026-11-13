//! The circuit solvers under fuzzing (DSP.md R7): the bytes pick a deck,
//! a method (DK, DK with ADAA, WDF), a rate from 8 to 768 kHz, a drawn
//! unit of its parts (E10's tolerances, up to ±50 %), control positions
//! and a hot or odd drive — `stave_test::solver_fuzz::Case::from_bytes`.
//! A case must not panic, must stay finite, and must report every cap
//! hit through N8's health account (and agree with a harder solve while
//! it reports none). A crash's input goes into
//! `crates/stave-test/tests/solver_fuzz.rs`'s `REGRESSIONS`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_test::solver_fuzz::{run, Case};

fuzz_target!(|data: &[u8]| {
    let case = Case::from_bytes(data);
    if let Err(failure) = run(&case) {
        panic!("{failure}");
    }
});

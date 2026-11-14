//! [`plugin_checks!`](macro@crate::plugin_checks): every check a plugin owes,
//! from one line in one test file.
//!
//! ```ignore
//! // tests/checks.rs
//! stave_test::plugin_checks!(stave_tilt_vst3::Tilt);
//!
//! // or, with what only this plugin can say:
//! stave_test::plugin_checks!(stave_tilt_vst3::Tilt, {
//!     perf: vec![PerfCase { name: "FULL TILT", set: &[("Tilt", 6.0)] }],
//!     off_face: &["Oversampling"],
//! });
//! ```
//!
//! It mounts, as separate tests: stave-host's validation through the
//! plugin's VST3 entry points (`passes_validation::*`, a test per check
//! and the per-param checks in parts, so they run side by side — see
//! [`crate::validation`]), and on macOS through its Audio Unit
//! (`passes_validation_as_an_audio_unit`); no allocation in `process` while every
//! parameter moves (debug builds; the file's global allocator is the
//! guard's, so give the macro a test file of its own); the perf check
//! against the plugin's `CPU_BUDGET` (release builds); the face's bindings, grabs and interaction laws (if
//! it has a face); and the host-behaviour checks of [`crate::checks`].

use crate::perf::PerfCase;

/// What a plugin tells its checks. Every field has a default.
pub struct Config {
    /// The perf check's cases (default: the default values); the
    /// searched worst parameter state is always added.
    pub perf: Vec<PerfCase>,
    /// Parameters no grab on the face moves (host-only settings).
    pub off_face: &'static [&'static str],
    /// Seeds for random automation and face sessions.
    pub seeds: core::ops::Range<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config { perf: vec![PerfCase { name: "DEFAULTS", set: &[] }], off_face: &[], seeds: 0..4 }
    }
}

/// Every check a plugin owes. See the [module docs](mod@crate::plugin_checks).
#[macro_export]
macro_rules! plugin_checks {
    ($krate:ident :: $plugin:ident) => {
        $crate::plugin_checks!($krate::$plugin, {});
    };
    ($krate:ident :: $plugin:ident, { $($key:ident : $value:expr),* $(,)? }) => {
        #[allow(unused_imports)]
        use $crate::perf::PerfCase;

        // The no-allocation guard, for the debug-only check: assert_no_alloc
        // compiles the guard out of release builds, where the perf check
        // runs on the system allocator.
        #[cfg(debug_assertions)]
        #[global_allocator]
        static STAVE_CHECKS_ALLOCATOR: $crate::__alloc::AllocDisabler = $crate::__alloc::AllocDisabler;

        #[allow(clippy::needless_update)]
        fn stave_checks_config() -> $crate::plugin_checks::Config {
            $crate::plugin_checks::Config { $($key: $value,)* ..::core::default::Default::default() }
        }

        fn stave_checks_name() -> &'static str {
            <$krate::$plugin as $crate::__stave::Plugin>::INFO.name
        }

        fn stave_checks_has_face() -> bool {
            let params = <<$krate::$plugin as $crate::__stave::Plugin>::Params as ::core::default::Default>::default();
            <$krate::$plugin as $crate::__stave::Plugin>::face(&params).is_some()
        }

        /// stave-host's validation suite (stave_test::validation), through the plugin's own VST3
        /// entry points: a test per check, the per-param checks in parts.
        mod passes_validation {
            use $crate::__host::validate::ParamShard;

            fn check(name: &str) {
                $crate::validation::assert_check_passes($crate::__host::entry_points!($krate), name, ParamShard::ALL);
            }

            fn part(name: &str, part: usize) {
                let params = ParamShard::new(part, $crate::validation::PARTS);
                $crate::validation::assert_check_passes($crate::__host::entry_points!($krate), name, params);
            }

            $crate::__validation_tests! {
                whole: reproducible, no_nan_inf, self_noise, silence_tail, deadline_headroom,
                    state_roundtrip, channels_linked, channels_split, bypass_honest;
                parts: param_sweep, params_glide, solver_converges;
            }
        }

        /// The whole validation suite through the plugin's Audio Unit, driven through AudioToolbox (`stave_test::validation::assert_au_passes`). macOS only.
        #[test]
        #[cfg(target_os = "macos")]
        #[cfg_attr(miri, ignore)]
        fn passes_validation_as_an_audio_unit() {
            let id = $crate::__stave::identity::<$krate::$plugin>();
            $crate::validation::assert_au_passes(id.au, stave_checks_name(), $krate::StaveAUFactory);
        }

        /// No allocation in `process` while every parameter moves ([`crate::no_alloc`]). Debug builds only.
        #[test]
        #[cfg(debug_assertions)]
        fn process_never_allocates() {
            $crate::no_alloc::check_no_alloc::<$krate::$plugin>(stave_checks_name(), &$crate::no_alloc::NoAllocConfig::default());
        }

        /// The perf check ([`crate::perf`]): the configured cases and the searched worst parameter state, CPU ns a sample a channel at 48 kHz; fails over `Plugin::CPU_BUDGET`, naming the hotspot stage; reports only when no budget is declared. Skips in debug builds.
        #[test]
        fn perf_within_budget() {
            $crate::perf::perf_check::<$krate::$plugin>(stave_checks_name(), &stave_checks_config().perf, &$crate::perf::PerfConfig::default());
        }

        /// Every part on the face binds a parameter the plugin declares ([`crate::face::binds_resolve`]). Only with a face.
        #[test]
        fn face_binds_resolve() {
            if stave_checks_has_face() {
                $crate::face::binds_resolve::<$krate::$plugin>(stave_checks_name());
            }
        }

        /// A press at the centre of each named part's grabs grabs that part ([`crate::face::grabs_land`]). Only with a face.
        #[test]
        fn face_grabs_land() {
            if stave_checks_has_face() {
                $crate::face::grabs_land::<$krate::$plugin>(stave_checks_name());
            }
        }

        /// The layout, gesture and overlay laws on every state of the face, and seeded random sessions ([`crate::face::laws`]). Only with a face.
        #[test]
        #[cfg_attr(miri, ignore)]
        fn face_keeps_the_interaction_laws() {
            if stave_checks_has_face() {
                let c = stave_checks_config();
                let laws = $crate::face::laws::Laws { off_face: c.off_face };
                let make = $crate::face::FaceDriver::of::<$krate::$plugin>;
                $crate::face::laws::layout(stave_checks_name(), &make, &laws);
                $crate::face::laws::gestures(stave_checks_name(), &make);
                $crate::face::laws::overlays(stave_checks_name(), &make);
                $crate::face::laws::sessions(stave_checks_name(), &make, c.seeds, 200);
            }
        }

        /// Blocks of 0, 1, odd and full sizes and a re-prepare to a larger maximum: output stays finite ([`crate::checks::block_sizes`]).
        #[test]
        #[cfg_attr(miri, ignore)]
        fn any_block_size() {
            $crate::checks::block_sizes::<$krate::$plugin>();
        }

        /// Prepared from 8 kHz to 768 kHz in a changing order: output stays finite ([`crate::checks::sample_rates`]).
        #[test]
        #[cfg_attr(miri, ignore)]
        fn any_sample_rate() {
            $crate::checks::sample_rates::<$krate::$plugin>();
        }

        /// Prepare, reset and process interleaved; after a reset the plugin sounds as it did fresh ([`crate::checks::prepare_and_reset_cycles`]).
        #[test]
        #[cfg_attr(miri, ignore)]
        fn prepare_and_reset_cycles() {
            $crate::checks::prepare_and_reset_cycles::<$krate::$plugin>();
        }

        /// Every parameter moved at random every block, smoothed and sample-accurate, for each seed ([`crate::checks::random_automation`]).
        #[test]
        #[cfg_attr(miri, ignore)]
        fn random_automation() {
            $crate::checks::random_automation::<$krate::$plugin>(stave_checks_config().seeds);
        }

        /// Random values saved and loaded into a fresh instance: every value comes back and both sound the same ([`crate::checks::state_round_trip`]).
        #[test]
        fn state_round_trip() {
            $crate::checks::state_round_trip::<$krate::$plugin>();
        }

        /// Two fresh instances fed the same input give bit-identical output ([`crate::checks::deterministic`]).
        #[test]
        fn deterministic() {
            $crate::checks::deterministic::<$krate::$plugin>();
        }
    };
}

/// The validation tests [`plugin_checks!`](macro@crate::plugin_checks)
/// mounts: one per whole check, [`PARTS`](crate::validation::PARTS) per
/// per-param check, and a test that they are every check of the suite.
#[doc(hidden)]
#[macro_export]
macro_rules! __validation_tests {
    (whole: $($whole:ident),* ; parts: $($parts:ident),* ;) => {
        $(
            #[test]
            fn $whole() {
                check(stringify!($whole));
            }
        )*
        $(
            mod $parts {
                #[test]
                fn part_1_of_4() {
                    super::part(stringify!($parts), 0);
                }
                #[test]
                fn part_2_of_4() {
                    super::part(stringify!($parts), 1);
                }
                #[test]
                fn part_3_of_4() {
                    super::part(stringify!($parts), 2);
                }
                #[test]
                fn part_4_of_4() {
                    super::part(stringify!($parts), 3);
                }
            }
        )*

        #[test]
        fn every_check_is_mounted() {
            let mut mounted = vec![$(stringify!($whole),)* $(stringify!($parts),)*];
            let mut suite: Vec<&str> = $crate::__host::validate::ALL.iter().map(|c| c.name).collect();
            mounted.sort_unstable();
            suite.sort_unstable();
            assert_eq!(mounted, suite, "plugin_checks! mounts other checks than stave-host's suite runs");
        }
    };
}

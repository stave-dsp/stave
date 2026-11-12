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
//! plugin's VST3 entry points; no allocation in `process` while every
//! parameter moves (debug builds; the file's global allocator is the
//! guard's, so give the macro a test file of its own); the perf report
//! (release builds); the face's bindings, grabs and interaction laws (if
//! it has a face); and the host-behaviour checks of [`crate::checks`].

use crate::perf::PerfCase;

/// What a plugin tells its checks. Every field has a default.
pub struct Config {
    /// The perf report's cases (default: the default values).
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

        #[test]
        fn passes_validation() {
            $crate::validation::assert_passes($crate::__host::entry_points!($krate));
        }

        #[test]
        #[cfg(debug_assertions)]
        fn process_never_allocates() {
            $crate::no_alloc::check_no_alloc::<$krate::$plugin>(stave_checks_name(), &$crate::no_alloc::NoAllocConfig::default());
        }

        #[test]
        fn perf_report_for_the_record() {
            $crate::perf::perf_report::<$krate::$plugin>(stave_checks_name(), &stave_checks_config().perf, &$crate::perf::PerfConfig::default());
        }

        #[test]
        fn face_binds_resolve() {
            if stave_checks_has_face() {
                $crate::face::binds_resolve::<$krate::$plugin>(stave_checks_name());
            }
        }

        #[test]
        fn face_grabs_land() {
            if stave_checks_has_face() {
                $crate::face::grabs_land::<$krate::$plugin>(stave_checks_name());
            }
        }

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

        #[test]
        #[cfg_attr(miri, ignore)]
        fn any_block_size() {
            $crate::checks::block_sizes::<$krate::$plugin>();
        }

        #[test]
        #[cfg_attr(miri, ignore)]
        fn any_sample_rate() {
            $crate::checks::sample_rates::<$krate::$plugin>();
        }

        #[test]
        #[cfg_attr(miri, ignore)]
        fn prepare_and_reset_cycles() {
            $crate::checks::prepare_and_reset_cycles::<$krate::$plugin>();
        }

        #[test]
        #[cfg_attr(miri, ignore)]
        fn random_automation() {
            $crate::checks::random_automation::<$krate::$plugin>(stave_checks_config().seeds);
        }

        #[test]
        fn state_round_trip() {
            $crate::checks::state_round_trip::<$krate::$plugin>();
        }

        #[test]
        fn deterministic() {
            $crate::checks::deterministic::<$krate::$plugin>();
        }
    };
}

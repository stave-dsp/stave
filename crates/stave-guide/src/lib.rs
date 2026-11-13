// The guide's articles are Markdown files beside this one, each the docs
// of an empty module: rustdoc resolves every link in them against the
// real API and runs every Rust example as a doctest, so an article that
// drifts from the code fails the build. tools/docsite turns rustdoc's
// pages for this crate into the site's guides; the module tree below
// is the site's table of contents, in order.
#![doc = include_str!("index.md")]

#![forbid(unsafe_code)]

#[doc = include_str!("getting-started.md")]
pub mod getting_started {}

pub mod tutorials {
    #![doc = include_str!("tutorials/index.md")]

    #[doc = include_str!("tutorials/first-plugin.md")]
    pub mod first_plugin {}
    #[doc = include_str!("tutorials/face-and-meters.md")]
    pub mod face_and_meters {}
    #[doc = include_str!("tutorials/prove-it.md")]
    pub mod prove_it {}
}

pub mod concepts {
    #![doc = include_str!("concepts/index.md")]

    #[doc = include_str!("concepts/lifecycle.md")]
    pub mod lifecycle {}
    #[doc = include_str!("concepts/params.md")]
    pub mod params {}
    #[doc = include_str!("concepts/faces.md")]
    pub mod faces {}
    #[doc = include_str!("concepts/telemetry.md")]
    pub mod telemetry {}
    #[doc = include_str!("concepts/testing.md")]
    pub mod testing {}
    #[doc = include_str!("concepts/verification.md")]
    pub mod verification {}
    #[doc = include_str!("concepts/dsp.md")]
    pub mod dsp {}
    #[doc = include_str!("concepts/architecture.md")]
    pub mod architecture {}
}

pub mod howto {
    #![doc = include_str!("howto/index.md")]

    #[doc = include_str!("howto/host-tools.md")]
    pub mod host_tools {}
    #[doc = include_str!("howto/project-layout.md")]
    pub mod project_layout {}
    #[doc = include_str!("howto/performance.md")]
    pub mod performance {}
    #[doc = include_str!("howto/model-placement.md")]
    pub mod model_placement {}
    #[doc = include_str!("howto/release.md")]
    pub mod release {}
}

#[doc = include_str!("glossary.md")]
pub mod glossary {}

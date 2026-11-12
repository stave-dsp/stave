//! The storybook's stories and stand-in plugin, as a library: the
//! binary shows them (live, or as screenshots), and `tests/` holds every
//! story to the interaction laws and its golden images. The catalogue
//! (one picture per part) is shared with the documentation site.

#![forbid(unsafe_code)]

pub mod catalogue;
pub mod controller;
pub mod stories;

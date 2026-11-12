# The Stave guide

Stave is a Rust framework for VST3 and CLAP audio plugins, with the host
ABIs, DSP, UI, testing and measurement written from scratch in one
workspace. A plugin is one trait and one macro; realtime safety,
validation and measurement come with it rather than after it.

This guide teaches the framework. Every page in it is checked against
the code: rustdoc resolves every link, and every Rust example is
compiled and run as a test, so an article cannot drift from the API it
describes.

- [Getting started](crate::getting_started): from nothing to a plugin
  loaded in a DAW.
- [Tutorials](crate::tutorials): step-by-step lessons, each step's code
  compiled and run.
- [Concepts](crate::concepts): how the framework works, from the plugin
  lifecycle to the DSP library and verification.
- [How-to guides](crate::howto): recipes for the tools, project layout,
  performance and releases.
- [Glossary](crate::glossary): one word per concept, the vocabulary the
  code and its gates hold to.

The API reference for each crate starts at [`stave`](stave), the one
dependency a plugin takes.

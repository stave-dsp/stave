//! An instance under check that hands what its plugin logged to the
//! check's result when it is dropped.

use std::cell::RefCell;
use std::rc::Rc;

use crate::param::{Param, ParamError};
use crate::instance::{Instance, Layout, PluginMessage, TapReading, StateError, Tail};
use stave_core::Event;

pub(super) struct Logged {
    pub inner: Box<dyn Instance>,
    pub into: Rc<RefCell<Vec<PluginMessage>>>,
}

impl Drop for Logged {
    fn drop(&mut self) {
        self.into.borrow_mut().extend(self.inner.messages(0));
    }
}

impl Instance for Logged {
    fn layout(&self) -> Layout {
        self.inner.layout()
    }
    fn latency(&self) -> u32 {
        self.inner.latency()
    }
    fn tail(&self) -> Tail {
        self.inner.tail()
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], events: &[Event]) {
        self.inner.process(inputs, outputs, events)
    }
    fn params(&self) -> Vec<Param> {
        self.inner.params()
    }
    fn set_param(&mut self, name: &str, plain: f64) -> Result<(), ParamError> {
        self.inner.set_param(name, plain)
    }
    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        self.inner.save_state()
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        self.inner.load_state(bytes)
    }
    fn taps(&self) -> Vec<TapReading> {
        self.inner.taps()
    }
    fn messages(&self, from: u64) -> Vec<PluginMessage> {
        self.inner.messages(from)
    }
}

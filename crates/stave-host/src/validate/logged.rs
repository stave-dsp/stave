//! An instance under check that hands what its plugin logged to the
//! check's result when it is dropped.

use std::cell::RefCell;
use std::rc::Rc;

use crate::control::{Control, ControlError};
use crate::instance::{Instance, Layout, PluginMessage, TapReading, StateError, Tail};
use crate::note::NoteEvent;

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
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[NoteEvent]) {
        self.inner.process(inputs, outputs, notes)
    }
    fn controls(&self) -> Vec<Control> {
        self.inner.controls()
    }
    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.inner.set_control(name, plain)
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

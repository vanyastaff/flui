//! An output by its global's name, and the outputs the compositor announces now.

use std::cell::RefCell;
use std::rc::Rc;

/// An output by its global's name, which the compositor numbers alike for
/// every client: what a surface's `enter` names, and what the display
/// source keys an output by. Public where it is defined, as the testing
/// kit names it, and nameable only through it outside the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputName(u32);

impl OutputName {
    /// The output whose global is `name`.
    pub(crate) const fn new(name: u32) -> Self {
        Self(name)
    }

    /// The global's name.
    pub(crate) const fn get(self) -> u32 {
        self.0
    }
}

/// The outputs the compositor announces now, which the registry keeps
/// current on the owner thread: an output removed while a surface was on
/// it sends that surface no `leave`.
#[derive(Clone, Default)]
pub(crate) struct LiveOutputs(Rc<RefCell<Vec<OutputName>>>);

impl LiveOutputs {
    /// Whether `output` is still announced.
    pub(crate) fn contains(&self, output: OutputName) -> bool {
        self.0.borrow().contains(&output)
    }

    /// Every output announced now.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn names(&self) -> Vec<OutputName> {
        self.0.borrow().clone()
    }

    pub(super) fn add(&self, output: OutputName) {
        let mut live = self.0.borrow_mut();
        if !live.contains(&output) {
            live.push(output);
        }
    }

    pub(super) fn remove(&self, output: OutputName) {
        self.0.borrow_mut().retain(|held| *held != output);
    }
}

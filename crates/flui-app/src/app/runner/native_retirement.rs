//! Native owners removed under short registry borrows and retired afterwards.

use std::{cell::RefCell, rc::Rc};

use crate::app::close_request::CloseRequestHandler;
use crate::app::lifecycle_state::preserve_first_lifecycle_panic;

use super::frame_driver::FrameDriver;

enum RetiredNativeOwner {
    Frame(FrameDriver),
    CloseHandler(CloseRequestHandler),
}

/// A strong completion handle survives replacement or teardown of app TLS.
#[derive(Clone, Default)]
pub(in crate::app) struct NativeRetirement(Rc<RefCell<Vec<RetiredNativeOwner>>>);

impl NativeRetirement {
    pub(super) fn frame(&self, driver: FrameDriver) {
        self.0.borrow_mut().push(RetiredNativeOwner::Frame(driver));
    }

    pub(in crate::app) fn close_handlers(
        &self,
        handlers: impl IntoIterator<Item = CloseRequestHandler>,
    ) {
        for handler in handlers {
            self.0
                .borrow_mut()
                .push(RetiredNativeOwner::CloseHandler(handler));
        }
    }

    /// Every owner is released independently, outside registry and store guards.
    pub(super) fn drain(&self, first_panic: &mut Option<Box<dyn std::any::Any + Send>>) {
        let owners = std::mem::take(&mut *self.0.borrow_mut());
        for owner in owners {
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match owner {
                RetiredNativeOwner::Frame(driver) => drop(driver),
                RetiredNativeOwner::CloseHandler(handler) => drop(handler),
            }))
            .err();
            preserve_first_lifecycle_panic(first_panic, failure, "retired native owner");
        }
    }
}

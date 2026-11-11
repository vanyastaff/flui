//! Native owners removed under short registry borrows and retired afterwards.

use std::{cell::RefCell, rc::Rc};

use crate::app::close_request::CloseRequestHandler;
use flui_foundation::panic::FirstPanic;

use super::frame_driver::FrameDriver;

enum RetiredNativeOwner {
    TextSizing(Rc<super::native_text_sizing::NativeTextSizing>),
    Frame(FrameDriver),
    CloseHandler(CloseRequestHandler),
}

/// A strong completion handle survives replacement or teardown of app TLS.
#[derive(Clone, Default)]
pub(in crate::app) struct NativeRetirement(Rc<RefCell<Vec<RetiredNativeOwner>>>);

impl NativeRetirement {
    pub(super) fn text_sizing(&self, sizing: Rc<super::native_text_sizing::NativeTextSizing>) {
        self.0
            .borrow_mut()
            .push(RetiredNativeOwner::TextSizing(sizing));
    }
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
    pub(super) fn drain(&self, first_panic: &mut FirstPanic) {
        let owners = std::mem::take(&mut *self.0.borrow_mut());
        for owner in owners {
            // Each retired owner drops contained; the first panic is kept.
            first_panic.contain(|| match owner {
                RetiredNativeOwner::TextSizing(sizing) => drop(sizing),
                RetiredNativeOwner::Frame(driver) => drop(driver),
                RetiredNativeOwner::CloseHandler(handler) => drop(handler),
            });
        }
    }
}

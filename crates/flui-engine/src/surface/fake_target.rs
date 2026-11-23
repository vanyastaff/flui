//! A GPU-free [`WindowTarget`] test double for `surface/lease.rs`'s unit
//! tests, kept in one place so a later suite uses this fake rather than a
//! copy that could silently diverge.
//!
//! Hands out plain Xlib IDs (no pointer, so `WindowHandle::borrow_raw`/
//! `DisplayHandle::borrow_raw` have no pointee whose validity to argue
//! about).
//!
//! [`WindowTarget`]: crate::surface::target::WindowTarget

use std::ffi::c_ulong;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XlibDisplayHandle, XlibWindowHandle,
};

pub(crate) struct FakeTarget {
    window_id: c_ulong,
}

impl FakeTarget {
    pub(crate) fn new(window_id: c_ulong) -> Self {
        Self { window_id }
    }
}

impl HasWindowHandle for FakeTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(self.window_id));
        #[expect(unsafe_code, reason = "test double, no FFI island")]
        // SAFETY: an Xlib window handle is a plain integer ID (`c_ulong`),
        // not a pointer — `borrow_raw`'s safety contract is entirely about
        // pointer fields staying valid, and there is no pointee here for it
        // to protect.
        let handle = unsafe { WindowHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

impl HasDisplayHandle for FakeTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
        #[expect(unsafe_code, reason = "test double, no FFI island")]
        // SAFETY: `display: None` carries no pointer at all, so there is no
        // pointee for `borrow_raw`'s safety contract to protect.
        let handle = unsafe { DisplayHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

//! A GPU-free [`WindowTarget`] test double, shared by `surface_lease.rs`'s
//! and `renderer.rs`'s own unit tests so both exercise the identical fake
//! rather than two copies that could silently diverge (issue #1043).
//!
//! Hands out plain Xlib IDs (no pointer, so `WindowHandle::borrow_raw`/
//! `DisplayHandle::borrow_raw` have no pointee whose validity to argue
//! about), records every probe call in order, and can be switched to answer
//! `Unavailable` at construction or mid-test.

use std::ffi::c_ulong;
use std::sync::{Arc, Mutex};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XlibDisplayHandle, XlibWindowHandle,
};

use crate::error::{EngineError, EngineResult};
use crate::window_target::WindowTarget;

pub(crate) struct FakeTarget {
    window_id: Mutex<c_ulong>,
    available: Mutex<bool>,
    calls: Mutex<Vec<&'static str>>,
}

impl FakeTarget {
    pub(crate) fn new(window_id: c_ulong) -> Self {
        Self {
            window_id: Mutex::new(window_id),
            available: Mutex::new(true),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A target that reports `Unavailable` from its very first probe call —
    /// for pinning the "fails before any GPU work starts" contract without
    /// caring what window ID it would otherwise have handed out.
    pub(crate) fn unavailable() -> Self {
        let target = Self::new(1);
        target.set_available(false);
        target
    }

    pub(crate) fn set_available(&self, available: bool) {
        *self
            .available
            .lock()
            .expect("BUG: test-only mutex is never poisoned") = available;
    }

    pub(crate) fn set_window_id(&self, window_id: c_ulong) {
        *self
            .window_id
            .lock()
            .expect("BUG: test-only mutex is never poisoned") = window_id;
    }

    pub(crate) fn call_log(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .expect("BUG: test-only mutex is never poisoned")
            .clone()
    }
}

impl HasWindowHandle for FakeTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.calls
            .lock()
            .expect("BUG: test-only mutex is never poisoned")
            .push("window_handle");
        if !*self
            .available
            .lock()
            .expect("BUG: test-only mutex is never poisoned")
        {
            return Err(HandleError::Unavailable);
        }
        let window_id = *self
            .window_id
            .lock()
            .expect("BUG: test-only mutex is never poisoned");
        let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(window_id));
        // SAFETY: an Xlib window handle is a plain integer ID (`c_ulong`),
        // not a pointer — `borrow_raw`'s safety contract is entirely about
        // pointer fields staying valid, and there is no pointee here for it
        // to protect.
        #[expect(unsafe_code)] // test double, no FFI island: see SAFETY above
        let handle = unsafe { WindowHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

impl HasDisplayHandle for FakeTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.calls
            .lock()
            .expect("BUG: test-only mutex is never poisoned")
            .push("display_handle");
        if !*self
            .available
            .lock()
            .expect("BUG: test-only mutex is never poisoned")
        {
            return Err(HandleError::Unavailable);
        }
        let raw = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
        // SAFETY: `display: None` carries no pointer at all, so there is no
        // pointee for `borrow_raw`'s safety contract to protect.
        #[expect(unsafe_code)] // test double, no FFI island: see SAFETY above
        let handle = unsafe { DisplayHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

/// Reads back the Xlib window ID a `FakeTarget`-backed `Arc<dyn
/// WindowTarget>` currently hands out, through the real `HasWindowHandle`
/// trait method — not a backdoor accessor — so tests build/rebuild a
/// minimal `S` stand-in via the same call production code makes.
pub(crate) fn xlib_window_id(target: &Arc<dyn WindowTarget>) -> EngineResult<c_ulong> {
    let raw = target
        .window_handle()
        .map_err(EngineError::surface_target_unavailable)?
        .as_raw();
    match raw {
        RawWindowHandle::Xlib(handle) => Ok(handle.window),
        _ => unreachable!("BUG: FakeTarget only ever hands out Xlib handles"),
    }
}

//! The frame driver's seam (ADR-0382).
//!
//! A presentation's frame driver owns its root registry through a
//! [`VsyncDriver`]: it ticks the registry once per frame, binds it to the
//! presentation's frame requester and build phase, and closes it with a
//! [`VsyncRetirement`] when the presentation retires. The tree receives only
//! the [`Vsync`](crate::Vsync) handle, which cannot do any of that. The
//! intended users are `flui-runtime`, and tests that drive a registry without
//! a host.
//!
//! **No semver promise.** Anything here may change or disappear in any
//! release. The module is `#[doc(hidden)]`, and the whole-crate re-exports in
//! `flui-sdk` and the `flui` facade shadow it, so `flui_sdk::animation::__runtime`
//! and `flui::animation::__runtime` do not resolve.

pub use crate::vsync::{VsyncDriver, VsyncRetirement};

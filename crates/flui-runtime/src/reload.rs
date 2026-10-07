//! The development reload a UI runtime applies to its presentations.
//!
//! A reload driver is a `flui_view::dev_reload::DevReloadHook` the
//! application installs (ADR-0094 §1); the host, `flui-app`, polls it and
//! translates its event into this crate's [`ReloadTier`], which the UI runtime
//! applies. Nothing here or in the host names a reload tool. The module
//! exists only with this crate's `hot-reload` feature, which `flui-app`
//! turns on; it names no reload crate, so it adds nothing to a production
//! graph.

/// How much of a presentation a development reload replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadTier {
    /// Rebuild the retained element tree and re-lay-out the render tree,
    /// keeping every `State`.
    Reassemble,
    /// Remount the root widget, disposing its state, in the same process.
    Restart,
    /// Replace the whole process. The process supervisor does this; a UI runtime
    /// has nothing to apply.
    ProcessRestart,
}

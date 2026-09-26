//! The development reload a realm applies to its presentations.
//!
//! A hot-reload driver lives in a host crate (`flui-hot-reload`, reached only
//! through `flui-app`'s `hot-reload` feature); the realm that applies a reload
//! lives here and may not depend on it. The driver's tier is therefore
//! translated at the host into this crate's [`ReloadTier`], and nothing below
//! the host names a hot-reload type. The module exists only with this
//! crate's `hot-reload` feature, which `flui-app`'s feature of the same name
//! turns on, so a production graph carries none of it.

/// How much of a presentation a development reload replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadTier {
    /// Rebuild the retained element tree and re-lay-out the render tree,
    /// keeping every `State`.
    Reassemble,
    /// Remount the root widget, disposing its state, in the same process.
    Restart,
    /// Replace the whole process. The process supervisor does this; a realm
    /// has nothing to apply.
    ProcessRestart,
}

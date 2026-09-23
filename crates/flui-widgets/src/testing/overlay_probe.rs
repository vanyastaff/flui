//! Read-only inspection of an [`OverlayHandle`]'s entry list, for tests.
//!
//! The overlay's public API (ADR-0076) mutates the list but never exposes it:
//! nothing outside a test needs to read the stacking order back. Tests of the
//! overlay and of the navigator built on it do, so they import
//! [`OverlayProbe`] and call `handle.entry_ids()`.

use crate::overlay::{OverlayEntryId, OverlayHandle};

/// Test-only reads of an overlay's entry list.
pub trait OverlayProbe {
    /// The ids of the entries in the list, bottom → top (the last one paints
    /// on top), whether or not the overlay is mounted.
    fn entry_ids(&self) -> Vec<OverlayEntryId>;
}

impl OverlayProbe for OverlayHandle {
    fn entry_ids(&self) -> Vec<OverlayEntryId> {
        self.ids_bottom_to_top()
    }
}

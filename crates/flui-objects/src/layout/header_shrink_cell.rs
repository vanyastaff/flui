//! [`HeaderShrinkCell`] — the sliver-header half of the build-during-layout
//! seam (ADR-0017).
//!
//! Same mailbox shape as [`LayoutConstraintsCell`](super::LayoutConstraintsCell),
//! carrying what a persistent header's delegate is rebuilt against instead of
//! box constraints: how far the header has shrunk, and whether it currently
//! overlaps the content scrolling beneath it.
//!
//! # Why a second cell rather than a generic one
//!
//! The two publish different payloads and are read by different elements, but
//! are *serviced* identically. That shared half is
//! [`BuildDuringLayoutCell`], which the registry
//! stores type-erased; the payload stays on the concrete type, where only the
//! owning element looks at it.
//!
//! # Why the payload is a pair and not just the offset
//!
//! `overlaps_content` changes independently of `shrink_offset` — a pinned
//! header at rest keeps a constant shrink offset while content scrolls under
//! it, and Flutter's delegates use that flag to raise an elevation or draw a
//! divider. Publishing only the offset would leave those rebuilds unscheduled,
//! and the bug would look like "the shadow appears one scroll late".

use parking_lot::Mutex;

use super::BuildDuringLayoutCell;

/// What a persistent header's delegate is rebuilt against.
///
/// `PartialEq` is the edge-trigger's comparison, so the derive is load-bearing
/// rather than incidental.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderShrink {
    /// How far the header has scrolled away, clamped to its max extent.
    pub shrink_offset: f64,
    /// Whether content is currently scrolling beneath this header.
    pub overlaps_content: bool,
}

/// Interior state of a [`HeaderShrinkCell`].
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct CellState {
    published: Option<HeaderShrink>,
    last_built: Option<HeaderShrink>,
    needs_build: bool,
}

/// Shared shrink-state mailbox between a persistent header's render object and
/// its element.
///
/// The `Mutex` is private and no guard crosses the API boundary.
#[derive(Debug, Default)]
pub struct HeaderShrinkCell {
    inner: Mutex<CellState>,
}

impl HeaderShrinkCell {
    /// Creates an empty cell: nothing published, nothing built, not dirty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the shrink state this header was just laid out with.
    ///
    /// Raises `needs_build` **iff** the value differs from the last committed
    /// one — including the first publish, where nothing has been built yet.
    ///
    /// Called from `perform_layout`; must not allocate or rebuild.
    pub fn publish(&self, shrink: HeaderShrink) {
        let mut state = self.inner.lock();
        if state.last_built != Some(shrink) {
            state.needs_build = true;
        }
        state.published = Some(shrink);
    }

    /// Records shrink state and schedules a rebuild **regardless** of whether
    /// the value changed.
    ///
    /// The header's own gate has a second trigger the payload cannot express:
    /// changing `min_extent`/`max_extent` forces a rebuild even when the
    /// header has not moved. Routing that through [`publish`](Self::publish)
    /// would compare an unchanged pair, leave `needs_build` false, and drop
    /// the very rebuild the extent change demanded.
    pub fn publish_forced(&self, shrink: HeaderShrink) {
        let mut state = self.inner.lock();
        state.published = Some(shrink);
        state.needs_build = true;
    }

    /// The most recently published shrink state, or `None` before first layout.
    #[must_use]
    pub fn shrink(&self) -> Option<HeaderShrink> {
        self.inner.lock().published
    }
}

impl BuildDuringLayoutCell for HeaderShrinkCell {
    fn needs_build(&self) -> bool {
        self.inner.lock().needs_build
    }

    fn has_published(&self) -> bool {
        self.inner.lock().published.is_some()
    }

    fn commit(&self) {
        let mut state = self.inner.lock();
        state.last_built = state.published;
        state.needs_build = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shrink(offset: f64, overlaps: bool) -> HeaderShrink {
        HeaderShrink {
            shrink_offset: offset,
            overlaps_content: overlaps,
        }
    }

    /// Edge-triggered, not level-triggered. Republishing an unchanged value
    /// after a commit must not re-dirty the element — a level-triggered flag
    /// would re-dirty on every layout pass and the frame would never settle.
    #[test]
    fn republishing_an_unchanged_value_does_not_re_dirty() {
        let cell = HeaderShrinkCell::new();
        cell.publish(shrink(12.0, false));
        cell.commit();
        assert!(!cell.needs_build());

        cell.publish(shrink(12.0, false));
        assert!(!cell.needs_build());
    }
}

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
    pub shrink_offset: f32,
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
/// The `Mutex` is private and no guard crosses the API boundary (SP-6 /
/// port-check "no locks in public API").
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

    fn shrink(offset: f32, overlaps: bool) -> HeaderShrink {
        HeaderShrink {
            shrink_offset: offset,
            overlaps_content: overlaps,
        }
    }

    #[test]
    fn a_fresh_cell_is_clean_and_empty() {
        let cell = HeaderShrinkCell::new();
        assert!(!cell.needs_build());
        assert!(!cell.has_published());
        assert_eq!(cell.shrink(), None);
    }

    /// The first publish must schedule a build: nothing has been built yet, so
    /// "unchanged since last time" is not a reason to skip.
    #[test]
    fn the_first_publish_schedules_a_build() {
        let cell = HeaderShrinkCell::new();
        cell.publish(shrink(0.0, false));
        assert!(cell.needs_build());
        assert!(cell.has_published());
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

    #[test]
    fn a_changed_shrink_offset_re_dirties() {
        let cell = HeaderShrinkCell::new();
        cell.publish(shrink(12.0, false));
        cell.commit();

        cell.publish(shrink(13.0, false));
        assert!(cell.needs_build());
        assert_eq!(cell.shrink(), Some(shrink(13.0, false)));
    }

    /// **The reason the payload is a pair.** A pinned header at rest holds a
    /// constant shrink offset while content scrolls beneath it; only the
    /// overlap flag moves. A cell keyed on the offset alone would leave that
    /// rebuild unscheduled, and the symptom would be a header shadow that
    /// appears one scroll too late.
    #[test]
    fn overlap_changing_alone_re_dirties() {
        let cell = HeaderShrinkCell::new();
        cell.publish(shrink(12.0, false));
        cell.commit();

        cell.publish(shrink(12.0, true));
        assert!(
            cell.needs_build(),
            "overlaps_content moves independently of the shrink offset"
        );
    }

    #[test]
    fn commit_clears_the_flag_and_records_what_was_built() {
        let cell = HeaderShrinkCell::new();
        cell.publish(shrink(4.0, true));
        cell.commit();

        assert!(!cell.needs_build());
        assert_eq!(cell.shrink(), Some(shrink(4.0, true)));
    }

    /// The registry only ever sees the erased half, so it has to work through
    /// it — a cell that behaved differently through `dyn` would strand headers
    /// as un-serviced.
    #[test]
    fn the_erased_handle_reports_the_same_state() {
        let cell = HeaderShrinkCell::new();
        let erased: &dyn BuildDuringLayoutCell = &cell;

        assert!(!erased.needs_build());
        assert!(!erased.has_published());

        cell.publish(shrink(1.0, false));
        assert!(erased.needs_build());
        assert!(erased.has_published());

        erased.commit();
        assert!(!erased.needs_build());
    }
}

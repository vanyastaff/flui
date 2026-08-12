//! [`LayoutConstraintsCell`] — the render-object → element channel of the
//! build-during-layout seam (ADR-0017).
//!
//! # What this is
//!
//! A one-slot mailbox shared (via `Arc`) between a build-during-layout render
//! object and the element that owns it. The render object *publishes* the
//! constraints it was laid out with; the element layer later *reads* them,
//! rebuilds, and *commits*.
//!
//! It exists because FLUI cannot do what Flutter does. Flutter's
//! `LayoutBuilder` rebuilds its child **inside** `performLayout` via
//! `invokeLayoutCallback`, mutating the element and render trees mid-walk. In
//! FLUI the layout walk holds `&mut RenderTree` for its whole duration (the
//! `SubtreeArena`), and building while the pipeline write-lock is held would
//! self-deadlock when mounting render objects. Instead the render object records
//! what it saw, and `BuildOwner::service_layout_builders` services it
//! **between** layout passes, where neither hazard is live. See
//! [`ADR-0017`](../../../../docs/adr/ADR-0017-build-during-layout-callback-seam.md).
//!
//! # Edge-triggered, not level-triggered
//!
//! [`publish`](LayoutConstraintsCell::publish) raises `needs_build` **only when
//! the constraints differ from the last committed ones**. This is the direct
//! analogue of Flutter's skip condition
//! (`_previousConstraints == constraints && !_needsBuild`), and it is what makes
//! "same constraints ⇒ no rebuild" a structural property of the seam rather than
//! something a test merely happens to observe. A level-triggered flag would
//! re-dirty the element on every layout pass and the fixpoint would never
//! converge.
//!
//! # Seam infrastructure
//!
//! This type is public so `flui-view` and `flui-objects` can share the same cell
//! without a dependency cycle. App code should use `LayoutBuilder`; the cell is
//! not exported from any prelude.

use flui_rendering::constraints::BoxConstraints;
use parking_lot::Mutex;

/// Interior state of a [`LayoutConstraintsCell`].
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct CellState {
    /// Constraints recorded by the most recent `perform_layout`.
    published: Option<BoxConstraints>,
    /// Constraints the element last *built* against.
    last_built: Option<BoxConstraints>,
    /// Edge-triggered: set when `published` diverges from `last_built`.
    needs_build: bool,
}

/// The registry-facing half of a build-during-layout mailbox.
///
/// The servicing loop (`BuildOwner::service_layout_builders`) needs to know
/// only three things about a cell: whether its owner must rebuild, whether it
/// has ever been laid out, and when to mark the published value as built. It
/// never reads the payload — the element does that, and the element is the one
/// place that knows the concrete cell type.
///
/// Keeping the payload off this trait is what lets a second kind of
/// build-during-layout node share the registry. `LayoutBuilder` publishes
/// `BoxConstraints`; a sliver persistent header would publish its shrink offset
/// and overlap flag. Both need identical servicing and nothing else in common.
pub trait BuildDuringLayoutCell: std::fmt::Debug + Send + Sync + std::any::Any {
    /// Whether the owning element must rebuild before the next layout pass.
    fn needs_build(&self) -> bool;

    /// Whether anything has been published yet.
    ///
    /// `false` before the node's first layout. A scope with nothing published
    /// has no value to build against, so servicing skips it rather than
    /// building against a placeholder.
    fn has_published(&self) -> bool;

    /// Marks the published value as built, clearing `needs_build`.
    fn commit(&self);

    /// Recover the concrete cell, for a caller that knows which kind it is.
    ///
    /// The registry is deliberately payload-blind, but the *owner* of a cell
    /// knows its type and sometimes needs the payload back out of an erased
    /// handle — a test driving a publish without a real layout pass, or an
    /// inspector reporting what a node was last laid out with.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Shared constraints mailbox between a render object and its element.
///
/// Cloneable only behind an `Arc` — the render object and the
/// `layout_builder_registry` entry hold the same cell.
///
/// The `Mutex` is private and no guard is ever returned across the API
/// boundary (SP-6 / port-check "no locks in public API").
#[derive(Debug, Default)]
pub struct LayoutConstraintsCell {
    inner: Mutex<CellState>,
}

impl LayoutConstraintsCell {
    /// Creates an empty cell: nothing published, nothing built, not dirty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the constraints this node was just laid out with.
    ///
    /// Raises `needs_build` **iff** `constraints` differs from the last
    /// committed value — including the first-ever publish, where `last_built`
    /// is `None` and the builder has therefore never run.
    ///
    /// Called from `perform_layout`; must not allocate or rebuild.
    pub fn publish(&self, constraints: BoxConstraints) {
        let mut state = self.inner.lock();
        if state.last_built != Some(constraints) {
            state.needs_build = true;
        }
        state.published = Some(constraints);
    }

    /// Whether the element must rebuild before the next layout pass.
    #[must_use]
    pub fn needs_build(&self) -> bool {
        self.inner.lock().needs_build
    }

    /// The most recently published constraints, or `None` before first layout.
    ///
    /// This is what a builder is handed — the *real* incoming constraints, never
    /// a placeholder.
    #[must_use]
    pub fn constraints(&self) -> Option<BoxConstraints> {
        self.inner.lock().published
    }

    /// Marks the published constraints as built, clearing `needs_build`.
    ///
    /// Called by `service_layout_builders` after `build_scope` has run the
    /// builder against [`constraints`](Self::constraints).
    pub fn commit(&self) {
        let mut state = self.inner.lock();
        state.last_built = state.published;
        state.needs_build = false;
    }
}

impl BuildDuringLayoutCell for LayoutConstraintsCell {
    fn needs_build(&self) -> bool {
        Self::needs_build(self)
    }

    fn has_published(&self) -> bool {
        Self::constraints(self).is_some()
    }

    fn commit(&self) {
        Self::commit(self);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    fn bc(w: f32) -> BoxConstraints {
        BoxConstraints::tight_for(Some(px(w)), Some(px(10.0)))
    }

    #[test]
    fn layout_builder_cell_starts_clean_and_empty() {
        let cell = LayoutConstraintsCell::new();
        assert!(!cell.needs_build());
        assert_eq!(cell.constraints(), None);
    }

    #[test]
    fn layout_builder_cell_first_publish_needs_build() {
        let cell = LayoutConstraintsCell::new();
        cell.publish(bc(100.0));
        assert!(
            cell.needs_build(),
            "first publish must schedule the builder"
        );
        assert_eq!(cell.constraints(), Some(bc(100.0)));
    }

    #[test]
    fn layout_builder_cell_commit_clears_needs_build() {
        let cell = LayoutConstraintsCell::new();
        cell.publish(bc(100.0));
        cell.commit();
        assert!(!cell.needs_build());
        assert_eq!(cell.constraints(), Some(bc(100.0)));
    }

    /// The edge-trigger: republishing the committed constraints is a no-op.
    /// This is what terminates the layout<->build fixpoint.
    #[test]
    fn layout_builder_cell_same_constraints_do_not_rebuild() {
        let cell = LayoutConstraintsCell::new();
        cell.publish(bc(100.0));
        cell.commit();

        cell.publish(bc(100.0));
        assert!(
            !cell.needs_build(),
            "unchanged constraints must not re-dirty the element"
        );
    }

    #[test]
    fn layout_builder_cell_changed_constraints_rebuild() {
        let cell = LayoutConstraintsCell::new();
        cell.publish(bc(100.0));
        cell.commit();

        cell.publish(bc(200.0));
        assert!(cell.needs_build());
        assert_eq!(cell.constraints(), Some(bc(200.0)));
    }

    /// A change followed by a revert within one pass still leaves the cell
    /// dirty only if the *final* published value differs from the committed
    /// one — `publish` compares against `last_built`, not against the previous
    /// publish.
    #[test]
    fn layout_builder_cell_revert_within_pass_settles_on_last_built() {
        let cell = LayoutConstraintsCell::new();
        cell.publish(bc(100.0));
        cell.commit();

        cell.publish(bc(200.0));
        assert!(cell.needs_build());

        // Republishing the committed value does not *clear* the flag — an
        // intervening build was already scheduled. Only `commit` clears it.
        cell.publish(bc(100.0));
        assert!(cell.needs_build());
        cell.commit();
        assert!(!cell.needs_build());
        assert_eq!(cell.constraints(), Some(bc(100.0)));
    }
}

//! Constraint cache validation methods for `RenderState<P>`.
//!
//! This file contains the constraint methods over the protocol's layout
//! record (`constraints`, `set_constraints`, `clear_constraints`,
//! `has_constraints`).
//!
//! Each layout pass overwrites the constraints. `set_constraints` takes
//! `&mut self`; the production caller (`RenderEntry::layout`) already holds
//! `&mut self` on the entry so the borrow chain reaches the state mutably.

use super::{LayoutRecord as _, RenderState};
use crate::protocol::Protocol;

// ============================================================================
// CONSTRAINTS (CACHE VALIDATION)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the last constraints used for layout.
    ///
    /// Returns `None` if layout has never been performed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(old_constraints) = state.constraints() {
    ///     if old_constraints == new_constraints {
    ///         // Can skip layout - constraints unchanged!
    ///         return state.geometry().unwrap();
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn constraints(&self) -> Option<&P::Constraints> {
        self.layout.constraints()
    }

    /// Sets (or replaces) the constraints used for layout.
    ///
    /// Idempotent — overwrites any prior value. Used for cache validation
    /// against the next layout pass: if `constraints()` matches the incoming
    /// constraints and the node is clean, the layout can be skipped.
    ///
    /// The prior `OnceCell`-backed implementation panicked on second
    /// invocation. Re-layout of the same node now
    /// straight-assigns the constraints. See module-level doc for rationale.
    #[inline]
    pub fn set_constraints(&mut self, constraints: P::Constraints) {
        self.layout.set_constraints(constraints);
    }

    /// Clears the constraints, signalling no prior layout has run.
    ///
    /// Equivalent to `set_constraints` with no value; useful as an explicit
    /// reset before a forced re-layout in tests or eviction paths. Production
    /// re-layout simply calls `set_constraints` with the new value — clearing
    /// first is no longer required (the `OnceCell`-era invariant is gone).
    #[inline]
    pub fn clear_constraints(&mut self) {
        self.layout.clear_constraints();
    }

    /// Checks if constraints match the given value.
    ///
    /// Returns `false` if constraints are not set.
    pub fn has_constraints(&self, constraints: &P::Constraints) -> bool
    where
        P::Constraints: PartialEq,
    {
        self.layout.constraints().is_some_and(|c| c == constraints)
    }
}

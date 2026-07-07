//! Constraint cache validation methods for `RenderState<P>`.
//!
//! This file contains the `Option<ProtocolConstraints<P>>`-backed constraint
//! methods (`constraints`, `set_constraints`, `clear_constraints`,
//! `has_constraints`).
//!
//! **D-block PR-A1 U14 migration (2026-05-23):** the prior `OnceCell`-backed
//! `set_constraints` panicked on second invocation, which made any re-layout
//! of the same node a crash. Flutter `.flutter/.../object.dart:2865`
//! straight-assigns `_constraints = constraints` each layout pass; we mirror
//! that semantics by holding constraints in `Option<T>` and replacing
//! unconditionally inside `set_constraints`. Method now takes `&mut self`;
//! the production caller (`RenderEntry::layout`) already holds `&mut self`
//! on the entry so the borrow chain reaches the state mutably.

use super::RenderState;
use crate::protocol::{Protocol, ProtocolConstraints};

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
    pub fn constraints(&self) -> Option<&ProtocolConstraints<P>> {
        self.constraints.as_ref()
    }

    /// Sets (or replaces) the constraints used for layout.
    ///
    /// Idempotent — overwrites any prior value. Used for cache validation
    /// against the next layout pass: if `constraints()` matches the incoming
    /// constraints and the node is clean, the layout can be skipped.
    ///
    /// **D-block PR-A1 U14**: prior `OnceCell`-backed implementation panicked
    /// on second invocation. Re-layout of the same node now mirrors Flutter's
    /// straight-assignment semantics. See module-level doc for rationale.
    #[inline]
    pub fn set_constraints(&mut self, constraints: ProtocolConstraints<P>) {
        self.constraints = Some(constraints);
    }

    /// Clears the constraints, signalling no prior layout has run.
    ///
    /// Equivalent to `set_constraints` with no value; useful as an explicit
    /// reset before a forced re-layout in tests or eviction paths. Production
    /// re-layout simply calls `set_constraints` with the new value — clearing
    /// first is no longer required (the `OnceCell`-era invariant is gone).
    #[inline]
    pub fn clear_constraints(&mut self) {
        self.constraints = None;
    }

    /// Checks if constraints match the given value.
    ///
    /// Returns `false` if constraints are not set.
    pub fn has_constraints(&self, constraints: &ProtocolConstraints<P>) -> bool
    where
        ProtocolConstraints<P>: PartialEq,
    {
        self.constraints.as_ref().is_some_and(|c| c == constraints)
    }
}

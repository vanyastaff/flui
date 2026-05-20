//! Constraint cache validation methods for `RenderState<P>`.
//!
//! This file contains the write-once `OnceCell` constraint methods
//! (`constraints`, `set_constraints`, `clear_constraints`, `has_constraints`).

use once_cell::sync::OnceCell;

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
    pub fn constraints(&self) -> Option<&ProtocolConstraints<P>> {
        self.constraints.get()
    }

    /// Sets the constraints used for layout.
    ///
    /// Used for cache validation - if constraints haven't changed,
    /// layout can be skipped (for sized-by-parent render objects).
    pub fn set_constraints(&self, constraints: ProtocolConstraints<P>) {
        if self.constraints.set(constraints).is_err() {
            // Constraints already set - clear first!
            panic!(
                "Constraints already set! Call clear_constraints() before relayout. \
                 This indicates a logic error in the layout code."
            );
        }
    }

    /// Clears the constraints to allow relayout.
    #[inline]
    pub fn clear_constraints(&mut self) {
        self.constraints = OnceCell::new();
    }

    /// Checks if constraints match the given value.
    ///
    /// Returns `false` if constraints are not set.
    pub fn has_constraints(&self, constraints: &ProtocolConstraints<P>) -> bool
    where
        ProtocolConstraints<P>: PartialEq,
    {
        self.constraints
            .get()
            .map(|c| c == constraints)
            .unwrap_or(false)
    }
}

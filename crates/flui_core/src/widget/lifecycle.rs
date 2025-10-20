//! State lifecycle management
//!
//! This module defines the StateLifecycle enum that tracks the lifecycle
//! progression of State objects from creation to disposal.

/// State lifecycle progression
///
/// Tracks the lifecycle state of a State object from creation to disposal.
/// This enforces correct lifecycle ordering and prevents invalid operations.
///
/// # Lifecycle Progression
///
/// ```text
/// Created → Initialized → Ready → Defunct
///    ↓           ↓          ↓        ↓
///  new()    initState()  build()  dispose()
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::StateLifecycle;
///
/// let lifecycle = StateLifecycle::Created;
/// assert!(!lifecycle.is_mounted());
/// assert!(!lifecycle.can_build());
///
/// let lifecycle = StateLifecycle::Ready;
/// assert!(lifecycle.is_mounted());
/// assert!(lifecycle.can_build());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateLifecycle {
    /// State object created but initState() not yet called
    Created,
    /// initState() called, ready to build
    Initialized,
    /// State is active and can build/rebuild
    Ready,
    /// dispose() called, state is defunct and cannot be used
    Defunct,
}

impl StateLifecycle {
    /// Check if state is mounted (can call setState)
    ///
    /// Returns `true` for `Initialized` and `Ready` states.
    /// Returns `false` for `Created` and `Defunct` states.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(StateLifecycle::Created.is_mounted(), false);
    /// assert_eq!(StateLifecycle::Initialized.is_mounted(), true);
    /// assert_eq!(StateLifecycle::Ready.is_mounted(), true);
    /// assert_eq!(StateLifecycle::Defunct.is_mounted(), false);
    /// ```
    pub fn is_mounted(&self) -> bool {
        matches!(self, StateLifecycle::Initialized | StateLifecycle::Ready)
    }

    /// Check if state can build
    ///
    /// Returns `true` only for `Ready` state.
    /// Returns `false` for all other states.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(StateLifecycle::Created.can_build(), false);
    /// assert_eq!(StateLifecycle::Initialized.can_build(), false);
    /// assert_eq!(StateLifecycle::Ready.can_build(), true);
    /// assert_eq!(StateLifecycle::Defunct.can_build(), false);
    /// ```
    pub fn can_build(&self) -> bool {
        matches!(self, StateLifecycle::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_lifecycle_is_mounted() {
        assert_eq!(StateLifecycle::Created.is_mounted(), false);
        assert_eq!(StateLifecycle::Initialized.is_mounted(), true);
        assert_eq!(StateLifecycle::Ready.is_mounted(), true);
        assert_eq!(StateLifecycle::Defunct.is_mounted(), false);
    }

    #[test]
    fn test_state_lifecycle_can_build() {
        assert_eq!(StateLifecycle::Created.can_build(), false);
        assert_eq!(StateLifecycle::Initialized.can_build(), false);
        assert_eq!(StateLifecycle::Ready.can_build(), true);
        assert_eq!(StateLifecycle::Defunct.can_build(), false);
    }

    #[test]
    fn test_state_lifecycle_equality() {
        assert_eq!(StateLifecycle::Created, StateLifecycle::Created);
        assert_ne!(StateLifecycle::Created, StateLifecycle::Ready);
    }

    #[test]
    fn test_state_lifecycle_clone() {
        let lifecycle = StateLifecycle::Ready;
        let cloned = lifecycle;
        assert_eq!(lifecycle, cloned);
    }
}

//! KeepAliveParentDataMixin - Keep-alive support for sliver children.

use std::fmt::Debug;

// ============================================================================
// KEEP ALIVE PARENT DATA MIXIN
// ============================================================================

/// Mixin providing keep-alive functionality for sliver list children.
///
/// Used by sliver multi-box adaptor to keep children alive even when
/// they scroll out of view (e.g., for AutomaticKeepAlive widgets).
///
/// # Fields
///
/// - `keep_alive` - Whether child should be kept alive (set by child)
/// - `kept_alive` - Whether child is currently being kept alive (managed by parent)
///
/// # Usage
///
/// ```ignore
/// #[derive(Debug, Clone)]
/// pub struct MySliverParentData {
///     pub layout_offset: f32,
///     pub keep_alive: KeepAliveParentDataMixin,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeepAliveParentDataMixin {
    /// Whether this child wants to be kept alive.
    ///
    /// Set by the child widget (usually via AutomaticKeepAlive).
    pub keep_alive: bool,

    /// Whether this child is currently being kept alive.
    ///
    /// Managed by the parent sliver. May differ from `keep_alive`
    /// during transition periods.
    pub kept_alive: bool,
}

impl KeepAliveParentDataMixin {
    /// Create new keep-alive mixin (not kept alive).
    pub const fn new() -> Self {
        Self {
            keep_alive: false,
            kept_alive: false,
        }
    }

    /// Create keep-alive mixin with specified state.
    pub const fn with_state(keep_alive: bool, kept_alive: bool) -> Self {
        Self {
            keep_alive,
            kept_alive,
        }
    }

    /// Check if child wants to be kept alive.
    #[inline]
    pub const fn wants_keep_alive(&self) -> bool {
        self.keep_alive
    }

    /// Check if child is currently kept alive.
    #[inline]
    pub const fn is_kept_alive(&self) -> bool {
        self.kept_alive
    }

    /// Request to keep child alive (called by child).
    pub fn request_keep_alive(&mut self) {
        self.keep_alive = true;
    }

    /// Cancel keep-alive request (called by child).
    pub fn cancel_keep_alive(&mut self) {
        self.keep_alive = false;
    }

    /// Mark child as kept alive (called by parent).
    pub fn mark_kept_alive(&mut self) {
        self.kept_alive = true;
    }

    /// Mark child as no longer kept alive (called by parent).
    pub fn mark_not_kept_alive(&mut self) {
        self.kept_alive = false;
    }

    /// Reset to default state (not kept alive).
    pub fn reset(&mut self) {
        self.keep_alive = false;
        self.kept_alive = false;
    }
}

impl Default for KeepAliveParentDataMixin {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let mixin = KeepAliveParentDataMixin::new();

        assert!(!mixin.keep_alive);
        assert!(!mixin.kept_alive);
        assert!(!mixin.wants_keep_alive());
        assert!(!mixin.is_kept_alive());
    }

    #[test]
    fn test_with_state() {
        let mixin = KeepAliveParentDataMixin::with_state(true, true);

        assert!(mixin.keep_alive);
        assert!(mixin.kept_alive);
    }

    #[test]
    fn test_request_keep_alive() {
        let mut mixin = KeepAliveParentDataMixin::new();

        mixin.request_keep_alive();

        assert!(mixin.wants_keep_alive());
        assert!(!mixin.is_kept_alive()); // Parent hasn't marked yet
    }

    #[test]
    fn test_cancel_keep_alive() {
        let mut mixin = KeepAliveParentDataMixin::with_state(true, true);

        mixin.cancel_keep_alive();

        assert!(!mixin.wants_keep_alive());
        assert!(mixin.is_kept_alive()); // Parent still keeping alive
    }

    #[test]
    fn test_mark_kept_alive() {
        let mut mixin = KeepAliveParentDataMixin::new();

        mixin.request_keep_alive();
        mixin.mark_kept_alive();

        assert!(mixin.wants_keep_alive());
        assert!(mixin.is_kept_alive());
    }

    #[test]
    fn test_reset() {
        let mut mixin = KeepAliveParentDataMixin::with_state(true, true);

        mixin.reset();

        assert!(!mixin.keep_alive);
        assert!(!mixin.kept_alive);
    }
}

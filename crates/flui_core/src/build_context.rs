//! Build context for accessing the element tree
//!
//! This module provides BuildContext, which is passed to widget build methods
//! to provide access to the element tree and framework services.

use std::fmt;

/// Build context provides access to the element tree and services
///
/// Similar to Flutter's BuildContext. Passed to build() methods to provide
/// access to the framework.
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessWidget for MyWidget {
///     fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
///         // Use context to access framework services
///         let size = context.size();
///         // ...
///     }
/// }
/// ```
#[derive(Clone)]
pub struct BuildContext {
    // For now, BuildContext is minimal. We'll add more fields as needed.
    // In a full implementation, this would hold references to:
    // - Element tree
    // - Theme data
    // - Media query data
    // - etc.
    _private: (),
}

impl BuildContext {
    /// Create a new build context
    ///
    /// This is an internal API used by the framework.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Mark the current element as needing rebuild
    ///
    /// Similar to Flutter's `setState()` for StatefulWidget.
    pub fn mark_needs_build(&self) {
        // TODO: Implement when we have element tree
    }

    /// Get the size of this element (after layout)
    ///
    /// Returns None if layout hasn't run yet.
    pub fn size(&self) -> Option<crate::constraints::Size> {
        // TODO: Implement when we have render objects
        None
    }
}

impl Default for BuildContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for BuildContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildContext").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_context_creation() {
        let context = BuildContext::new();
        assert!(context.size().is_none());
    }

    #[test]
    fn test_build_context_default() {
        let context = BuildContext::default();
        assert!(context.size().is_none());
    }

    #[test]
    fn test_build_context_debug() {
        let context = BuildContext::new();
        let debug_str = format!("{:?}", context);
        assert!(debug_str.contains("BuildContext"));
    }
}

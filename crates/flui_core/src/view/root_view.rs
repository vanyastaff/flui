//! Root view for FLUI applications
//!
//! This module provides `RootView`, which serves as the root of the view tree,
//! similar to Flutter's `RootWidget`. It bootstraps the element tree and provides
//! the entry point for FLUI applications.

use crate::element::{Element, ElementId, IntoElement};
use crate::pipeline::{PipelineError, PipelineOwner};
use crate::view::{BuildContext, StatelessView};
use flui_foundation::{DiagnosticLevel, Diagnosticable, DiagnosticsNode, Key};
use parking_lot::RwLock;
use std::sync::Arc;

/// Root view for the view tree
///
/// `RootView` serves as the root of the entire view tree and provides the bootstrap
/// mechanism for FLUI applications. It's analogous to Flutter's `RootWidget`.
///
/// # Architecture
///
/// ```text
/// RootView (application entry point)
///   ├─ child: Option<Element> (your app's root element)
///   ├─ attach() method (connects to PipelineOwner)
///   └─ creates root element in element tree
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::view::{RootView, StatelessView};
/// use flui_core::pipeline::PipelineOwner;
///
/// // Create your app view and convert to element
/// let app_element = MyApp::new().build(&ctx).into_element();
///
/// // Create root view with the element
/// let root = RootView::new(app_element)
///     .with_debug_name("MyApp Root");
///
/// // Attach to pipeline owner (done by AppBinding)
/// let element_id = root.attach(&pipeline_owner)?;
/// ```
///
/// # Lifecycle
///
/// 1. Created with child element in `runApp()` or similar
/// 2. Attached to `PipelineOwner` via `attach()`
/// 3. Registers root element and bootstraps element tree
/// 4. Manages the top-level build/layout/paint cycle
#[derive(Debug)]
pub struct RootView {
    /// The child element (typically your app's root element)
    child: Option<Element>,

    /// Optional debug name for diagnostics
    debug_name: Option<String>,

    /// Optional key for the root view
    key: Option<Key>,
}

impl RootView {
    /// Creates a new root view with the given child element
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let app_element = MyApp::new().build(&ctx).into_element();
    /// let root = RootView::new(app_element);
    /// ```
    pub fn new(child: Element) -> Self {
        Self {
            child: Some(child),
            debug_name: None,
            key: None,
        }
    }

    /// Creates an empty root view (no child)
    ///
    /// This is useful for testing or when the child will be set later.
    pub fn empty() -> Self {
        Self {
            child: None,
            debug_name: Some("Empty Root".to_string()),
            key: None,
        }
    }

    /// Creates a root view from any StatelessView
    ///
    /// This is a convenience method that builds the view and creates the root.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let root = RootView::from_view(MyApp::new(), &ctx);
    /// ```
    pub fn from_view<V: StatelessView>(view: V, ctx: &BuildContext) -> Self {
        let element = view.build(ctx).into_element();
        Self::new(element)
    }

    /// Sets a debug name for this root view
    ///
    /// The debug name appears in diagnostics and debug output.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let root = RootView::new(element)
    ///     .with_debug_name("MyApp Root");
    /// ```
    pub fn with_debug_name(mut self, name: impl Into<String>) -> Self {
        self.debug_name = Some(name.into());
        self
    }

    /// Sets a key for this root view
    ///
    /// Keys are rarely needed for root views but can be useful for testing.
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Returns the debug name if set
    pub fn debug_name(&self) -> Option<&str> {
        self.debug_name.as_deref()
    }

    /// Returns the key if set
    pub fn key(&self) -> Option<Key> {
        self.key
    }

    /// Checks if this root view has a child
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Attaches this root view to a pipeline owner and returns the root element ID
    ///
    /// This method:
    /// 1. Takes the child element from this root view
    /// 2. Registers it with the pipeline owner as the root
    /// 3. Returns the element ID for further use
    ///
    /// This is typically called by `AppBinding` during application startup.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    /// let root = RootView::new(element);
    /// let root_element_id = root.attach(&pipeline_owner)?;
    /// ```
    pub fn attach(
        mut self,
        pipeline_owner: &Arc<RwLock<PipelineOwner>>,
    ) -> Result<ElementId, RootViewError> {
        let element = self.child.take().ok_or(RootViewError::NoChild)?;

        // Register with pipeline owner
        let mut pipeline = pipeline_owner.write();
        let element_id =
            pipeline
                .attach_root_element(element)
                .map_err(|e| RootViewError::AttachFailed {
                    reason: format!("Pipeline error: {}", e),
                })?;

        Ok(element_id)
    }

    /// Detaches the root view from the pipeline owner
    ///
    /// This is typically called during application shutdown.
    pub fn detach(
        element_id: ElementId,
        pipeline_owner: &Arc<RwLock<PipelineOwner>>,
    ) -> Result<(), RootViewError> {
        let mut pipeline = pipeline_owner.write();
        pipeline
            .detach_root_element(element_id)
            .map_err(|e| RootViewError::DetachFailed {
                reason: format!("Pipeline error: {}", e),
            })?;
        Ok(())
    }
}

impl StatelessView for RootView {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // We need to return a consistent type from both match arms
        // Since Element implements IntoElement, we can convert both cases to Element
        match self.child {
            Some(element) => {
                // Return the child element directly
                element
            }
            None => {
                // Empty root - create an empty element using unit type
                ().into_element()
            }
        }
    }
}

impl Diagnosticable for RootView {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let mut node = DiagnosticsNode::new("RootView").with_level(DiagnosticLevel::Info);

        if let Some(ref name) = self.debug_name {
            node = node.property("debugName", name);
        }

        if let Some(key) = self.key {
            node = node.property("key", format!("{:?}", key));
        }

        node = node.property("hasChild", self.has_child());

        node
    }
}

/// Errors that can occur with root views
#[derive(Debug, thiserror::Error)]
pub enum RootViewError {
    /// Pipeline owner rejected the root element
    #[error("Failed to attach root element to pipeline: {reason}")]
    AttachFailed {
        /// Detailed reason for the attachment failure
        reason: String,
    },

    /// Pipeline owner couldn't detach the root element
    #[error("Failed to detach root element from pipeline: {reason}")]
    DetachFailed {
        /// Detailed reason for the detachment failure
        reason: String,
    },

    /// Root view has no child element to attach
    #[error("Root view has no child element to attach")]
    NoChild,

    /// Root view was already attached
    #[error("Root view is already attached to a pipeline")]
    AlreadyAttached,

    /// Root view was not attached
    #[error("Root view is not attached to any pipeline")]
    NotAttached,
}

// Conversion from PipelineError to RootViewError for convenience
impl From<PipelineError> for RootViewError {
    fn from(err: PipelineError) -> Self {
        RootViewError::AttachFailed {
            reason: format!("Pipeline error: {}", err),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::EmptyView;

    fn create_test_element() -> Element {
        let ctx = BuildContext::root();
        EmptyView.build(&ctx).into_element()
    }

    #[test]
    fn test_root_view_creation() {
        let element = create_test_element();
        let root = RootView::new(element);
        assert!(root.has_child());
        assert!(root.debug_name().is_none());
        assert!(root.key().is_none());
    }

    #[test]
    fn test_root_view_empty() {
        let root = RootView::empty();
        assert!(!root.has_child());
        assert_eq!(root.debug_name(), Some("Empty Root"));
    }

    #[test]
    fn test_root_view_with_debug_name() {
        let element = create_test_element();
        let root = RootView::new(element).with_debug_name("Test Root");

        assert_eq!(root.debug_name(), Some("Test Root"));
    }

    #[test]
    fn test_root_view_with_key() {
        let element = create_test_element();
        let key = Key::new();
        let root = RootView::new(element).with_key(key);

        assert_eq!(root.key(), Some(key));
    }

    #[test]
    fn test_root_view_from_view() {
        let ctx = BuildContext::root();
        let root = RootView::from_view(EmptyView, &ctx);

        assert!(root.has_child());
    }

    #[test]
    fn test_root_view_diagnostics() {
        let element = create_test_element();
        let root = RootView::new(element).with_debug_name("Test Root");

        let diagnostics = root.to_diagnostics_node();
        assert_eq!(diagnostics.name(), Some("RootView"));
    }

    #[test]
    fn test_root_view_build_with_element() {
        let element = create_test_element();
        let root = RootView::new(element);
        let ctx = BuildContext::root();

        // Should build without panicking
        let _built_element = root.build(&ctx).into_element();

        // Basic validation that we got an element
        // More detailed tests would require a full element tree setup
    }

    #[test]
    fn test_root_view_build_empty() {
        let root = RootView::empty();
        let ctx = BuildContext::root();

        // Should build to empty element without panicking
        let _built_element = root.build(&ctx).into_element();
    }

    #[test]
    fn test_attach_empty_root_fails() {
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        let root = RootView::empty();

        let result = root.attach(&pipeline_owner);
        assert!(matches!(result, Err(RootViewError::NoChild)));
    }
}

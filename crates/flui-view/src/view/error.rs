//! ErrorView - Widget displayed when build fails.
//!
//! When an error occurs during build, the broken widget is replaced
//! by an ErrorView. This provides visual feedback and debugging information.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `ErrorWidget` class.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::ErrorView;
//!
//! // Create from error message
//! let error = ErrorView::new("Failed to build widget");
//!
//! // Create with details
//! let error = ErrorView::with_details(
//!     "Build failed",
//!     Some("Stack trace here...".to_string()),
//! );
//! ```

use std::sync::RwLock;

use flui_foundation::panic::payload_text;

use super::into_view::BoxedView;
use super::stateless::StatelessView;
use super::view::View;

/// Factory function type for creating custom error widgets.
///
/// This allows applications to customize how errors are displayed.
pub type ErrorViewBuilder = fn(&FlutterError) -> Box<dyn View>;

/// Global configurable factory for ErrorView.
///
/// Applications can set this to customize error display.
static ERROR_VIEW_BUILDER: RwLock<Option<ErrorViewBuilder>> = RwLock::new(None);

/// Set the global error view builder.
///
/// When an error occurs during build, this factory is used to create
/// the error widget. If not set, the default ErrorView is used.
pub fn set_error_view_builder(builder: ErrorViewBuilder) {
    if let Ok(mut guard) = ERROR_VIEW_BUILDER.write() {
        *guard = Some(builder);
    }
}

/// Clear the global error view builder.
pub fn clear_error_view_builder() {
    if let Ok(mut guard) = ERROR_VIEW_BUILDER.write() {
        *guard = None;
    }
}

/// Run a unit test that mutates [`ERROR_VIEW_BUILDER`] in a dedicated
/// process, because unrelated lib tests cannot share a process-local guard.
#[cfg(test)]
pub(crate) fn isolate_error_view_builder_test(test_name: &str) -> bool {
    const ISOLATED_TEST: &str = "FLUI_ISOLATED_ERROR_VIEW_BUILDER_TEST";
    if std::env::var(ISOLATED_TEST).as_deref() == Ok(test_name) {
        return false;
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("the libtest executable must exist"),
    )
    .args([test_name, "--exact", "--nocapture"])
    .env(ISOLATED_TEST, test_name)
    .output()
    .expect("the isolated libtest process must start");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            && stdout.contains("running 1 test")
            && stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "isolated test process did not run exactly one passing test: {test_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    true
}

/// Error details for framework errors.
#[derive(Debug, Clone)]
pub struct FlutterError {
    /// The error message.
    pub message: String,
    /// Optional stack trace or additional details.
    pub details: Option<String>,
    /// The exception that caused the error, if any.
    pub exception: Option<String>,
}

impl FlutterError {
    /// Create a new error with just a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
            exception: None,
        }
    }

    /// Create a new error with message and details.
    pub fn with_details(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: Some(details.into()),
            exception: None,
        }
    }

    /// Create from an exception.
    pub fn from_exception(exception: &dyn std::fmt::Debug) -> Self {
        Self {
            message: format!("{exception:?}"),
            details: None,
            exception: Some(format!("{exception:?}")),
        }
    }

    /// Create a `FlutterError` from a panic payload caught by
    /// [`std::panic::catch_unwind`].
    ///
    /// A panic payload is a `Box<dyn Any + Send>`. The common shapes are
    /// `&'static str` (from `panic!("literal")`) and `String` (from
    /// `panic!("{}", formatted)`); anything else (a custom panic value)
    /// cannot be rendered and falls back to a generic message.
    ///
    /// `context` describes *what* was building when the panic happened
    /// (e.g. `"building StatelessElement"`) and is stored as the error
    /// `details` so the rendered [`ErrorView`] carries a breadcrumb.
    ///
    /// Flutter parity: `ComponentElement.performRebuild`
    /// (`framework.dart:5823-5834`) funnels the caught exception through
    /// `_reportException` into `ErrorWidget.builder`.
    pub fn from_panic(payload: &(dyn std::any::Any + Send), context: impl Into<String>) -> Self {
        Self::from_payload_text(payload_text(payload), context)
    }

    /// Build the same display diagnostic from text already extracted from a
    /// panic payload. This lets a recovery record reuse one provenance read
    /// for both its exact payload field and the display-facing error.
    pub(crate) fn from_payload_text(
        source_payload_text: Option<&str>,
        context: impl Into<String>,
    ) -> Self {
        let message = source_payload_text.map_or_else(
            || "panic during build (non-string payload)".to_string(),
            str::to_owned,
        );
        Self {
            message,
            details: Some(context.into()),
            exception: None,
        }
    }
}

impl std::fmt::Display for FlutterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, "\n{details}")?;
        }
        Ok(())
    }
}

impl std::error::Error for FlutterError {}

/// A View that displays an error message.
///
/// This is used when a widget fails to build. It displays the error
/// message in debug mode and a gray background in release mode.
///
/// # Customization
///
/// Use [`set_error_view_builder`] to customize how errors are displayed.
#[derive(Clone)]
pub struct ErrorView {
    /// The error message to display.
    pub message: String,
    /// Optional additional details.
    pub details: Option<String>,
}

impl ErrorView {
    /// Create an ErrorView with just a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    /// Create an ErrorView with message and details.
    pub fn with_details(message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            message: message.into(),
            details,
        }
    }

    /// Create an ErrorView from a FlutterError.
    pub fn from_error(error: &FlutterError) -> Self {
        Self {
            message: error.message.clone(),
            details: error.details.clone(),
        }
    }

    /// Build an error view using the global builder or default.
    pub fn build_error_view(error: &FlutterError) -> Box<dyn View> {
        // Check for custom builder
        if let Ok(guard) = ERROR_VIEW_BUILDER.read()
            && let Some(builder) = *guard
        {
            return builder(error);
        }

        // Default: use ErrorView
        Box::new(Self::from_error(error))
    }
}

impl std::fmt::Debug for ErrorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorView")
            .field("message", &self.message)
            .field("details", &self.details)
            .finish()
    }
}

impl crate::view::RenderView for ErrorView {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderErrorBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        flui_objects::RenderErrorBox::new(self.message.clone(), self.details.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_error(self.message.clone(), self.details.clone())
    }
}

impl View for ErrorView {
    /// A render element over [`RenderErrorBox`](flui_objects::RenderErrorBox).
    ///
    /// The error view owns a render node on purpose: it stands in for a
    /// subtree that failed to build, and every place that subtree could sit
    /// — a lazy sliver child in particular, which must carry a render node to
    /// be laid out at all — needs something with size and paint there.
    /// Flutter's `ErrorWidget` is a `RenderErrorBox` for the same reason.
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// The substitute view for a child whose containment window caught and
/// substituted a lifecycle-hook panic, whatever hook it failed at (mount,
/// activate, update).
///
/// A recovered child must be unkeyed, whatever the registered error-view
/// factory returned: a keyed one would take part in a reconcile's key
/// matching, or trigger a `GlobalKey` retake, instead of staying isolated
/// to the failed slot. `ElementTree::mount_or_substitute` /
/// `ElementTree::update_or_substitute` are this primitive's own callers;
/// `crate::element::sparse_children::build_item_or_error` — a *different*
/// containment window, the lazy-sliver item builder itself — calls this
/// too, so the two never drift into two ways of stripping a key.
pub(crate) fn recovery_view_for(error: &FlutterError) -> BoxedView {
    let recovered = ErrorView::build_error_view(error);
    if recovered.key().is_some() {
        BoxedView(Box::new(UnkeyedRecovery {
            inner: BoxedView(recovered),
        }))
    } else {
        BoxedView(recovered)
    }
}

/// A keyless composite around a custom error view that carried a key. Its
/// render descendant is stamped at adoption like any composite item's.
#[derive(Clone)]
struct UnkeyedRecovery {
    inner: BoxedView,
}

impl StatelessView for UnkeyedRecovery {
    fn build(&self, _ctx: &dyn crate::BuildContext) -> impl crate::view::IntoView {
        self.inner.clone()
    }
}

impl View for UnkeyedRecovery {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_builder_helper_rejects_an_unknown_test_name() {
        let result = std::panic::catch_unwind(|| {
            isolate_error_view_builder_test(
                "view::error::tests::this_is_not_a_registered_libtest_name",
            )
        });
        assert!(
            result.is_err(),
            "a successful zero-test subprocess would make isolation silently vacuous"
        );
    }

    #[test]
    fn test_error_view_creation() {
        let error = ErrorView::new("Test error");
        assert_eq!(error.message, "Test error");
        assert!(error.details.is_none());
    }

    #[test]
    fn test_error_view_with_details() {
        let error = ErrorView::with_details("Test error", Some("Stack trace".to_string()));
        assert_eq!(error.message, "Test error");
        assert_eq!(error.details.as_deref(), Some("Stack trace"));
    }

    #[test]
    fn error_view_is_a_render_element_over_an_error_box() {
        let view = ErrorView::new("boom");
        assert!(
            matches!(
                view.create_element(),
                crate::element::ElementKind::RenderVariable(_)
            ),
            "the error view must own a render node so a failed subtree still has size and paint"
        );
        let ctx = crate::RenderObjectContext::new(None);
        let render_object =
            <ErrorView as crate::view::RenderView>::create_render_object(&view, &ctx);
        assert_eq!(render_object.message(), "boom");
    }
}

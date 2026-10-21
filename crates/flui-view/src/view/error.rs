//! ErrorView - Widget displayed when build fails.
//!
//! When an error occurs during build, the broken widget is replaced
//! by an ErrorView. This provides visual feedback and debugging information.
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

use flui_foundation::panic::payload_text;

use super::into_view::BoxedView;
use super::stateless::StatelessView;
use super::view::View;

/// Factory function type for creating custom error widgets.
///
/// This allows applications to customize how errors are displayed.
pub type ErrorViewBuilder = fn(&FrameworkError) -> Box<dyn View>;

/// Error details for framework errors.
#[derive(Debug, Clone)]
pub struct FrameworkError {
    /// The error message.
    pub message: String,
    /// Optional stack trace or additional details.
    pub details: Option<String>,
    /// The exception that caused the error, if any.
    pub exception: Option<String>,
}

impl FrameworkError {
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

    /// Create a `FrameworkError` from a panic payload caught by
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

impl std::fmt::Display for FrameworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, "\n{details}")?;
        }
        Ok(())
    }
}

impl std::error::Error for FrameworkError {}

/// A View that displays an error message.
///
/// This is used when a widget fails to build. It displays the error
/// message in debug mode and a gray background in release mode.
///
/// # Customization
///
/// Set an [`ErrorViewBuilder`] on the UI runtime (`AppConfig::with_error_view_builder`,
/// or `BuildOwner::set_error_view_builder` for one presentation) to customize how
/// errors are displayed.
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

    /// Create an ErrorView from a FrameworkError.
    pub fn from_error(error: &FrameworkError) -> Self {
        Self {
            message: error.message.clone(),
            details: error.details.clone(),
        }
    }

    /// Build an error view with `custom` when one is configured, else the
    /// default [`ErrorView`].
    pub fn build_error_view(
        error: &FrameworkError,
        custom: Option<ErrorViewBuilder>,
    ) -> Box<dyn View> {
        match custom {
            Some(builder) => builder(error),
            None => Box::new(Self::from_error(error)),
        }
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
/// `crate::element::sparse_children::build_item_or_report` — a *different*
/// containment window, the lazy-sliver item builder itself — calls this
/// too, so the two never drift into two ways of stripping a key.
pub(crate) fn recovery_view_for(
    error: &FrameworkError,
    custom: Option<ErrorViewBuilder>,
) -> BoxedView {
    let recovered = ErrorView::build_error_view(error, custom);
    if recovered.key().is_some() {
        BoxedView::new(UnkeyedRecovery {
            inner: BoxedView::from(recovered),
        })
    } else {
        BoxedView::from(recovered)
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

//! Typed errors for the platform layer's public API.
//!
//! `anyhow::Error` is never returned from this crate's public API: libraries
//! in this workspace surface `thiserror` taxonomies and leave `anyhow` to
//! application crates (the same migration `flui-engine` completed for its
//! `EngineResult`). The window-open family ([`OpenWindowError`],
//! [`ProxySendError`], [`WaitError`]) lives in the `traits::owner` module
//! beside the capability types it serves; this module carries the
//! platform-lifecycle taxonomy shared by every backend.
//!
//! [`OpenWindowError`]: crate::traits::OpenWindowError
//! [`ProxySendError`]: crate::traits::ProxySendError
//! [`WaitError`]: crate::traits::WaitError

/// The boxed error an embedder's bootstrap (`on_ready`) may return.
///
/// Embedder bootstrap code is application-land and returns arbitrary error
/// types (window creation, GPU init, root-widget attach, application
/// wiring), so the boundary carries an opaque `std::error::Error` trait
/// object instead of forcing any one error library into the signature. An
/// application using `anyhow` converts for free: `?` on an
/// `anyhow::Result` inside a callback returning
/// `Result<(), BootstrapError>` goes through anyhow's own
/// `From<anyhow::Error> for Box<dyn Error + Send + Sync>` impl, and plain
/// `&str`/`String` messages convert via the std `From` impls.
pub type BootstrapError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Failure of a platform-lifecycle or platform-service operation.
///
/// The variants map one-to-one onto where they surface:
///
/// * [`Init`](Self::Init) — [`crate::current_platform`] and backend
///   constructors (`WindowsPlatform::new`, `MacOSPlatform::with_config`, …).
/// * [`EventLoop`](Self::EventLoop) — `Platform::run` when the loop itself
///   fails to start or exits abnormally.
/// * [`Bootstrap`](Self::Bootstrap) — `Platform::run` when the embedder's
///   `on_ready` callback returned `Err`; the embedder's own error rides
///   along as the `source`.
/// * [`AppPath`](Self::AppPath) — `Platform::app_path`.
/// * [`Dialog`](Self::Dialog) — `Platform::prompt_for_paths` /
///   `Platform::prompt_for_new_path`.
///
/// `#[non_exhaustive]`: platform backends grow capabilities (and therefore
/// failure shapes) over time; new variants are additions, not breaks.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformError {
    /// Platform initialization failed: no backend is available for this
    /// target/configuration, or the backend's own OS-level setup (COM,
    /// NSApplication, …) failed.
    #[error("platform initialization failed: {message}")]
    Init {
        /// What failed, in the backend's own words.
        message: String,
    },

    /// The platform event loop failed to start, or terminated abnormally.
    #[error("platform event loop failed: {message}")]
    EventLoop {
        /// What failed, in the backend's own words.
        message: String,
    },

    /// The embedder's bootstrap (`on_ready`) returned `Err`. Every backend
    /// stops entering (or promptly exits) its loop on this failure; the
    /// embedder's own error is preserved as the [`source`] of this variant
    /// rather than flattened into a message.
    ///
    /// [`source`]: std::error::Error::source
    #[error(
        "embedder bootstrap (on_ready) failed{}",
        .loop_error
            .as_deref()
            .map(|error| format!(" (the event loop also failed while unwinding: {error})"))
            .unwrap_or_default()
    )]
    Bootstrap {
        /// The embedder's own bootstrap error, reachable via
        /// [`std::error::Error::source`].
        #[source]
        source: BootstrapError,
        /// A loop-level failure that surfaced while the loop unwound after
        /// the bootstrap failure. Carried here (not returned instead) so
        /// the bootstrap error stays the root cause and the loop error is
        /// still not lost — the exact both-failed scenario the fallible
        /// `on_ready` design exists to get right.
        loop_error: Option<String>,
    },

    /// The application's executable/bundle path could not be determined.
    #[error("could not determine the application path: {message}")]
    AppPath {
        /// What failed, in the backend's own words.
        message: String,
    },

    /// A native file dialog could not be run to completion. A user
    /// cancelling is NOT an error — the dialog methods return `Ok(None)`
    /// for that.
    #[error("file dialog failed: {message}")]
    Dialog {
        /// What failed, in the backend's own words.
        message: String,
    },
}

impl PlatformError {
    /// Wraps an embedder bootstrap failure with no accompanying loop-level
    /// error — the common case on every backend except a loop that also
    /// fails while unwinding (see [`PlatformError::Bootstrap`]).
    #[must_use]
    pub fn bootstrap(source: impl Into<BootstrapError>) -> Self {
        Self::Bootstrap {
            source: source.into(),
            loop_error: None,
        }
    }
}

/// Result type alias for the platform crate's lifecycle/service surface.
pub type PlatformResult<T> = Result<T, PlatformError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_display_without_loop_error_has_no_suffix() {
        let error = PlatformError::bootstrap("root widget attach failed");
        assert_eq!(error.to_string(), "embedder bootstrap (on_ready) failed");
    }

    #[test]
    fn bootstrap_display_with_loop_error_names_both() {
        let error = PlatformError::Bootstrap {
            source: "gpu init failed".into(),
            loop_error: Some("loop failed".to_string()),
        };
        assert_eq!(
            error.to_string(),
            "embedder bootstrap (on_ready) failed \
             (the event loop also failed while unwinding: loop failed)"
        );
    }

    #[test]
    fn bootstrap_source_preserves_the_embedder_error() {
        let error = PlatformError::bootstrap("root widget attach failed");
        let source = std::error::Error::source(&error).expect("Bootstrap carries a source");
        assert_eq!(source.to_string(), "root widget attach failed");
    }
}

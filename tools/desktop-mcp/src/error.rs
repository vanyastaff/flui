//! The one error type every tool reports.
//!
//! Each variant's message is what the calling agent reads, so it names the
//! argument, element or window involved and, where there is one, the next
//! step that would succeed.

use thiserror::Error;

/// Why a tool call did not do what it was asked.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The arguments are malformed or contradict each other.
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),

    /// The feature has no backend on this OS.
    #[error("{0}")]
    NotSupported(String),

    /// No window, monitor or process matches the target.
    #[error("{0}")]
    NotFound(String),

    /// The element handle was never issued in this session.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "raised by the UIA backend, the only accessibility backend built yet"
        )
    )]
    #[error(
        "unknown element `{0}`: element ids come from accessibility_tree, find or wait_for in this session"
    )]
    UnknownElement(String),

    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "raised by the UIA backend, the only accessibility backend built yet"
        )
    )]
    /// The element existed once but the application has since removed it.
    #[error(
        "element `{0}` is no longer available (the UI changed); read the tree again for a fresh id"
    )]
    StaleElement(String),

    /// The element does not implement the control pattern the action needs.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "raised by the UIA backend, the only accessibility backend built yet"
        )
    )]
    #[error(
        "element `{element}` does not support the {pattern} pattern; it supports: [{supported}]"
    )]
    PatternUnsupported {
        /// The element handle.
        element: String,
        /// The missing pattern.
        pattern: &'static str,
        /// The patterns it does support, comma-separated.
        supported: String,
    },

    /// Input was refused because the target window is not in front.
    #[error(
        "refused: the target {target} is not the foreground window (foreground: {foreground}); call activate_window first"
    )]
    NotForeground {
        /// The window the caller wanted the input to reach.
        target: String,
        /// The window that would actually have received it.
        foreground: String,
    },

    /// A coordinate input would not land on the target.
    #[error("refused: point ({x}, {y}) is not on the target: {reason}; nothing was sent there")]
    OutsideTarget {
        /// Point x in physical screen pixels.
        x: i32,
        /// Point y in physical screen pixels.
        y: i32,
        /// What is there instead.
        reason: String,
    },

    /// A wait ran out before the condition held.
    #[error("timed out after {timeout_ms} ms waiting for {what}; last tree seen:\n{summary}")]
    Timeout {
        /// The wait's budget.
        timeout_ms: u64,
        /// The awaited condition, formatted.
        what: String,
        /// A short summary of the last tree the wait read.
        summary: String,
    },

    /// An action failed partway, after part of it reached the OS or the
    /// application: `what` says how much, so a retry does not repeat it.
    #[error("{cause}; {what}")]
    Interrupted {
        /// Why it stopped.
        cause: Box<ToolError>,
        /// What had already happened.
        what: String,
    },

    /// An OS API call failed.
    #[error("{context}: {message}")]
    Platform {
        /// What the server was doing.
        context: String,
        /// The OS or library message.
        message: String,
    },
}

impl ToolError {
    /// An OS or library failure while doing `context`.
    pub fn platform(context: impl Into<String>, err: impl std::fmt::Display) -> Self {
        Self::Platform {
            context: context.into(),
            message: err.to_string(),
        }
    }
}

/// The result every backend operation returns.
pub type ToolResult<T> = Result<T, ToolError>;

//! Internal macros for FLUI core.

/// Macro for logging in hot paths (only enabled with feature flag).
///
/// Use this for very frequent operations that would otherwise spam the logs.
///
/// # Examples
///
/// ```no_run
/// use flui_core::trace_hot_path;
/// # use flui_core::foundation::ElementId;
///
/// fn paint_child(id: ElementId) {
///     trace_hot_path!("paint_child called", id = ?id);
///     // ... painting logic ...
/// }
/// ```
#[macro_export]
macro_rules! trace_hot_path {
    ($($arg:tt)*) => {
        #[cfg(feature = "trace-hot-paths")]
        {
            ::tracing::trace!($($arg)*);
        }
    };
}

//! ViewState - State for stateful views
//!
//! Defines the trait that all view states must implement.

/// ViewState - Marker trait for view state types
///
/// State types used with `StatefulView` must implement this trait.
///
/// # Requirements
///
/// - `Send`: State can be transferred between threads
/// - `Sync`: State can be shared between threads
/// - `'static`: State has no non-static references
///
/// # Example
///
/// ```rust,ignore
/// struct CounterState {
///     count: i32,
/// }
///
/// impl ViewState for CounterState {}
/// ```
pub trait ViewState: Send + Sync + 'static {}

// Blanket implementation for all qualifying types
impl<T: Send + Sync + 'static> ViewState for T {}

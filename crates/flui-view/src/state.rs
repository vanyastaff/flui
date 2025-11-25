//! View state trait.
//!
//! Defines the requirements for state types used with `StatefulView`.

// ============================================================================
// VIEW STATE TRAIT
// ============================================================================

/// Trait for view state types.
///
/// State types must be `Send` for thread-safe UI updates.
/// The `'static` bound ensures state can be stored in the element tree.
///
/// # Example
///
/// ```rust
/// use flui_view::ViewState;
///
/// #[derive(Default)]
/// struct CounterState {
///     count: i32,
/// }
///
/// // Automatically implements ViewState because it's Send + 'static
/// fn assert_view_state<T: ViewState>() {}
/// assert_view_state::<CounterState>();
/// ```
pub trait ViewState: Send + 'static {}

/// Blanket implementation for all `Send + 'static` types.
impl<T: Send + 'static> ViewState for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestState {
        value: i32,
    }

    fn assert_view_state<T: ViewState>() {}

    #[test]
    fn test_primitive_types_are_view_state() {
        assert_view_state::<i32>();
        assert_view_state::<String>();
        assert_view_state::<bool>();
        assert_view_state::<()>();
    }

    #[test]
    fn test_custom_struct_is_view_state() {
        assert_view_state::<TestState>();
    }

    #[test]
    fn test_vec_is_view_state() {
        assert_view_state::<Vec<i32>>();
        assert_view_state::<Vec<String>>();
    }
}

//! Test macros
//!
//! Provides macros for simplifying common test patterns.

/// Quick test macro - creates a test that mounts a view and runs assertions
///
/// # Examples
///
/// ```rust,ignore
/// quick_test!(MyView::new(), |harness, root_id| {
///     assert!(harness.is_mounted());
///     assert_eq!(harness.element_count(), 1);
/// });
/// ```
#[macro_export]
macro_rules! quick_test {
    ($view:expr, $test:expr) => {{
        let mut harness = $crate::testing::TestHarness::new();
        let root_id = harness.mount($view);
        $test(harness, root_id)
    }};
}

/// Quick test with pump - creates a test that mounts and pumps before assertions
///
/// # Examples
///
/// ```rust,ignore
/// quick_test_pump!(MyView::new(), |harness, root_id| {
///     // Pipeline has been pumped
///     assert!(harness.is_mounted());
/// });
/// ```
#[macro_export]
macro_rules! quick_test_pump {
    ($view:expr, $test:expr) => {{
        let mut harness = $crate::testing::TestHarness::new();
        let root_id = harness.mount($view);
        harness.pump();
        $test(harness, root_id)
    }};
}

/// Assert tree structure macro
///
/// # Examples
///
/// ```rust,ignore
/// assert_tree!(tree, {
///     components: 2,
///     renders: 3,
///     providers: 1
/// });
/// ```
#[macro_export]
macro_rules! assert_tree {
    ($tree:expr, {
        components: $c:expr,
        renders: $r:expr,
        providers: $p:expr
    }) => {{
        $crate::testing::snapshot::assert_tree_snapshot(&$tree, $c, $r, $p)
    }};
}

/// Create a test view with a given name (for debugging)
///
/// # Examples
///
/// ```rust,ignore
/// let view = test_view!("my-test-view");
/// ```
#[macro_export]
macro_rules! test_view {
    ($name:expr) => {{
        $crate::testing::TestView::new($name)
    }};
}

/// Assert element exists macro
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_exists!(tree, element_id);
/// ```
#[macro_export]
macro_rules! assert_element_exists {
    ($tree:expr, $id:expr) => {{
        $crate::testing::assertions::assert_element_exists(&$tree, $id)
    }};
}

/// Assert element is a component
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_component!(tree, element_id);
/// ```
#[macro_export]
macro_rules! assert_is_component {
    ($tree:expr, $id:expr) => {{
        $crate::testing::assertions::assert_is_component(&$tree, $id)
    }};
}

/// Assert element is a render
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_render!(tree, element_id);
/// ```
#[macro_export]
macro_rules! assert_is_render {
    ($tree:expr, $id:expr) => {{
        $crate::testing::assertions::assert_is_render(&$tree, $id)
    }};
}

/// Assert element count
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_count!(tree, 5);
/// ```
#[macro_export]
macro_rules! assert_element_count {
    ($tree:expr, $count:expr) => {{
        $crate::testing::assertions::assert_element_count(&$tree, $count)
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macros_compile() {
        // Just ensure macros compile - actual testing requires full integration
    }
}

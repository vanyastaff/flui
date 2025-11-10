# FLUI Testing Utilities

Comprehensive testing utilities for FLUI applications and components.

## Quick Start

```rust
use flui_core::testing::*;

#[test]
fn my_test() {
    // Quick test - mount and test in one go
    let (harness, root_id) = quick_test_pump(MyView::new());

    // Make assertions
    assert!(harness.is_mounted());
    assert_element_count!(harness.tree().read(), 1);
}
```

## Core Components

### 1. TestHarness

Isolated environment for testing view trees.

```rust
use flui_core::testing::TestHarness;

#[test]
fn test_with_harness() {
    let mut harness = TestHarness::new();

    // Mount a view
    let root_id = harness.mount(MyView::new());

    // Process the pipeline
    harness.pump();

    // Check state
    assert!(harness.is_mounted());
}
```

### 2. MockRender & SpyRender

Mock render objects for unit testing.

```rust
use flui_core::testing::MockRender;
use flui_types::Size;

#[test]
fn test_render_mock() {
    let mock = MockRender::leaf(Size::new(100.0, 50.0));

    // Verify initial state
    assert_eq!(mock.layout_call_count(), 0);

    // Use in tests...
    // mock.layout(&ctx);

    // Verify calls
    // assert_eq!(mock.layout_call_count(), 1);
}
```

### 3. ViewTester

Fluent API for testing views.

```rust
use flui_core::testing::ViewTester;

#[test]
fn test_view() {
    let result = ViewTester::new()
        .with_view(MyView::new());

    assert!(result.is_mounted());
}
```

## Test Fixtures

Pre-configured fixtures to reduce boilerplate.

### Standard Constraints

```rust
use flui_core::testing::*;

// Use standard 800x600 constraints
let constraints = TEST_CONSTRAINTS;

// Or create tight constraints
let constraints = tight_constraints(); // 800x600, tight

// Or loose constraints
let constraints = loose_constraints(); // unbounded

// Or custom size
let constraints = fixed_size_constraints(640.0, 480.0);
```

### Common Sizes

```rust
use flui_core::testing::sizes::*;

let small = SMALL;    // 100x100
let medium = MEDIUM;  // 300x200
let large = LARGE;    // 800x600
let square = SQUARE;  // 200x200
```

### Test Harness Builder

```rust
use flui_core::testing::TestHarnessBuilder;

// Desktop size
let harness = TestHarnessBuilder::new()
    .desktop()  // 800x600
    .build();

// Mobile portrait
let harness = TestHarnessBuilder::new()
    .mobile_portrait()  // 375x667
    .build();

// Custom size
let harness = TestHarnessBuilder::new()
    .with_size(1024.0, 768.0)
    .build();
```

### Quick Test Helpers

```rust
use flui_core::testing::*;

// Mount and return harness
let (harness, root_id) = quick_test(MyView::new());

// Mount, pump, and return harness
let (harness, root_id) = quick_test_pump(MyView::new());

// Create test build context
let ctx = test_build_context();
```

## Assertions

Comprehensive assertion helpers with clear error messages.

```rust
use flui_core::testing::*;

// Element existence
assert_element_exists!(&tree, element_id);
assert_element_not_exists(&tree, element_id);

// Element type
assert_is_component!(&tree, element_id);
assert_is_render!(&tree, element_id);
assert_is_provider(&tree, element_id);

// Element state
assert_is_dirty(&tree, element_id);
assert_is_clean(&tree, element_id);

// Element properties
assert_element_size(&tree, element_id, Size::new(100.0, 50.0));

// Tree structure
assert_element_count!(&tree, 5);
```

## Snapshot Testing

Capture and compare element tree snapshots.

```rust
use flui_core::testing::*;

// Capture a snapshot
let snapshot = ElementTreeSnapshot::capture(&tree);

// Check if it matches expected counts
assert!(snapshot.matches(2, 3, 1)); // 2 components, 3 renders, 1 provider

// Or use the helper
assert_tree_snapshot(&tree, 2, 3, 1);

// Use macro for clean syntax
assert_tree!(tree, {
    components: 2,
    renders: 3,
    providers: 1
});

// Compare snapshots
let before = ElementTreeSnapshot::capture(&tree);
// ... make changes ...
let after = ElementTreeSnapshot::capture(&tree);
let diff = before.diff(&after);

if diff.has_changes() {
    println!("{}", diff);
}
```

## Tree Inspection

Debug and inspect element tree state.

```rust
use flui_core::testing::*;

// Quick print
print_tree(&tree);

// Get summary
let summary = tree_summary(&tree);
println!("Total elements: {}", summary.total_count);
println!("Dirty elements: {}", summary.dirty_count);

// Advanced inspection
let inspector = TreeInspector::new(&tree);

// Find elements by type
let components = inspector.find_components();
let renders = inspector.find_renders();
let providers = inspector.find_providers();

// Find dirty elements
let dirty = inspector.find_dirty();

// Print detailed tree
inspector.print_tree();
```

## Test Macros

Convenient macros for common patterns.

```rust
use flui_core::testing::*;

// Quick test
quick_test!(MyView::new(), |harness, root_id| {
    assert!(harness.is_mounted());
});

// Quick test with pump
quick_test_pump!(MyView::new(), |harness, root_id| {
    // Pipeline has been pumped
    assert_eq!(harness.element_count(), 3);
});

// Create test view
let view = test_view!("my-test-view");

// Assert macros
assert_element_exists!(tree, id);
assert_is_component!(tree, id);
assert_element_count!(tree, 5);
```

## Complete Example

```rust
use flui_core::testing::*;

#[derive(Clone)]
struct Counter {
    count: i32,
}

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Your implementation
        Option::<Counter>::None
    }
}

#[test]
fn test_counter_full() {
    // Use fixtures for setup
    let (mut harness, root_id) = quick_test(Counter { count: 0 });

    // Pump the pipeline
    harness.pump();

    // Inspect the tree
    let tree = harness.tree().read();
    print_tree(&tree);

    // Assert tree structure
    assert_tree!(tree, {
        components: 1,
        renders: 0,
        providers: 0
    });

    // Take snapshot
    let snapshot = ElementTreeSnapshot::capture(&tree);

    // Make changes...
    // harness.mount(Counter { count: 1 });
    // harness.pump();

    // Compare snapshots
    // let new_snapshot = ElementTreeSnapshot::capture(&tree);
    // let diff = snapshot.diff(&new_snapshot);
    // println!("{}", diff);
}
```

## Best Practices

1. **Use fixtures** - Avoid duplicating test setup code
2. **Use snapshots** - Track tree structure changes over time
3. **Use inspection** - Debug failing tests easily
4. **Use macros** - Write less boilerplate
5. **Use quick helpers** - For simple one-off tests
6. **Use TestHarness** - For complex integration tests
7. **Use MockRender** - For unit testing render objects

## Tips

- Start with `quick_test_pump()` for simple tests
- Use `print_tree()` when debugging
- Use snapshots to catch unintended tree changes
- Use inspection to find specific elements
- Use macros for cleaner test code

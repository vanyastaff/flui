# Error Handling Integration Guide

This document describes how to integrate automatic panic catching into the FLUI framework.

## Architecture

```text
User Widget Build
       ↓
catch_unwind() ← Framework wraps all build calls
       ↓
   Panic?
    ├─ No → Normal build flow
    └─ Yes → Find ErrorBoundary → Set error → Rebuild with ErrorWidget
```

## Integration Points

### 1. PipelineOwner Build Phase

The PipelineOwner should wrap element build calls in `std::panic::catch_unwind`:

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};
use flui_app::error_handling::{handle_build_panic, ErrorInfo};

// In PipelineOwner::build_element or similar:
fn build_element_safe(&mut self, element_id: ElementId) -> Result<(), ErrorInfo> {
    // Wrap the build call in catch_unwind
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Normal build logic here
        self.build_element_internal(element_id)
    }));

    match result {
        Ok(_) => Ok(()),
        Err(panic_info) => {
            // Convert panic to ErrorInfo
            let error = handle_build_panic(&*panic_info);

            // Find nearest ErrorBoundary and set error
            // (requires tree walking implementation)
            if let Some(boundary_id) = self.find_error_boundary(element_id) {
                self.set_boundary_error(boundary_id, error.clone());
            }

            Err(error)
        }
    }
}
```

### 2. Finding ErrorBoundary

To find the nearest ErrorBoundary, walk up the tree:

```rust
fn find_error_boundary(&self, element_id: ElementId) -> Option<ElementId> {
    let tree = self.tree.read();
    let mut current = element_id;

    loop {
        // Check if current element is an ErrorBoundary
        if let Some(element) = tree.get(current) {
            if element.is_stateful() {
                // Check if view_object is ErrorBoundary
                if let Some(view_obj) = element.view_object() {
                    // Downcast to check type
                    if view_obj.as_any().is::<StatefulViewWrapper<ErrorBoundary>>() {
                        return Some(current);
                    }
                }
            }

            // Move to parent
            if let Some(parent) = element.parent() {
                current = parent;
            } else {
                // Reached root, no ErrorBoundary found
                return None;
            }
        } else {
            return None;
        }
    }
}
```

### 3. Setting Error in ErrorBoundary

Once ErrorBoundary is found, set the error in its state:

```rust
fn set_boundary_error(&mut self, boundary_id: ElementId, error: ErrorInfo) {
    let mut tree = self.tree.write();

    if let Some(element) = tree.get_mut(boundary_id) {
        if let Some(view_obj) = element.view_object_mut() {
            // Access the StatefulViewWrapper
            if let Some(wrapper) = view_obj.as_any_mut()
                .downcast_mut::<StatefulViewWrapper<ErrorBoundary>>() {
                // Get state and set error
                if let Some(state) = wrapper.state_mut() {
                    state.set_error(error);

                    // Mark element dirty for rebuild
                    self.dirty_set.write().mark(boundary_id);
                }
            }
        }
    }
}
```

## Usage Example

Once integrated, errors will be automatically caught:

```rust
#[derive(Debug, Clone)]
struct BuggyWidget;

impl StatelessView for BuggyWidget {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        panic!("This widget has a bug!");
    }
}

fn main() {
    // ErrorBoundary will catch the panic automatically
    run_app(WidgetsApp::new(BuggyWidget));

    // Instead of crashing, the app will display ErrorWidget
}
```

## Testing

Test that panics are caught:

```rust
#[test]
fn test_error_boundary_catches_panic() {
    let binding = AppBinding::ensure_initialized();

    // Create widget that panics
    let buggy = BuggyWidget;
    let boundary = ErrorBoundary::new(
        buggy.build(&test_ctx()).into_element()
    );

    // Build should not panic
    let result = std::panic::catch_unwind(|| {
        boundary.build(&state, &test_ctx());
    });

    assert!(result.is_ok());
    assert!(state.has_error());
}
```

## Future Improvements

1. **Stack Trace Capture**: Capture and display stack traces in debug mode
2. **Error Recovery**: Allow ErrorBoundary to reset and retry
3. **Error Telemetry**: Send errors to monitoring services
4. **Custom Error Widgets**: Allow apps to customize error display

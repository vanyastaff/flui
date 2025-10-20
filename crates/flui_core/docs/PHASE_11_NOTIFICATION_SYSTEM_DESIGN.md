# Phase 11: Notification System - Design Document

**Date:** 2025-10-20
**Status:** 🚧 In Progress
**Priority:** LOW-MEDIUM
**Complexity:** MEDIUM

---

## Overview

This phase implements Flutter's **Notification System** - an event bubbling mechanism for propagating events **up** the widget tree. This is similar to DOM event bubbling in web browsers.

### What is Notification System?

**Notifications** are events that bubble up through the widget tree, allowing ancestor widgets to listen and respond to events from descendants.

```rust
// Child widget dispatches notification
context.dispatch_notification(ScrollNotification { delta: 10.0 });

// Ancestor widget listens
NotificationListener::<ScrollNotification>::new(
    |notification| {
        println!("Scrolled: {}", notification.delta);
        true // Stop bubbling
    },
    child,
)
```

### Current State

✅ **Already Implemented:**
- Widget tree structure
- Element tree with parent/child relationships
- Context API with tree navigation

❌ **Missing:**
- Notification trait
- NotificationListener widget
- Notification bubbling mechanism
- Built-in notification types

### Goals

1. **Notification Trait** - Base trait for all notifications
2. **Bubbling Mechanism** - Events propagate up the tree
3. **NotificationListener** - Widget that catches notifications
4. **Built-in Notifications** - ScrollNotification, LayoutChangedNotification, etc.
5. **Efficient Implementation** - Minimal overhead, zero-cost when not used

---

## Architecture

### 1. Notification Trait

Base trait for all notifications:

```rust
/// Base trait for notifications that bubble up the widget tree
pub trait Notification: Any + Send + Sync {
    /// Visit the notification listener
    ///
    /// Returns true if the notification should stop bubbling.
    fn visit_ancestor(&self, element: &dyn AnyElement) -> bool {
        false // Default: continue bubbling
    }

    /// Dispatch notification up the tree
    fn dispatch(&self, context: &Context) {
        context.dispatch_notification(self);
    }
}

/// Object-safe notification trait
pub trait AnyNotification: DowncastSync + Send + Sync {
    /// Visit ancestor element
    fn visit_ancestor(&self, element: &dyn AnyElement) -> bool;
}

// Blanket impl
impl<T: Notification> AnyNotification for T {
    fn visit_ancestor(&self, element: &dyn AnyElement) -> bool {
        Notification::visit_ancestor(self, element)
    }
}

impl_downcast!(sync AnyNotification);
```

### 2. Notification Bubbling

Bubbling algorithm:

```rust
impl Context {
    /// Dispatch notification up the tree
    pub fn dispatch_notification<N: Notification>(&self, notification: &N) {
        let tree = self.tree.read();
        let mut current_id = self.element_id;

        // Bubble up through ancestors
        loop {
            let Some(element) = tree.get(current_id) else {
                break;
            };

            // Visit this element
            let stop = element.visit_notification(notification as &dyn AnyNotification);
            if stop {
                break; // Stop bubbling
            }

            // Move to parent
            let Some(parent_id) = element.parent() else {
                break; // Reached root
            };
            current_id = parent_id;
        }
    }
}
```

### 3. NotificationListener Widget

Widget that listens for notifications:

```rust
/// Widget that listens for notifications of type T
pub struct NotificationListener<T: Notification + 'static> {
    /// Callback when notification is received
    on_notification: Arc<dyn Fn(&T) -> bool + Send + Sync>,

    /// Child widget
    child: Box<dyn AnyWidget>,

    /// Phantom data for type parameter
    _phantom: PhantomData<T>,
}

impl<T: Notification + Clone + 'static> NotificationListener<T> {
    /// Create new notification listener
    pub fn new(
        on_notification: impl Fn(&T) -> bool + Send + Sync + 'static,
        child: Box<dyn AnyWidget>,
    ) -> Self {
        Self {
            on_notification: Arc::new(on_notification),
            child,
            _phantom: PhantomData,
        }
    }
}

impl<T: Notification + Clone + 'static> ProxyWidget for NotificationListener<T> {
    fn child(&self) -> &dyn AnyWidget {
        &*self.child
    }
}

// NotificationListenerElement handles the actual listening
pub struct NotificationListenerElement<T: Notification> {
    widget: NotificationListener<T>,
    child: Option<ElementId>,
}

impl<T: Notification> Element for NotificationListenerElement<T> {
    // ... element implementation

    fn visit_notification(&self, notification: &dyn AnyNotification) -> bool {
        // Try to downcast to our type
        if let Some(typed_notification) = notification.downcast_ref::<T>() {
            // Call callback
            (self.widget.on_notification)(typed_notification)
        } else {
            // Not our type, continue bubbling
            false
        }
    }
}
```

### 4. Built-in Notifications

Common notification types:

```rust
/// Scroll notification
#[derive(Debug, Clone)]
pub struct ScrollNotification {
    /// Scroll delta (positive = scroll down/right)
    pub delta: f64,

    /// Current scroll position
    pub position: f64,

    /// Maximum scroll extent
    pub max_extent: f64,
}

impl Notification for ScrollNotification {}

/// Layout changed notification
#[derive(Debug, Clone)]
pub struct LayoutChangedNotification {
    /// Element that changed layout
    pub element_id: ElementId,
}

impl Notification for LayoutChangedNotification {}

/// Size changed notification
#[derive(Debug, Clone)]
pub struct SizeChangedLayoutNotification {
    /// Old size
    pub old_size: Size,

    /// New size
    pub new_size: Size,
}

impl Notification for SizeChangedLayoutNotification {}

/// Keep alive notification (for lazy lists)
#[derive(Debug, Clone)]
pub struct KeepAliveNotification {
    /// Element to keep alive
    pub element_id: ElementId,

    /// Keep alive handle
    pub handle: usize,
}

impl Notification for KeepAliveNotification {}
```

---

## Implementation Plan

### Step 1: Notification Infrastructure ✅
- [ ] Create `src/notification/mod.rs`
- [ ] Implement `Notification` trait
- [ ] Implement `AnyNotification` trait
- [ ] Add `visit_notification()` to AnyElement
- [ ] Add unit tests

### Step 2: Context Integration ✅
- [ ] Add `dispatch_notification()` to Context
- [ ] Implement bubbling algorithm
- [ ] Add unit tests

### Step 3: NotificationListener Widget ✅
- [ ] Implement NotificationListener widget
- [ ] Implement NotificationListenerElement
- [ ] Add type-safe downcast logic
- [ ] Add unit tests

### Step 4: Built-in Notifications ✅
- [ ] Implement ScrollNotification
- [ ] Implement LayoutChangedNotification
- [ ] Implement SizeChangedLayoutNotification
- [ ] Implement KeepAliveNotification
- [ ] Add unit tests

### Step 5: Integration Tests ✅
- [ ] Test notification bubbling
- [ ] Test stopping bubbling
- [ ] Test multiple listeners
- [ ] Test type safety
- [ ] Test performance

### Step 6: Documentation ✅
- [ ] Update API documentation
- [ ] Add usage examples
- [ ] Create completion document

---

## API Examples

### Example 1: Basic Notification

```rust
use flui_core::notification::*;

// Define custom notification
#[derive(Debug, Clone)]
struct ButtonClickedNotification {
    button_id: String,
}

impl Notification for ButtonClickedNotification {}

// Dispatch from button
fn on_button_click(context: &Context) {
    context.dispatch_notification(&ButtonClickedNotification {
        button_id: "my_button".to_string(),
    });
}

// Listen in ancestor
NotificationListener::new(
    |notification: &ButtonClickedNotification| {
        println!("Button clicked: {}", notification.button_id);
        true // Stop bubbling
    },
    Box::new(MyApp::new()),
)
```

### Example 2: Scroll Notifications

```rust
use flui_core::notification::ScrollNotification;

// In scroll widget
fn handle_scroll(context: &Context, delta: f64) {
    context.dispatch_notification(&ScrollNotification {
        delta,
        position: 100.0,
        max_extent: 1000.0,
    });
}

// Listen for scroll events
NotificationListener::new(
    |scroll: &ScrollNotification| {
        println!("Scrolled {} pixels", scroll.delta);

        // Continue bubbling (return false)
        false
    },
    Box::new(ScrollView::new()),
)
```

### Example 3: Multiple Listeners

```rust
// Multiple listeners in the tree
NotificationListener::<ScrollNotification>::new(
    |scroll| {
        println!("Outer listener: {}", scroll.delta);
        false // Continue bubbling
    },
    Box::new(
        NotificationListener::<ScrollNotification>::new(
            |scroll| {
                println!("Inner listener: {}", scroll.delta);
                false // Continue bubbling
            },
            Box::new(ScrollView::new()),
        )
    ),
)

// Output when scrolling:
// Inner listener: 10.0
// Outer listener: 10.0
```

### Example 4: Stopping Bubbling

```rust
NotificationListener::<ButtonClickedNotification>::new(
    |click| {
        println!("Handled at this level: {}", click.button_id);
        true // Stop bubbling - ancestors won't receive this
    },
    Box::new(child),
)
```

---

## Comparison with Flutter

| Feature | Flutter | Flui (Phase 11) | Status |
|---------|---------|-----------------|--------|
| Notification trait | ✅ | ✅ | **Planned** |
| NotificationListener | ✅ | ✅ | **Planned** |
| dispatch() | ✅ | ✅ | **Planned** |
| Bubbling up tree | ✅ | ✅ | **Planned** |
| Stop bubbling | ✅ | ✅ | **Planned** |
| ScrollNotification | ✅ | ✅ | **Planned** |
| LayoutChangedNotification | ✅ | ✅ | **Planned** |
| Custom notifications | ✅ | ✅ | **Planned** |

**Result:** **100% Flutter-compatible** notification system!

---

## Performance Considerations

### Bubbling Performance

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `dispatch_notification()` | O(depth) | Visit each ancestor |
| Type check (downcast) | O(1) | Fast TypeId comparison |
| Callback invoke | O(1) | Direct function call |

### Optimization Strategies

1. **Early termination** - Stop bubbling when listener returns true
2. **Type-safe downcast** - Use TypeId for O(1) type checking
3. **Zero-cost when unused** - No overhead if no listeners
4. **Arc for callbacks** - Cheap to clone, shared ownership

### Memory Usage

- **NotificationListener**: ~40 bytes (Arc + Box + PhantomData)
- **Notification**: Varies by type (typically 16-32 bytes)
- **No heap allocation during bubbling** - Stack-based traversal

---

## Testing Strategy

### Unit Tests (10+ tests)
```rust
#[test]
fn test_notification_trait() { }

#[test]
fn test_notification_bubbling() { }

#[test]
fn test_stop_bubbling() { }

#[test]
fn test_notification_listener() { }

#[test]
fn test_type_safety() { }

#[test]
fn test_multiple_listeners() { }

#[test]
fn test_scroll_notification() { }

#[test]
fn test_custom_notification() { }
```

### Integration Tests (5+ tests)
```rust
#[test]
fn test_notification_through_tree() { }

#[test]
fn test_notification_listener_element() { }

#[test]
fn test_scroll_notification_integration() { }

#[test]
fn test_multiple_notification_types() { }

#[test]
fn test_notification_performance() { }
```

---

## Breaking Changes

**None!** All additions are new APIs.

---

## Files to Create/Modify

### New Files
1. **`src/notification/mod.rs`** (~300 lines)
   - Notification trait
   - AnyNotification trait
   - Built-in notification types

2. **`src/notification/listener.rs`** (~200 lines)
   - NotificationListener widget
   - NotificationListenerElement

3. **`tests/notification_tests.rs`** (~400 lines)
   - Comprehensive tests

### Modified Files
1. **`src/lib.rs`** (+5 lines)
   - Export notification module

2. **`src/context/mod.rs`** (+20 lines)
   - Add dispatch_notification()

3. **`src/element/any_element.rs`** (+10 lines)
   - Add visit_notification() method

---

## Success Criteria

✅ **Phase 11 is complete when:**

1. [ ] Notification trait implemented
2. [ ] NotificationListener widget working
3. [ ] dispatch_notification() on Context
4. [ ] Bubbling algorithm functional
5. [ ] Built-in notifications (Scroll, LayoutChanged, etc.)
6. [ ] Type-safe downcast working
7. [ ] 15+ tests passing
8. [ ] Complete documentation
9. [ ] Zero overhead when unused

---

## Next Steps After Phase 11

1. **Phase 12**: Advanced Widget Types
2. **Phase 13**: Rendering Infrastructure (to make things visible!)

---

**Last Updated:** 2025-10-20
**Status:** 🚧 Design Complete, Ready for Implementation
**Estimated Time:** 3-4 hours

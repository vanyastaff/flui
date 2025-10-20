# Phase 10: Error Handling & Debugging - Design Document

**Date:** 2025-10-20
**Status:** 🚧 In Progress
**Priority:** HIGH
**Complexity:** MEDIUM-HIGH

---

## Overview

This phase adds comprehensive error handling and debugging infrastructure to Flui, matching Flutter's debugging capabilities. This includes ErrorWidget for displaying exceptions, debug flags, element diagnostic trees, and lifecycle validation.

### Current State

✅ **Already Implemented:**
- Basic element tree structure
- Widget lifecycle (mount, unmount, update)
- Context API with navigation
- InheritedWidget dependency tracking

❌ **Missing:**
- ErrorWidget for displaying exceptions
- Debug flags (debug_print_build_scope, etc.)
- Element diagnostic tree printing
- Widget inspector support
- Lifecycle validation and assertions
- Better error messages
- Global key uniqueness validation

### Goals

1. **ErrorWidget**: Display exceptions gracefully (red screen in debug, gray in release)
2. **Debug Infrastructure**: Flags, logging, and diagnostic tools
3. **Diagnostic Trees**: Print element tree with details for debugging
4. **Lifecycle Validation**: Assertions to catch bugs early
5. **Better Error Messages**: Clear, actionable error messages
6. **Production Ready**: Minimal overhead in release builds

---

## Architecture

### 1. ErrorWidget

Flutter's ErrorWidget displays exceptions with a red screen in debug mode:

```rust
/// Widget that displays an error message
#[derive(Debug, Clone)]
pub struct ErrorWidget {
    message: String,
    details: Option<String>,
    error: Arc<Box<dyn std::error::Error + Send + Sync>>,
}

impl ErrorWidget {
    /// Create from error
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            message: error.to_string(),
            details: None,
            error: Arc::new(Box::new(error)),
        }
    }

    /// Create with custom message
    pub fn with_message(message: impl Into<String>) -> Self;

    /// Set error details
    pub fn with_details(mut self, details: impl Into<String>) -> Self;

    /// Get error reference
    pub fn error(&self) -> &(dyn std::error::Error + Send + Sync);
}

impl StatelessWidget for ErrorWidget {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Debug mode: Red screen with error details
        #[cfg(debug_assertions)]
        {
            Box::new(ErrorDisplay {
                background: Color::RED,
                message: self.message.clone(),
                details: self.details.clone(),
            })
        }

        // Release mode: Simple gray box
        #[cfg(not(debug_assertions))]
        {
            Box::new(Container {
                color: Color::GRAY,
                child: None,
            })
        }
    }
}
```

### 2. Debug Flags

Global debug flags for controlling debug output:

```rust
/// Global debug configuration
pub struct DebugFlags {
    /// Print when build() is called
    pub debug_print_build_scope: bool,

    /// Print when markNeedsBuild() is called
    pub debug_print_mark_needs_build: bool,

    /// Print when layout() is called
    pub debug_print_layout: bool,

    /// Print when rebuild is scheduled
    pub debug_print_schedule_build: bool,

    /// Print global key registration
    pub debug_print_global_key_registry: bool,

    /// Enable lifecycle validation
    pub debug_check_element_lifecycle: bool,

    /// Enable intrinsic size checks
    pub debug_check_intrinsic_sizes: bool,
}

impl DebugFlags {
    /// Global instance (thread-local)
    pub fn global() -> &'static RwLock<Self>;

    /// Create default (all false)
    pub fn new() -> Self;

    /// Enable all debug flags
    pub fn all() -> Self;
}

// Usage:
DebugFlags::global().write().debug_print_build_scope = true;
```

### 3. Diagnostic Tree Printing

```rust
/// Element diagnostic information
#[derive(Debug, Clone)]
pub struct DiagnosticNode {
    pub name: String,
    pub description: String,
    pub properties: Vec<(String, String)>,
    pub children: Vec<DiagnosticNode>,
}

impl DiagnosticNode {
    /// Print diagnostic tree to string
    pub fn to_string_deep(&self) -> String;

    /// Print diagnostic tree with indentation
    pub fn to_string_tree(&self, indent: usize) -> String;
}

pub trait DiagnosticableTree {
    /// Create diagnostic description
    fn to_diagnostic_node(&self) -> DiagnosticNode;

    /// Print diagnostic tree
    fn debug_print_tree(&self) {
        println!("{}", self.to_diagnostic_node().to_string_deep());
    }
}

// Implement for AnyElement
impl dyn AnyElement {
    /// Create diagnostic node
    pub fn to_diagnostic_node(&self) -> DiagnosticNode {
        DiagnosticNode {
            name: self.widget_type_name(),
            description: format!("#{:?}", self.id()),
            properties: vec![
                ("depth".to_string(), self.depth().to_string()),
                ("dirty".to_string(), self.is_dirty().to_string()),
                ("mounted".to_string(), self.mounted().to_string()),
            ],
            children: vec![],
        }
    }

    /// Print element subtree
    pub fn debug_print_tree(&self) {
        println!("{}", self.to_diagnostic_node().to_string_deep());
    }
}

// Implement for ElementTree
impl ElementTree {
    /// Print entire tree
    pub fn debug_print_tree(&self) {
        if let Some(root_id) = self.root_element_id() {
            if let Some(root) = self.get(root_id) {
                root.debug_print_tree();
            }
        }
    }
}
```

Example output:
```
RenderObjectWidget<Container> #ElementId(1)
  depth: 0
  dirty: false
  mounted: true
  ├─ ComponentElement<StatelessWidget<MyApp>> #ElementId(2)
  │   depth: 1
  │   dirty: false
  │   mounted: true
  │   └─ RenderObjectWidget<Column> #ElementId(3)
  │       depth: 2
  │       dirty: false
  │       mounted: true
  │       ├─ RenderObjectWidget<Text> #ElementId(4)
  │       └─ RenderObjectWidget<Button> #ElementId(5)
```

### 4. Lifecycle Validation

Assertions to catch common bugs:

```rust
/// Lifecycle state validation
pub struct LifecycleValidator {
    element_id: ElementId,
    state: LifecycleState,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LifecycleState {
    Created,
    Mounting,
    Mounted,
    Updating,
    Unmounting,
    Unmounted,
}

impl LifecycleValidator {
    pub fn new(element_id: ElementId) -> Self;

    /// Assert element is mounted
    pub fn assert_mounted(&self) {
        assert!(
            self.state == LifecycleState::Mounted,
            "Element {:?} is not mounted (state: {:?})",
            self.element_id,
            self.state
        );
    }

    /// Assert element can be updated
    pub fn assert_can_update(&self) {
        assert!(
            matches!(self.state, LifecycleState::Mounted | LifecycleState::Updating),
            "Cannot update element {:?} in state {:?}",
            self.element_id,
            self.state
        );
    }

    /// Assert element can be unmounted
    pub fn assert_can_unmount(&self) {
        assert!(
            self.state != LifecycleState::Unmounted,
            "Element {:?} is already unmounted",
            self.element_id
        );
    }
}

// Add to AnyElement:
pub trait AnyElement {
    // ... existing methods

    /// Get lifecycle state (debug only)
    #[cfg(debug_assertions)]
    fn lifecycle_state(&self) -> LifecycleState;

    /// Validate lifecycle state (debug only)
    #[cfg(debug_assertions)]
    fn assert_mounted(&self) {
        assert!(
            self.mounted(),
            "Element {:?} is not mounted",
            self.id()
        );
    }
}
```

### 5. Global Key Validation

Ensure global keys are unique:

```rust
/// Global key registry for validation
pub struct GlobalKeyRegistry {
    keys: HashMap<GlobalKey, ElementId>,
}

impl GlobalKeyRegistry {
    /// Get global instance
    pub fn global() -> &'static RwLock<Self>;

    /// Register key with element
    pub fn register(&mut self, key: GlobalKey, element_id: ElementId) -> Result<(), KeyError>;

    /// Unregister key
    pub fn unregister(&mut self, key: &GlobalKey);

    /// Check if key is registered
    pub fn is_registered(&self, key: &GlobalKey) -> bool;

    /// Get element for key
    pub fn get_element(&self, key: &GlobalKey) -> Option<ElementId>;
}

#[derive(Debug, Clone)]
pub enum KeyError {
    DuplicateKey(GlobalKey),
    KeyNotFound(GlobalKey),
}

impl std::error::Error for KeyError {}

impl Display for KeyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::DuplicateKey(key) => write!(
                f,
                "Duplicate GlobalKey detected: {:?}. Each GlobalKey must be unique.",
                key
            ),
            KeyError::KeyNotFound(key) => write!(
                f,
                "GlobalKey not found: {:?}",
                key
            ),
        }
    }
}
```

### 6. Better Error Messages

```rust
/// Error types for Flui
#[derive(Debug, Clone)]
pub enum FluiError {
    /// Widget build failed
    BuildFailed {
        widget_type: &'static str,
        element_id: ElementId,
        source: Arc<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Element not found in tree
    ElementNotFound {
        element_id: ElementId,
    },

    /// Lifecycle violation
    LifecycleViolation {
        element_id: ElementId,
        expected_state: LifecycleState,
        actual_state: LifecycleState,
        operation: &'static str,
    },

    /// Key error
    KeyError {
        error: KeyError,
    },

    /// InheritedWidget not found
    InheritedWidgetNotFound {
        widget_type: &'static str,
        context_element_id: ElementId,
    },
}

impl Display for FluiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FluiError::BuildFailed { widget_type, element_id, source } => write!(
                f,
                "Failed to build widget '{}' (element {:?}): {}",
                widget_type, element_id, source
            ),
            FluiError::ElementNotFound { element_id } => write!(
                f,
                "Element {:?} not found in tree. It may have been unmounted.",
                element_id
            ),
            FluiError::LifecycleViolation { element_id, expected_state, actual_state, operation } => write!(
                f,
                "Lifecycle violation for element {:?}: Cannot {} in state {:?} (expected {:?})",
                element_id, operation, actual_state, expected_state
            ),
            FluiError::KeyError { error } => write!(f, "{}", error),
            FluiError::InheritedWidgetNotFound { widget_type, context_element_id } => write!(
                f,
                "No InheritedWidget of type '{}' found in ancestor tree of element {:?}. Did you forget to wrap your app with the widget?",
                widget_type, context_element_id
            ),
        }
    }
}

impl std::error::Error for FluiError {}
```

---

## Implementation Plan

### Step 1: ErrorWidget ✅
- [ ] Create `src/error_widget.rs`
- [ ] Implement ErrorWidget struct
- [ ] Add debug/release mode rendering
- [ ] Add unit tests

### Step 2: Debug Flags ✅
- [ ] Create `src/debug/mod.rs`
- [ ] Implement DebugFlags struct with thread-local global
- [ ] Add flag checks in element lifecycle methods
- [ ] Add unit tests

### Step 3: Diagnostic Tree ✅
- [ ] Create `src/debug/diagnostics.rs`
- [ ] Implement DiagnosticNode
- [ ] Add DiagnosticableTree trait
- [ ] Implement for AnyElement and ElementTree
- [ ] Add tree printing methods
- [ ] Add unit tests

### Step 4: Lifecycle Validation ✅
- [ ] Create `src/debug/lifecycle.rs`
- [ ] Implement LifecycleValidator
- [ ] Add lifecycle state tracking to elements
- [ ] Add assertion methods
- [ ] Integrate with element lifecycle
- [ ] Add unit tests

### Step 5: Global Key Registry ✅
- [ ] Create `src/debug/key_registry.rs`
- [ ] Implement GlobalKeyRegistry with HashMap
- [ ] Add registration/unregistration
- [ ] Integrate with element mount/unmount
- [ ] Add unit tests

### Step 6: Error Types ✅
- [ ] Create `src/error.rs`
- [ ] Implement FluiError enum
- [ ] Implement KeyError
- [ ] Add Display and Error trait implementations
- [ ] Add helper functions for common errors
- [ ] Add unit tests

### Step 7: Integration ✅
- [ ] Integrate ErrorWidget with element tree
- [ ] Add debug flag checks in element methods
- [ ] Add lifecycle validation in element methods
- [ ] Add key validation in element mount/unmount
- [ ] Update Context methods to use FluiError

### Step 8: Testing ✅
- [ ] 20+ comprehensive tests
- [ ] Test ErrorWidget rendering
- [ ] Test debug flag behavior
- [ ] Test diagnostic tree printing
- [ ] Test lifecycle validation
- [ ] Test key uniqueness validation
- [ ] Test error messages

### Step 9: Documentation ✅
- [ ] Update API documentation
- [ ] Add usage examples
- [ ] Create completion document

---

## API Examples

### Example 1: ErrorWidget

```rust
use flui_core::*;

// Create app with error handling
fn build_app() -> Box<dyn AnyWidget> {
    match try_build_app() {
        Ok(widget) => widget,
        Err(e) => Box::new(ErrorWidget::new(e).with_details("Failed to build app")),
    }
}

// ErrorWidget automatically shows red screen in debug, gray in release
```

### Example 2: Debug Flags

```rust
use flui_core::debug::DebugFlags;

// Enable debug printing
fn setup_debug() {
    let mut flags = DebugFlags::global().write();
    flags.debug_print_build_scope = true;
    flags.debug_print_mark_needs_build = true;
    flags.debug_check_element_lifecycle = true;
}

// Now when widgets build:
// [BUILD] StatelessWidget<MyApp> #ElementId(1)
// [BUILD] StatelessWidget<MyButton> #ElementId(2)
```

### Example 3: Diagnostic Tree

```rust
use flui_core::*;

fn debug_tree(tree: &ElementTree) {
    // Print entire tree
    tree.debug_print_tree();

    // Output:
    // RenderObjectWidget<Container> #ElementId(1)
    //   depth: 0
    //   dirty: false
    //   mounted: true
    //   ├─ ComponentElement<StatelessWidget<MyApp>> #ElementId(2)
    //   │   depth: 1
    //   │   dirty: false
    //   │   mounted: true
}
```

### Example 4: Lifecycle Validation

```rust
use flui_core::*;

impl StatelessWidget for MyWidget {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Debug assertion - automatically checked in debug builds
        #[cfg(debug_assertions)]
        context.assert_mounted();

        // This will panic in debug if context is not mounted!
        Box::new(Text::new("Hello"))
    }
}
```

### Example 5: Better Error Messages

```rust
use flui_core::*;

// Try to access InheritedWidget
let theme = context.inherit::<Theme>()
    .ok_or_else(|| FluiError::InheritedWidgetNotFound {
        widget_type: "Theme",
        context_element_id: context.id(),
    })?;

// Error message:
// "No InheritedWidget of type 'Theme' found in ancestor tree of element ElementId(5).
//  Did you forget to wrap your app with the widget?"
```

---

## Comparison with Flutter

| Feature | Flutter | Flui (Phase 10) | Status |
|---------|---------|-----------------|--------|
| ErrorWidget | ✅ | ✅ | **Planned** |
| Debug flags | ✅ | ✅ | **Planned** |
| Diagnostic tree | ✅ | ✅ | **Planned** |
| Lifecycle validation | ✅ | ✅ | **Planned** |
| Global key registry | ✅ | ✅ | **Planned** |
| Better error messages | ✅ | ✅ | **Planned** |
| Widget inspector | ✅ | ⏸️ | **Future** |
| DevTools integration | ✅ | ⏸️ | **Future** |

**Result:** Core debugging infrastructure **100% Flutter-compatible**!

---

## Performance Considerations

### Debug vs Release

All debug infrastructure should have **zero overhead in release builds**:

```rust
// Use #[cfg(debug_assertions)] for debug-only code
#[cfg(debug_assertions)]
fn validate_lifecycle(&self) {
    self.assert_mounted();
}

// Use debug flags for runtime control
if DebugFlags::global().read().debug_print_build_scope {
    println!("[BUILD] {}", widget.type_name());
}
```

### Diagnostic Tree Performance

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `to_diagnostic_node()` | O(1) | Create single node |
| `to_string_deep()` | O(n) | Visit entire subtree |
| `debug_print_tree()` | O(n) | Visit entire tree |

Only use in debug/development!

---

## Testing Strategy

### Unit Tests (15+ tests)
```rust
#[test]
fn test_error_widget_debug_mode() { }

#[test]
fn test_error_widget_release_mode() { }

#[test]
fn test_debug_flags() { }

#[test]
fn test_diagnostic_node_printing() { }

#[test]
fn test_lifecycle_validation() { }

#[test]
fn test_global_key_uniqueness() { }

#[test]
fn test_error_messages() { }
```

### Integration Tests (5+ tests)
```rust
#[test]
fn test_error_widget_in_tree() { }

#[test]
fn test_debug_flags_in_build() { }

#[test]
fn test_diagnostic_tree_full() { }

#[test]
fn test_lifecycle_validation_integration() { }

#[test]
fn test_key_registry_integration() { }
```

---

## Breaking Changes

**None!** All additions are new APIs or debug-only features.

---

## Files to Create/Modify

### New Files
1. **`src/error_widget.rs`** (~150 lines)
   - ErrorWidget implementation

2. **`src/error.rs`** (~200 lines)
   - FluiError and KeyError types
   - Error message formatting

3. **`src/debug/mod.rs`** (~100 lines)
   - DebugFlags implementation
   - Module exports

4. **`src/debug/diagnostics.rs`** (~250 lines)
   - DiagnosticNode implementation
   - Tree printing

5. **`src/debug/lifecycle.rs`** (~150 lines)
   - LifecycleValidator implementation
   - Lifecycle assertions

6. **`src/debug/key_registry.rs`** (~150 lines)
   - GlobalKeyRegistry implementation

### Modified Files
1. **`src/lib.rs`** (+10 lines)
   - Export error types
   - Export debug module

2. **`src/element/mod.rs`** (+20 lines)
   - Add lifecycle validation
   - Add debug printing

3. **`src/element/any_element.rs`** (+30 lines)
   - Add diagnostic methods
   - Add lifecycle state methods

4. **`src/context/mod.rs`** (+20 lines)
   - Add debug assertions
   - Return FluiError from methods

### Test Files
1. **`tests/error_handling_tests.rs`** (new, ~500 lines)
   - Comprehensive error handling tests

---

## Success Criteria

✅ **Phase 10 is complete when:**

1. [ ] ErrorWidget implemented and tested
2. [ ] Debug flags working in element lifecycle
3. [ ] Diagnostic tree printing functional
4. [ ] Lifecycle validation integrated
5. [ ] Global key registry validates uniqueness
6. [ ] Better error messages with FluiError
7. [ ] 20+ tests passing
8. [ ] Zero overhead in release builds
9. [ ] Complete documentation

---

## Next Steps After Phase 10

1. **Phase 11**: Notification System
2. **Phase 12**: Focus & Input Handling
3. **Phase 13**: Performance Optimizations

---

**Last Updated:** 2025-10-20
**Status:** 🚧 Design Complete, Ready for Implementation
**Estimated Time:** 5-6 hours

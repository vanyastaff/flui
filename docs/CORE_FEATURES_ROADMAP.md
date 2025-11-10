# FLUI Core Features Roadmap

**Version:** 0.1.0
**Date:** 2025-11-10
**Status:** Planning Document

## Executive Summary

This document outlines missing core infrastructure features in `flui_core` based on comprehensive analysis of competing frameworks (Flutter, Xilem, GPUI) and FLUI's existing architecture. The focus is on **core-level infrastructure** that belongs in `flui_core`, not higher-level features that should live in separate crates.

### Key Findings

**What FLUI Already Has (Strong Foundation):**
- ✅ Three-tree architecture (View → Element → Render)
- ✅ Event system with hit testing (`flui_engine::EventRouter`, `flui_types::events`)
- ✅ Notification bubbling system (`foundation/notification.rs`)
- ✅ Hooks for state management (`use_signal`, `use_memo`, `use_effect`)
- ✅ Thread-safe architecture with Arc/Mutex
- ✅ Element lifecycle management
- ✅ Provider system for data propagation

**Critical Missing Infrastructure:**
1. **Focus Management System** - No FocusNode/FocusScope (hybrid: persistent objects + widgets)
2. **Action Foundation** - No Action trait for type-safe commands (traits in core, widgets in flui_widgets)
3. **Scroll Infrastructure** - No ScrollController/ScrollPosition (persistent object + widgets/rendering)
4. **Overlay System** - No OverlayEntry/OverlayState (hybrid: persistent objects + widgets)
5. **Platform Abstraction** - No platform traits for clipboard/locale/brightness

**Architecture Pattern:** Following Flutter's **persistent object pattern** for controllers:
- **Persistent objects** (FocusNode, ScrollController, OverlayEntry) in `flui_core/foundation` - Arc-based, extend ChangeNotifier/Listenable
- **Lifecycle widgets** (Focus, ListView, Overlay) in `flui_widgets` - manage object lifecycle
- **Render objects** (RenderScrollable) in `flui_rendering` - handle gestures and painting

**Total Work Estimate:** ~1,400 LOC in core + ~800 LOC in widgets/rendering (Focus: 400+200, Actions: 150+250, Scroll: 350+200, Overlay: 250+150, Platform: 200)

---

## 1. Architecture Analysis

### 1.1 Current FLUI Core Structure

```
flui_core/
├── foundation/        # Core types and utilities
│   ├── atomic_flags.rs
│   ├── change_notifier.rs
│   ├── diagnostics.rs
│   ├── element_id.rs
│   ├── error.rs
│   ├── key.rs
│   ├── notification.rs  ✅ Bubbling events
│   └── slot.rs
├── element/           # Element tree (mutable state)
│   ├── component.rs
│   ├── dependency.rs
│   ├── hit_test.rs     ✅ Element-level hit testing
│   ├── provider.rs
│   └── render.rs
├── hooks/             # Reactive state management
│   ├── signal.rs       ✅ Fine-grained reactivity
│   ├── memo.rs
│   ├── effect.rs
│   └── resource.rs
├── pipeline/          # Build/Layout/Paint phases
│   ├── build_pipeline.rs
│   ├── layout_pipeline.rs
│   └── paint_pipeline.rs
├── render/            # Render traits (LeafRender, etc.)
└── view/              # View system (unified View trait)
    └── build_context.rs  ⚠️ Needs enhancement
```

### 1.2 Related Crates

**flui_types** (Foundation types):
- ✅ `events.rs` - Complete event types (PointerEvent, KeyEvent, WindowEvent)
- ✅ `gestures/` - Gesture detail types (TapDetails, DragDetails, ScaleDetails)

**flui_engine** (Rendering engine):
- ✅ `event_router.rs` - EventRouter with hit testing and focus/visibility tracking
- ✅ `layer/` - Layer system with PointerListenerLayer

**flui_gestures** (Gesture recognition):
- ⚠️ `recognizers/` - Partial implementation (tap recognizer exists)
- ⚠️ `detector.rs` - GestureDetector widget (incomplete)

### 1.3 Comparison with Other Frameworks

| Feature | Flutter | GPUI | Xilem | FLUI |
|---------|---------|------|-------|------|
| **Three-tree Architecture** | ✅ Full | ⚠️ Hybrid | ✅ Full | ✅ Full |
| **Event System** | ✅ Full | ✅ Full | ⚠️ Basic | ✅ Good |
| **Notification Bubbling** | ✅ Full | ❌ None | ❌ None | ✅ Full |
| **Focus Management** | ✅ FocusNode/Scope | ✅ Built-in | ⚠️ Basic | ❌ **Missing** |
| **Action/Intent System** | ✅ Intent/Action (widgets) | ✅ Actions | ❌ None | ⚠️ **Traits only** |
| **Scroll Controller** | ✅ Full | ✅ Good | ⚠️ Basic | ❌ **Missing** |
| **Overlay System** | ✅ OverlayEntry | ❌ None | ❌ None | ❌ **Missing** |
| **Hooks System** | ❌ None | ❌ None | ⚠️ Lenses | ✅ Full |
| **Thread Safety** | ❌ Single-thread | ✅ Async | ✅ Yes | ✅ Full |
| **GPU Rendering** | ✅ Skia | ✅ Blade | ✅ Vello | ✅ wgpu |

**Key Insight:** FLUI has a strong foundation (three-tree, events, notifications, hooks) but lacks **focus management** and **programmatic control** features (ScrollController, Overlay) that other frameworks provide. Actions/Intents will be implemented as widgets (like Flutter), with only foundation traits in core.

---

## 2. Missing Core Features

### 2.1 Focus Management System

**Priority:** ⭐⭐⭐ CRITICAL (P0)
**Estimated Size:** ~400 LOC in core + ~200 LOC in widgets
**Location:**
- Core: `crates/flui_core/src/foundation/focus.rs` (persistent objects)
- Widgets: `crates/flui_widgets/src/focus.rs` (Focus/FocusScope widgets)

#### Problem Statement

FLUI has `FocusChangedNotification` in foundation but no actual focus management infrastructure:
- No `FocusNode` to represent focusable elements
- No `FocusScope` to manage focus within subtrees
- No focus traversal (Tab/Shift+Tab navigation)
- No global `FocusManager` to track primary focus
- BuildContext cannot request/query focus state

This prevents:
- Keyboard navigation between form fields
- Accessibility features (screen readers need focus info)
- Modal dialogs (need focus scopes)
- Keyboard shortcuts bound to focused widget

**Inspiration:** Flutter's hybrid approach - FocusNode (persistent object) + Focus widget (lifecycle manager).

#### Architecture Decision: Hybrid Approach (Like Flutter)

**Flutter uses BOTH:**
1. **FocusNode** - Long-lived persistent object (like RenderObject, NOT a widget)
2. **Focus widget** - Manages FocusNode lifecycle, attaches/detaches from focus tree

**From Flutter docs:**
> "FocusNodes are long-lived objects (longer than widgets, similar to render objects) that hold the focus state and attributes so that they are persistent between builds of the widget tree."

**Flutter Example:**
```dart
// FocusNode - persistent object (created once)
class MyWidget extends StatefulWidget {
  @override
  State createState() => _MyWidgetState();
}

class _MyWidgetState extends State<MyWidget> {
  late FocusNode _focusNode;

  @override
  void initState() {
    super.initState();
    _focusNode = FocusNode();  // Long-lived persistent object
  }

  @override
  void dispose() {
    _focusNode.dispose();  // Clean up
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // Focus widget wraps and manages the FocusNode
    return Focus(
      focusNode: _focusNode,
      child: TextField(),
    );
  }
}

// Or let Focus widget create internal FocusNode
Focus(
  autofocus: true,
  onFocusChange: (hasFocus) => print('Focus: $hasFocus'),
  child: TextField(),
)
```

**FLUI will follow the same pattern:**
- ✅ `FocusNode` persistent object in `flui_core` (foundation)
- ✅ `Focus` and `FocusScope` widgets in `flui_widgets`
- ✅ Widget manages node lifecycle (attach/detach/reparent)
- ✅ Supports both external and internal FocusNode

#### Proposed API (Core Foundation Only)

**In `flui_core` - Persistent objects and focus tree logic:**

```rust
// crates/flui_core/src/foundation/focus.rs
//
// IMPORTANT: FocusNode is a PERSISTENT OBJECT, not a widget!
// Similar to RenderObject - lives longer than widgets.

/// Focus node - represents a focusable element in the tree
///
/// FocusNodes form a parallel tree to the element tree, tracking which
/// elements can receive keyboard input. Each focusable widget should
/// create a FocusNode and attach it to the tree.
///
/// # Thread Safety
///
/// FocusNode uses Arc<Mutex<>> for thread-safe focus management,
/// consistent with FLUI's thread-safe architecture.
///
/// # Example
///
/// ```rust,ignore
/// let focus_node = FocusNode::new(element_id);
/// focus_node.request_focus();
///
/// if focus_node.has_focus() {
///     // Handle keyboard input
/// }
/// ```
#[derive(Debug)]
pub struct FocusNode {
    element_id: ElementId,
    can_request_focus: bool,
    skip_traversal: bool,
    state: Arc<Mutex<FocusNodeState>>,
}

#[derive(Debug)]
struct FocusNodeState {
    has_focus: bool,
    children: Vec<Arc<FocusNode>>,
    parent: Option<Weak<FocusNode>>,
    // Callback invoked when focus changes
    on_focus_change: Option<Arc<dyn Fn(bool) + Send + Sync>>,
}

impl FocusNode {
    /// Create a new focus node
    pub fn new(element_id: ElementId) -> Arc<Self> {
        Arc::new(Self {
            element_id,
            can_request_focus: true,
            skip_traversal: false,
            state: Arc::new(Mutex::new(FocusNodeState {
                has_focus: false,
                children: Vec::new(),
                parent: None,
                on_focus_change: None,
            })),
        })
    }

    /// Request focus for this node
    pub fn request_focus(&self) {
        if self.can_request_focus {
            FocusManager::global().request_focus(self);
        }
    }

    /// Remove focus from this node
    pub fn unfocus(&self) {
        let mut state = self.state.lock();
        if state.has_focus {
            state.has_focus = false;
            if let Some(callback) = &state.on_focus_change {
                callback(false);
            }
        }
    }

    /// Check if this node currently has focus
    pub fn has_focus(&self) -> bool {
        self.state.lock().has_focus
    }

    /// Get next focusable node (for Tab traversal)
    pub fn next_focus(&self) -> Option<Arc<FocusNode>> {
        // Depth-first search for next focusable node
        // Implementation follows Flutter's FocusTraversalPolicy
        todo!("Implement focus traversal")
    }

    /// Get previous focusable node (for Shift+Tab traversal)
    pub fn previous_focus(&self) -> Option<Arc<FocusNode>> {
        todo!("Implement reverse focus traversal")
    }

    /// Attach child focus node
    pub fn attach_child(&self, child: Arc<FocusNode>) {
        let mut state = self.state.lock();
        state.children.push(child.clone());

        let mut child_state = child.state.lock();
        child_state.parent = Some(Arc::downgrade(&Arc::new(self.clone())));
    }

    /// Set focus change callback
    pub fn set_on_focus_change(&self, callback: impl Fn(bool) + Send + Sync + 'static) {
        self.state.lock().on_focus_change = Some(Arc::new(callback));
    }
}

/// Focus scope - manages focus within a subtree
///
/// Focus scopes create boundaries for focus traversal. When Tab reaches
/// the end of a scope, it wraps to the beginning (if configured) or
/// moves to the next scope.
///
/// # Example
///
/// ```rust,ignore
/// // Create scope for a dialog
/// let dialog_scope = FocusScope::new(dialog_element_id)
///     .with_autofocus(first_field_id);
///
/// // Tab navigation stays within dialog
/// dialog_scope.request_focus();
/// ```
#[derive(Debug)]
pub struct FocusScope {
    node: Arc<FocusNode>,
    autofocus: Option<ElementId>,
    trap_focus: bool,  // Prevent Tab from leaving scope
}

impl FocusScope {
    pub fn new(element_id: ElementId) -> Self {
        Self {
            node: FocusNode::new(element_id),
            autofocus: None,
            trap_focus: false,
        }
    }

    /// Set element that should be focused when scope gains focus
    pub fn with_autofocus(mut self, element_id: ElementId) -> Self {
        self.autofocus = Some(element_id);
        self
    }

    /// Prevent Tab key from leaving this scope (for modals)
    pub fn with_focus_trap(mut self) -> Self {
        self.trap_focus = true;
        self
    }

    /// Request focus for this scope (focuses autofocus element if set)
    pub fn request_focus(&self) {
        self.node.request_focus();

        if let Some(element_id) = self.autofocus {
            // Focus the autofocus element
            // Implementation requires access to element tree
            todo!("Focus autofocus element")
        }
    }
}

/// Global focus manager
///
/// Singleton that manages primary focus state across the entire application.
/// Thread-safe for use in multi-threaded UI.
///
/// # Example
///
/// ```rust,ignore
/// let manager = FocusManager::global();
///
/// // Request focus
/// manager.request_focus(my_node.clone());
///
/// // Check primary focus
/// if let Some(focused) = manager.primary_focus() {
///     println!("Element {} has focus", focused.element_id);
/// }
/// ```
#[derive(Debug)]
pub struct FocusManager {
    primary_focus: Arc<Mutex<Option<Arc<FocusNode>>>>,
    root_scope: Arc<Mutex<Option<Arc<FocusScope>>>>,
}

impl FocusManager {
    /// Get global focus manager instance
    pub fn global() -> &'static FocusManager {
        static INSTANCE: OnceCell<FocusManager> = OnceCell::new();
        INSTANCE.get_or_init(|| FocusManager {
            primary_focus: Arc::new(Mutex::new(None)),
            root_scope: Arc::new(Mutex::new(None)),
        })
    }

    /// Request focus for a node
    pub fn request_focus(&self, node: &FocusNode) {
        let mut primary = self.primary_focus.lock();

        // Unfocus old node
        if let Some(old_node) = primary.take() {
            old_node.unfocus();
        }

        // Focus new node
        let mut state = node.state.lock();
        state.has_focus = true;
        *primary = Some(Arc::new(node.clone()));

        // Notify listeners
        if let Some(callback) = &state.on_focus_change {
            callback(true);
        }
    }

    /// Get currently focused node
    pub fn primary_focus(&self) -> Option<Arc<FocusNode>> {
        self.primary_focus.lock().clone()
    }

    /// Set root focus scope
    pub fn set_root_scope(&self, scope: Arc<FocusScope>) {
        *self.root_scope.lock() = Some(scope);
    }

    /// Handle Tab key (traverse to next)
    pub fn handle_tab(&self) {
        if let Some(current) = self.primary_focus() {
            if let Some(next) = current.next_focus() {
                self.request_focus(&next);
            }
        }
    }

    /// Handle Shift+Tab key (traverse to previous)
    pub fn handle_shift_tab(&self) {
        if let Some(current) = self.primary_focus() {
            if let Some(prev) = current.previous_focus() {
                self.request_focus(&prev);
            }
        }
    }
}

/// Focus traversal direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTraversalDirection {
    /// Next focusable (Tab)
    Next,
    /// Previous focusable (Shift+Tab)
    Previous,
    /// Up arrow
    Up,
    /// Down arrow
    Down,
    /// Left arrow
    Left,
    /// Right arrow
    Right,
}
```

**That's it for core!** The rest is in `flui_widgets`.

#### Widget Implementation (in `flui_widgets`)

**Focus widget (`flui_widgets/src/focus.rs`):**

```rust
/// Focus widget - manages FocusNode lifecycle
///
/// Similar to Flutter's Focus widget. Can either create an internal
/// FocusNode or use an externally-provided one.
pub struct Focus {
    focus_node: Option<Arc<FocusNode>>,
    autofocus: bool,
    on_focus_change: Option<Box<dyn Fn(bool) + Send + Sync>>,
    child: AnyElement,
}

impl Focus {
    pub fn new() -> Self {
        Self {
            focus_node: None,  // Will create internal node
            autofocus: false,
            on_focus_change: None,
            child: EmptyElement,
        }
    }

    /// Use external FocusNode (for programmatic control)
    pub fn focus_node(mut self, node: Arc<FocusNode>) -> Self {
        self.focus_node = Some(node);
        self
    }

    /// Request focus when widget mounts
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }

    /// Callback when focus changes
    pub fn on_focus_change(mut self, callback: impl Fn(bool) + Send + Sync + 'static) -> Self {
        self.on_focus_change = Some(Box::new(callback));
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for Focus {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Get or create FocusNode
        let node = self.focus_node.unwrap_or_else(|| FocusNode::new(ctx.element_id));

        // Register callback if provided
        if let Some(callback) = self.on_focus_change {
            node.set_on_focus_change(callback);
        }

        // Autofocus if requested
        if self.autofocus {
            node.request_focus();
        }

        // Attach node to focus tree via Provider
        FocusProvider::new(node.clone())
            .child(self.child)
    }
}

/// FocusScope widget - manages focus within a subtree
///
/// Similar to Flutter's FocusScope widget.
pub struct FocusScope {
    node: Option<Arc<FocusNode>>,
    autofocus_element: Option<ElementId>,
    trap_focus: bool,  // Prevent Tab from leaving scope (for modals)
    child: AnyElement,
}

impl FocusScope {
    pub fn new() -> Self { ... }

    pub fn autofocus_element(mut self, element_id: ElementId) -> Self {
        self.autofocus_element = Some(element_id);
        self
    }

    pub fn trap_focus(mut self) -> Self {
        self.trap_focus = true;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for FocusScope {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create scope node
        let node = self.node.unwrap_or_else(|| FocusNode::new(ctx.element_id));

        // Register as scope in focus tree
        FocusScopeProvider::new(node.clone(), self.trap_focus)
            .child(self.child)
    }
}
```

#### Usage Examples

**Example 1: Automatic FocusNode (managed by widget)**
```rust
// Widget creates and manages FocusNode internally
Focus::new()
    .autofocus()
    .on_focus_change(|has_focus| {
        println!("Focus changed: {}", has_focus);
    })
    .child(TextField::new())
```

**Example 2: External FocusNode (for programmatic control)**
```rust
// User creates and manages FocusNode
struct MyView {
    focus_node: Arc<FocusNode>,
    text: Arc<Mutex<String>>,
}

impl View for MyView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Column::new()
            .children(vec![
                // Pass our FocusNode to Focus widget
                Box::new(
                    Focus::new()
                        .focus_node(self.focus_node.clone())
                        .child(TextField::new(self.text.clone()))
                ),
                Box::new(
                    Button::new("Focus TextField")
                        .on_press({
                            let node = self.focus_node.clone();
                            move || node.request_focus()
                        })
                ),
            ])
    }
}
```

**Example 3: FocusScope for modal dialogs**
```rust
// Modal dialog with focus trap
FocusScope::new()
    .trap_focus()  // Tab doesn't escape modal
    .autofocus_element(first_field_id)
    .child(
        Column::new()
            .children(vec![
                Box::new(TextField::new()),  // This gets autofocus
                Box::new(TextField::new()),
                Box::new(Button::new("OK")),
                Box::new(Button::new("Cancel")),
            ])
    )
```

#### BuildContext Integration

```rust
// In crates/flui_core/src/view/build_context.rs

impl BuildContext {
    /// Find nearest FocusNode in widget tree
    pub fn focus_node(&self) -> Option<Arc<FocusNode>> {
        // Walk up element tree to find Focus widget (via Provider)
        todo!("Implemented via FocusProvider in flui_widgets")
    }

    /// Request focus for current element
    pub fn request_focus(&self) {
        if let Some(node) = self.focus_node() {
            node.request_focus();
        }
    }

    /// Remove focus from current element
    pub fn unfocus(&self) {
        if let Some(node) = self.focus_node() {
            node.unfocus();
        }
    }

    /// Check if current element has focus
    pub fn has_focus(&self) -> bool {
        self.focus_node()
            .map(|node| node.has_focus())
            .unwrap_or(false)
    }
}
```

#### Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_node_creation() {
        let node = FocusNode::new(ElementId::new(1));
        assert!(!node.has_focus());
        assert!(node.can_request_focus);
    }

    #[test]
    fn test_focus_request() {
        let node = FocusNode::new(ElementId::new(1));
        node.request_focus();
        assert!(node.has_focus());
    }

    #[test]
    fn test_focus_manager_switches_focus() {
        let node1 = FocusNode::new(ElementId::new(1));
        let node2 = FocusNode::new(ElementId::new(2));

        node1.request_focus();
        assert!(node1.has_focus());
        assert!(!node2.has_focus());

        node2.request_focus();
        assert!(!node1.has_focus());
        assert!(node2.has_focus());
    }

    #[test]
    fn test_focus_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let node = FocusNode::new(ElementId::new(1));
        node.set_on_focus_change(move |has_focus| {
            if has_focus {
                called_clone.store(true, Ordering::SeqCst);
            }
        });

        node.request_focus();
        assert!(called.load(Ordering::SeqCst));
    }
}
```

---

### 2.2 Action/Intent System (Foundation Traits)

**Priority:** ⭐⭐ IMPORTANT (P1)
**Estimated Size:** ~150 LOC in core + ~250 LOC in widgets
**Location:**
- Core traits: `crates/flui_core/src/foundation/action.rs`
- Widgets: `crates/flui_widgets/src/shortcuts.rs` and `actions.rs`

#### Problem Statement

FLUI lacks a type-safe command system for:
- Keyboard shortcuts (Ctrl+C, Ctrl+V, etc.)
- Menu commands
- Toolbar buttons
- Context menu actions

Without an Action system:
- Shortcuts are implemented ad-hoc in event handlers
- No central place to see all available commands
- Hard to override/customize shortcuts
- No way to disable actions based on context

**Inspiration:** Flutter's Intent/Action/Shortcuts system (widget-based, NOT global dispatcher).

#### Architecture Decision: Widget-Based (Like Flutter)

**Flutter Approach:**
```dart
// Flutter uses WIDGETS for Actions and Shortcuts
Shortcuts(
  shortcuts: {
    LogicalKeySet(LogicalKeyboardKey.control, LogicalKeyboardKey.keyC): CopyIntent(),
  },
  child: Actions(
    actions: {
      CopyIntent: CopyAction(model),
    },
    child: TextField(...),
  ),
)
```

**FLUI will follow the same pattern:**
- ✅ `Action` trait and common actions in `flui_core` (foundation)
- ✅ `Shortcuts` and `Actions` widgets in `flui_widgets`
- ✅ Widget-based dispatch (NOT global dispatcher)
- ✅ Works with focus system automatically

#### Proposed API (Core Foundation Only)

**In `flui_core` - Only traits and basic action types:**

```rust
// crates/flui_core/src/foundation/action.rs

use std::any::TypeId;
use std::fmt;

/// Type-safe action trait
///
/// Actions represent commands that can be invoked via keyboard shortcuts,
/// menu items, or programmatically. Each action is a unique type that
/// implements this trait.
///
/// # Note
///
/// This is a **foundation trait** only. The actual `Shortcuts` and `Actions`
/// widgets are implemented in `flui_widgets` crate.
///
/// # Example
///
/// ```rust
/// use flui_core::foundation::Action;
///
/// #[derive(Debug, Clone)]
/// struct CopyAction;
///
/// impl Action for CopyAction {}
///
/// #[derive(Debug, Clone)]
/// struct PasteAction;
///
/// impl Action for PasteAction {}
/// ```
pub trait Action: 'static + Send + Sync + fmt::Debug {
    /// Get TypeId for this action (automatic)
    fn action_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Optional: Describe the action for debugging
    fn describe(&self) -> String {
        format!("{:?}", self)
    }
}

/// Action context - provides access to app state during action execution
///
/// Passed to action handlers to give them access to the element tree,
/// build context, and other application state.
pub struct ActionContext<'a> {
    /// Build context for the element that is handling the action
    pub build_context: &'a BuildContext,

    /// Element ID of the handler
    pub element_id: ElementId,

    /// Whether the action originated from a keyboard shortcut
    pub from_keyboard: bool,
}

/// Action handler trait for a specific action type
///
/// Implement this to handle an action. Handlers are registered via the
/// `Actions` widget (in `flui_widgets`), NOT globally.
///
/// # Example
///
/// ```rust,ignore
/// struct CopyHandler {
///     text: Arc<Mutex<String>>,
/// }
///
/// impl ActionHandler<CopyAction> for CopyHandler {
///     fn handle(&self, _action: &CopyAction, ctx: &ActionContext) -> bool {
///         // Copy selected text to clipboard
///         let text = self.text.lock();
///         ctx.build_context.set_clipboard_text(&text).ok();
///         true  // Action handled
///     }
/// }
/// ```
pub trait ActionHandler<A: Action>: Send + Sync {
    /// Handle the action
    ///
    /// Returns `true` if the action was handled, `false` to continue
    /// propagating to parent elements.
    fn handle(&self, action: &A, ctx: &ActionContext) -> bool;
}

// ============================================================================
// Common Actions (Standard Flutter-like actions)
// ============================================================================

/// Copy action (Ctrl+C / Cmd+C)
#[derive(Debug, Clone, Copy)]
pub struct CopyAction;
impl Action for CopyAction {}

/// Cut action (Ctrl+X / Cmd+X)
#[derive(Debug, Clone, Copy)]
pub struct CutAction;
impl Action for CutAction {}

/// Paste action (Ctrl+V / Cmd+V)
#[derive(Debug, Clone, Copy)]
pub struct PasteAction;
impl Action for PasteAction {}

/// Select all action (Ctrl+A / Cmd+A)
#[derive(Debug, Clone, Copy)]
pub struct SelectAllAction;
impl Action for SelectAllAction {}

/// Undo action (Ctrl+Z / Cmd+Z)
#[derive(Debug, Clone, Copy)]
pub struct UndoAction;
impl Action for UndoAction {}

/// Redo action (Ctrl+Y / Cmd+Shift+Z)
#[derive(Debug, Clone, Copy)]
pub struct RedoAction;
impl Action for RedoAction {}
```

**That's it for core!** The rest is in `flui_widgets`.

#### Widget Implementation (in `flui_widgets`)

**Shortcuts widget (`flui_widgets/src/shortcuts.rs`):**

```rust
/// Shortcuts widget - maps keyboard shortcuts to Actions
///
/// Similar to Flutter's Shortcuts widget.
pub struct Shortcuts {
    shortcuts: HashMap<KeyboardShortcut, Box<dyn Action>>,
    child: AnyElement,
}

impl Shortcuts {
    pub fn new() -> Self { ... }

    pub fn shortcut<A: Action + Clone>(mut self, shortcut: KeyboardShortcut, action: A) -> Self {
        self.shortcuts.insert(shortcut, Box::new(action));
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for Shortcuts {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Listen for keyboard events
        // When a shortcut is pressed, dispatch the corresponding action
        // via ctx.dispatch_action()
        KeyboardListener::new()
            .on_key_down(move |event| {
                if let Some(action) = self.find_matching_shortcut(event) {
                    ctx.dispatch_action(action);
                }
            })
            .child(self.child)
    }
}
```

**Actions widget (`flui_widgets/src/actions.rs`):**

```rust
/// Actions widget - registers action handlers for a subtree
///
/// Similar to Flutter's Actions widget.
pub struct Actions {
    handlers: HashMap<TypeId, Box<dyn ErasedActionHandler>>,
    child: AnyElement,
}

impl Actions {
    pub fn new() -> Self { ... }

    pub fn handler<A: Action, H: ActionHandler<A> + 'static>(
        mut self,
        handler: H
    ) -> Self {
        let type_id = TypeId::of::<A>();
        self.handlers.insert(type_id, Box::new(handler));
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for Actions {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Register handlers via Provider pattern
        // (so ctx.dispatch_action() can find them)
        ActionProvider::new(self.handlers)
            .child(self.child)
    }
}
```

#### Usage Example

```rust
// Usage in TextField widget
Shortcuts::new()
    .shortcut(Shortcut::ctrl(Key::C), CopyAction)
    .shortcut(Shortcut::ctrl(Key::V), PasteAction)
    .shortcut(Shortcut::ctrl(Key::A), SelectAllAction)
    .child(
        Actions::new()
            .handler(TextFieldCopyHandler { text: text.clone() })
            .handler(TextFieldPasteHandler { text: text.clone() })
            .handler(TextFieldSelectAllHandler { /* ... */ })
            .child(
                TextField::new(text)
            )
    )

// Handlers implementation
struct TextFieldCopyHandler {
    text: Arc<Mutex<String>>,
}

impl ActionHandler<CopyAction> for TextFieldCopyHandler {
    fn handle(&self, _action: &CopyAction, ctx: &ActionContext) -> bool {
        let text = self.text.lock();
        ctx.build_context.set_clipboard_text(&text).ok();
        true
    }
}
```

**BuildContext Integration:**

```rust
// In crates/flui_core/src/view/build_context.rs

impl BuildContext {
    /// Find action handler in widget tree (walks up like Provider)
    pub fn find_action_handler<A: Action>(&self) -> Option<&dyn ActionHandler<A>> {
        // Walk up element tree to find Actions widget (via Provider)
        todo!("Implemented in flui_widgets via ActionProvider")
    }

    /// Dispatch action (finds handler via widget tree)
    pub fn dispatch_action<A: Action>(&self, action: A) -> bool {
        if let Some(handler) = self.find_action_handler::<A>() {
            handler.handle(&action, &ActionContext {
                build_context: self,
                element_id: self.element_id,
                from_keyboard: false,
            })
        } else {
            false
        }
    }
}
```

---

### 2.3 Scroll Infrastructure

**Priority:** ⭐⭐ IMPORTANT (P1)
**Estimated Size:** ~350 LOC in core + ~200 LOC in widgets
**Location:**
- Core: `crates/flui_core/src/foundation/scroll.rs` (persistent objects)
- Widgets: `crates/flui_widgets/src/scrollable.rs` (ListView/ScrollView widgets)

#### Problem Statement

FLUI has `ScrollNotification` for reactive scroll events, but no programmatic scroll control:
- No `ScrollController` to programmatically scroll
- No `ScrollPosition` to track scroll state
- No `ScrollPhysics` to customize scroll behavior (bounce vs clamp)
- No way to animate scrolling
- No scroll position persistence

This is needed for:
- ListView scrolling to specific items
- ScrollToTop buttons
- Scroll restoration (e.g., after navigation)
- Custom scroll behaviors (iOS bounce, Android edge glow)

**Inspiration:** Flutter's persistent object pattern - ScrollController (extends ChangeNotifier) + ScrollPosition object.

#### Architecture Decision: Persistent Object (Like Flutter)

**Flutter Architecture:**
1. **ScrollController** - Persistent object that extends `ChangeNotifier` (NOT a widget!)
2. **ScrollPosition** - Internal object that tracks scroll state (pixels, min/max extents)
3. **Scrollable widgets** - ListView, GridView, CustomScrollView accept `controller` parameter

**From Flutter source:**
```dart
class ScrollController extends ChangeNotifier {
  ScrollPosition createScrollPosition(
    ScrollPhysics physics,
    ScrollContext context,
    ScrollPosition? oldPosition,
  );

  void dispose() {
    for (final ScrollPosition position in _positions) {
      position.removeListener(notifyListeners);
    }
    super.dispose();
  }
}
```

**Key Flutter Characteristics:**
- ScrollController extends `ChangeNotifier` (persistent, reusable across builds)
- Has `dispose()` method for cleanup
- Creates `ScrollPosition` objects (one per attached scrollable)
- Methods: `animateTo()`, `jumpTo()`, `attach()`, `detach()`
- Properties: `offset` (read-only), `position`, `hasClients`

**FLUI will follow the same pattern:**
- ✅ `ScrollController` persistent object in `flui_core` (Arc + ChangeNotifier)
- ✅ `ScrollPosition` struct tracking scroll state
- ✅ `ScrollPhysics` trait for customization
- ✅ Scrollable widgets in `flui_widgets` (ListView, ScrollView)
- ✅ Supports disposal pattern via `dispose()` method

#### Proposed API (Core Foundation Only)

**In `flui_core` - Persistent objects and scroll state logic:**

```rust
// crates/flui_core/src/foundation/scroll.rs
//
// IMPORTANT: ScrollController is a PERSISTENT OBJECT, not a widget!
// Similar to Flutter's ScrollController which extends ChangeNotifier.

use crate::foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// Scroll controller - programmatic scroll control
///
/// ScrollController is a **persistent object** (NOT a widget) that controls
/// scrollable widgets. It extends the Listenable pattern (via ChangeNotifier)
/// to notify listeners when scroll position changes.
///
/// Similar to Flutter's ScrollController - created once, reused across builds,
/// manages ScrollPosition objects for attached scrollables.
///
/// # Thread Safety
///
/// ScrollController is thread-safe and can be shared across threads using Arc.
/// All state is protected by Arc/Mutex.
///
/// # Lifecycle
///
/// Controllers should be disposed when no longer needed:
/// ```rust,ignore
/// // Create in widget state
/// let controller = ScrollController::new();
///
/// // Use across builds
/// ListView::new()
///     .controller(controller.clone())
///     .items(items)
///
/// // Dispose when widget unmounts
/// controller.dispose();
/// ```
///
/// # Example
///
/// ```rust,ignore
/// let controller = ScrollController::new();
///
/// // In widget
/// ScrollView::new()
///     .controller(controller.clone())
///     .child(content)
///
/// // Later: scroll to top
/// controller.jump_to(0.0);
///
/// // Or animate
/// controller.animate_to(0.0, Duration::from_millis(300));
///
/// // Cleanup when done
/// controller.dispose();
/// ```
#[derive(Debug, Clone)]
pub struct ScrollController {
    state: Arc<Mutex<ScrollControllerState>>,
    notifier: Arc<ChangeNotifier>,
}

#[derive(Debug)]
struct ScrollControllerState {
    position: ScrollPosition,
    initial_offset: f64,
}

impl ScrollController {
    /// Create a new scroll controller
    pub fn new() -> Self {
        Self::with_initial_offset(0.0)
    }

    /// Create with initial scroll offset
    pub fn with_initial_offset(offset: f64) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScrollControllerState {
                position: ScrollPosition {
                    pixels: offset,
                    min_scroll_extent: 0.0,
                    max_scroll_extent: 0.0,
                    viewport_dimension: 0.0,
                },
                initial_offset: offset,
            })),
            notifier: Arc::new(ChangeNotifier::new()),
        }
    }

    /// Get current scroll offset
    pub fn offset(&self) -> f64 {
        self.state.lock().position.pixels
    }

    /// Jump to offset immediately (no animation)
    pub fn jump_to(&self, offset: f64) {
        let mut state = self.state.lock();
        state.position.pixels = offset.clamp(
            state.position.min_scroll_extent,
            state.position.max_scroll_extent,
        );
        drop(state);

        self.notifier.notify_listeners();
    }

    /// Animate to offset
    ///
    /// TODO: This requires animation system to be implemented first
    pub fn animate_to(&self, _offset: f64, _duration: std::time::Duration) {
        todo!("Requires animation system")
    }

    /// Get current scroll position
    pub fn position(&self) -> ScrollPosition {
        self.state.lock().position
    }

    /// Update scroll position (called by scroll widget)
    pub(crate) fn update_position(&self, position: ScrollPosition) {
        self.state.lock().position = position;
        self.notifier.notify_listeners();
    }

    /// Add listener for scroll changes
    pub fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    /// Remove listener
    pub fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    /// Dispose the scroll controller
    ///
    /// Removes all listeners and cleans up resources. Should be called
    /// when the controller is no longer needed (e.g., widget unmounts).
    ///
    /// Similar to Flutter's `dispose()` method.
    pub fn dispose(&self) {
        // Clear all listeners
        // (actual ChangeNotifier::dispose() implementation)
        // Notifier cleanup happens via Arc drop
    }

    /// Check if controller has any attached scroll positions
    pub fn has_clients(&self) -> bool {
        // For now, assume always attached if controller exists
        // In full implementation, track attached scrollables
        true
    }
}

impl Default for ScrollController {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Listenable for ScrollController
impl Listenable for ScrollController {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }
}

/// Scroll position - current scroll state
///
/// Contains all information about the current scroll position,
/// including bounds and viewport size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollPosition {
    /// Current scroll offset in pixels
    pub pixels: f64,

    /// Minimum scroll extent (usually 0.0)
    pub min_scroll_extent: f64,

    /// Maximum scroll extent (content_size - viewport_size)
    pub max_scroll_extent: f64,

    /// Size of the viewport
    pub viewport_dimension: f64,
}

impl ScrollPosition {
    /// Create new scroll position
    pub const fn new(
        pixels: f64,
        min_scroll_extent: f64,
        max_scroll_extent: f64,
        viewport_dimension: f64,
    ) -> Self {
        Self {
            pixels,
            min_scroll_extent,
            max_scroll_extent,
            viewport_dimension,
        }
    }

    /// Check if at minimum extent (top/left)
    pub fn at_edge_start(&self) -> bool {
        self.pixels <= self.min_scroll_extent
    }

    /// Check if at maximum extent (bottom/right)
    pub fn at_edge_end(&self) -> bool {
        self.pixels >= self.max_scroll_extent
    }

    /// Check if out of range (overscroll)
    pub fn out_of_range(&self) -> bool {
        self.pixels < self.min_scroll_extent || self.pixels > self.max_scroll_extent
    }

    /// Get overscroll amount (0.0 if in range)
    pub fn overscroll(&self) -> f64 {
        if self.pixels < self.min_scroll_extent {
            self.pixels - self.min_scroll_extent
        } else if self.pixels > self.max_scroll_extent {
            self.pixels - self.max_scroll_extent
        } else {
            0.0
        }
    }

    /// Get scroll percentage (0.0 to 1.0)
    pub fn scroll_percentage(&self) -> f64 {
        if self.max_scroll_extent <= 0.0 {
            0.0
        } else {
            (self.pixels / self.max_scroll_extent).clamp(0.0, 1.0)
        }
    }
}

/// Scroll physics - defines scroll behavior
///
/// ScrollPhysics determines how scrollable widgets respond to user input
/// and scroll commands. Different platforms use different physics:
/// - iOS: BouncingScrollPhysics (rubber-band effect)
/// - Android: ClampingScrollPhysics (hard stop at edges)
///
/// Custom physics can be implemented by extending this trait.
pub trait ScrollPhysics: Send + Sync + fmt::Debug {
    /// Apply boundary conditions to proposed scroll delta
    ///
    /// This method is called when the user tries to scroll beyond
    /// the scroll bounds. Return the adjusted delta.
    ///
    /// # Arguments
    ///
    /// * `position` - Current scroll position
    /// * `delta` - Proposed scroll delta
    ///
    /// # Returns
    ///
    /// Adjusted delta to apply (may be less than proposed delta)
    fn apply_boundary_conditions(&self, position: &ScrollPosition, delta: f64) -> f64;

    /// Check if user offset should be accepted
    ///
    /// This method is called before applying user-initiated scroll.
    /// Return false to reject the scroll (e.g., if already at boundary).
    fn should_accept_user_offset(&self, position: &ScrollPosition) -> bool;

    /// Apply physics to velocity-based scrolling (fling)
    ///
    /// This method is called during scroll animations or fling gestures.
    /// Return the adjusted target position.
    fn apply_physics_to_user_offset(&self, position: &ScrollPosition, offset: f64) -> f64 {
        // Default: just clamp to bounds
        offset.clamp(position.min_scroll_extent, position.max_scroll_extent)
    }
}

/// Bouncing scroll physics (iOS-style)
///
/// Allows scrolling past boundaries with a rubber-band effect.
/// The further you scroll past the boundary, the more resistance.
#[derive(Debug, Clone, Copy)]
pub struct BouncingScrollPhysics;

impl ScrollPhysics for BouncingScrollPhysics {
    fn apply_boundary_conditions(&self, position: &ScrollPosition, delta: f64) -> f64 {
        // Allow overscroll but with resistance
        let new_pixels = position.pixels + delta;

        if new_pixels < position.min_scroll_extent {
            // Scrolling past top - apply resistance
            let overscroll = position.min_scroll_extent - new_pixels;
            let resistance = (overscroll / position.viewport_dimension).min(1.0);
            delta * (1.0 - resistance * 0.5)
        } else if new_pixels > position.max_scroll_extent {
            // Scrolling past bottom - apply resistance
            let overscroll = new_pixels - position.max_scroll_extent;
            let resistance = (overscroll / position.viewport_dimension).min(1.0);
            delta * (1.0 - resistance * 0.5)
        } else {
            // Within bounds - no resistance
            delta
        }
    }

    fn should_accept_user_offset(&self, _position: &ScrollPosition) -> bool {
        // Always accept user input (bouncing allows overscroll)
        true
    }
}

/// Clamping scroll physics (Android-style)
///
/// Hard stop at scroll boundaries. Does not allow overscroll.
#[derive(Debug, Clone, Copy)]
pub struct ClampingScrollPhysics;

impl ScrollPhysics for ClampingScrollPhysics {
    fn apply_boundary_conditions(&self, position: &ScrollPosition, delta: f64) -> f64 {
        // Clamp to bounds - no overscroll allowed
        let new_pixels = position.pixels + delta;
        let clamped = new_pixels.clamp(position.min_scroll_extent, position.max_scroll_extent);
        clamped - position.pixels
    }

    fn should_accept_user_offset(&self, position: &ScrollPosition) -> bool {
        // Reject input if already at boundary
        !position.out_of_range()
    }
}

/// Never scrollable physics
///
/// Rejects all scroll attempts. Useful for disabling scroll temporarily.
#[derive(Debug, Clone, Copy)]
pub struct NeverScrollableScrollPhysics;

impl ScrollPhysics for NeverScrollableScrollPhysics {
    fn apply_boundary_conditions(&self, _position: &ScrollPosition, _delta: f64) -> f64 {
        0.0  // Reject all scroll
    }

    fn should_accept_user_offset(&self, _position: &ScrollPosition) -> bool {
        false
    }
}

/// Always scrollable physics
///
/// Accepts all scroll without any physics. Useful for testing.
#[derive(Debug, Clone, Copy)]
pub struct AlwaysScrollableScrollPhysics;

impl ScrollPhysics for AlwaysScrollableScrollPhysics {
    fn apply_boundary_conditions(&self, _position: &ScrollPosition, delta: f64) -> f64 {
        delta  // Accept all scroll
    }

    fn should_accept_user_offset(&self, _position: &ScrollPosition) -> bool {
        true
    }
}
```

**That's it for core!** The rest is in `flui_widgets`.

#### Widget Implementation (in `flui_widgets`)

**Scrollable widgets (`flui_widgets/src/scrollable.rs`):**

```rust
/// ListView widget - scrollable list of items
///
/// Similar to Flutter's ListView. Accepts a ScrollController for
/// programmatic control.
pub struct ListView {
    /// Optional scroll controller
    controller: Option<Arc<ScrollController>>,

    /// Scroll physics
    physics: Option<Arc<dyn ScrollPhysics>>,

    /// List items
    children: Vec<AnyElement>,
}

impl ListView {
    pub fn new() -> Self {
        Self {
            controller: None,
            physics: None,
            children: Vec::new(),
        }
    }

    /// Set scroll controller
    pub fn controller(mut self, controller: Arc<ScrollController>) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Set scroll physics
    pub fn physics(mut self, physics: Arc<dyn ScrollPhysics>) -> Self {
        self.physics = Some(physics);
        self
    }

    /// Set children
    pub fn children(mut self, children: Vec<AnyElement>) -> Self {
        self.children = children;
        self
    }
}

impl View for ListView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create internal ScrollController if not provided
        let controller = self.controller.unwrap_or_else(|| {
            use_ref(ctx, || ScrollController::new())
        });

        // Use default physics if not provided
        let physics = self.physics.unwrap_or_else(|| {
            Arc::new(ClampingScrollPhysics)
        });

        // Return RenderScrollable (custom render object)
        (
            RenderScrollable::new(controller, physics),
            self.children
        )
    }
}

/// ScrollView widget - single scrollable child
///
/// Similar to Flutter's SingleChildScrollView.
pub struct ScrollView {
    controller: Option<Arc<ScrollController>>,
    physics: Option<Arc<dyn ScrollPhysics>>,
    child: AnyElement,
}

impl ScrollView {
    pub fn new() -> Self {
        Self {
            controller: None,
            physics: None,
            child: EmptyElement,
        }
    }

    pub fn controller(mut self, controller: Arc<ScrollController>) -> Self {
        self.controller = Some(controller);
        self
    }

    pub fn physics(mut self, physics: Arc<dyn ScrollPhysics>) -> Self {
        self.physics = Some(physics);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for ScrollView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let controller = self.controller.unwrap_or_else(|| {
            use_ref(ctx, || ScrollController::new())
        });

        let physics = self.physics.unwrap_or_else(|| {
            Arc::new(ClampingScrollPhysics)
        });

        (
            RenderScrollable::new(controller, physics),
            Some(self.child)
        )
    }
}
```

**RenderScrollable (`flui_rendering/src/scrollable.rs`):**

```rust
/// Render object for scrollable content
///
/// This is where scroll gestures are handled and ScrollController
/// is updated based on user input.
pub struct RenderScrollable {
    controller: Arc<ScrollController>,
    physics: Arc<dyn ScrollPhysics>,
    // ... scroll state
}

impl RenderScrollable {
    pub fn new(controller: Arc<ScrollController>, physics: Arc<dyn ScrollPhysics>) -> Self {
        Self {
            controller,
            physics,
        }
    }

    fn handle_scroll_gesture(&mut self, delta: f64) {
        // Get current position
        let position = self.controller.position();

        // Apply physics
        let adjusted_delta = self.physics.apply_boundary_conditions(&position, delta);

        // Update position
        let new_pixels = position.pixels + adjusted_delta;
        let new_position = ScrollPosition::new(
            new_pixels,
            position.min_scroll_extent,
            position.max_scroll_extent,
            position.viewport_dimension,
        );

        self.controller.update_position(new_position);
    }
}
```

#### Usage Examples

**Example 1: Basic ListView with controller**
```rust
struct MyView {
    scroll_controller: Arc<ScrollController>,
}

impl View for MyView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Column::new()
            .children(vec![
                Box::new(
                    Button::new("Scroll to Top")
                        .on_press({
                            let controller = self.scroll_controller.clone();
                            move || controller.jump_to(0.0)
                        })
                ),
                Box::new(
                    ListView::new()
                        .controller(self.scroll_controller.clone())
                        .children(
                            (0..100)
                                .map(|i| Box::new(Text::new(format!("Item {}", i))))
                                .collect()
                        )
                ),
            ])
    }
}
```

**Example 2: ScrollView with custom physics**
```rust
ScrollView::new()
    .controller(controller)
    .physics(Arc::new(BouncingScrollPhysics))  // iOS-style bounce
    .child(
        LongContent::new()
    )
```

**Example 3: Listen to scroll changes**
```rust
let controller = ScrollController::new();

// Add listener
controller.add_listener(|| {
    println!("Scrolled to: {}", controller.offset());
});

// Use in widget
ListView::new()
    .controller(controller.clone())
    .children(items)
```

#### BuildContext Integration

```rust
// In crates/flui_core/src/view/build_context.rs

impl BuildContext {
    /// Get scroll controller for an element (if it's scrollable)
    pub fn scroll_controller(&self, element_id: ElementId) -> Option<Arc<ScrollController>> {
        // Look up element and get its scroll controller
        todo!("Implement via Provider pattern in flui_widgets")
    }
}
```

---

### 2.4 Overlay System

**Priority:** ⭐⭐ IMPORTANT (P1)
**Estimated Size:** ~250 LOC in core + ~150 LOC in widgets
**Location:**
- Core: `crates/flui_core/src/foundation/overlay.rs` (persistent objects)
- Widgets: `crates/flui_widgets/src/overlay.rs` (Overlay/OverlayPortal widgets)

#### Problem Statement

No infrastructure for overlays (dialogs, tooltips, dropdowns) that need to:
- Render above normal content (Z-index)
- Be managed independently of widget tree
- Support stacking (multiple overlays)
- Allow programmatic show/hide

**Inspiration:** Flutter's hybrid approach - OverlayEntry (persistent object) + Overlay widget (lifecycle manager).

#### Architecture Decision: Hybrid Approach (Like Flutter)

**Flutter uses BOTH:**
1. **OverlayEntry** - Long-lived persistent object (like FocusNode, NOT a widget!)
2. **Overlay widget** - StatefulWidget that manages OverlayState and renders entries
3. **OverlayState** - Manages the stack of OverlayEntry objects

**From Flutter docs:**
> "OverlayEntry is a place in an Overlay that can contain a widget. It implements Listenable and allows dynamic insertion/removal from the overlay stack."

**Key Flutter Characteristics:**
- OverlayEntry implements `Listenable` (NOT Widget)
- Has `builder: WidgetBuilder` property (function that builds widget)
- Methods: `insert()`, `remove()`, `markNeedsBuild()`, `dispose()`
- Overlay is a **StatefulWidget** with OverlayState
- OverlayState manages stack via `insert(OverlayEntry)` and `remove(OverlayEntry)`

**Flutter Example:**
```dart
// OverlayEntry - persistent object (created once)
OverlayEntry entry = OverlayEntry(
  builder: (context) => Positioned(
    top: 100,
    left: 100,
    child: Material(
      child: Text('Overlay!'),
    ),
  ),
  opaque: false,
  maintainState: true,
);

// Insert programmatically via OverlayState
Overlay.of(context).insert(entry);

// Later: remove
entry.remove();  // or
entry.dispose();

// Rebuild overlay UI
entry.markNeedsBuild();
```

**FLUI will follow the same pattern:**
- ✅ `OverlayEntry` persistent object in `flui_core` (foundation)
- ✅ `Overlay` and `OverlayPortal` widgets in `flui_widgets`
- ✅ `OverlayState` manages entry stack (insert/remove)
- ✅ Supports both programmatic control and declarative usage

#### Proposed API (Core Foundation Only)

**In `flui_core` - Persistent objects and overlay stack logic:**

```rust
// crates/flui_core/src/foundation/overlay.rs
//
// IMPORTANT: OverlayEntry is a PERSISTENT OBJECT, not a widget!
// Similar to FocusNode - lives longer than widgets and implements Listenable.

use crate::foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use crate::view::AnyElement;
use parking_lot::Mutex;
use std::num::NonZeroU64;
use std::sync::Arc;

/// Overlay entry - represents a place in an Overlay that can contain a widget
///
/// OverlayEntry is a **persistent object** (NOT a widget) that holds a builder
/// function for creating overlay UI. It implements Listenable to notify when
/// the overlay needs to rebuild.
///
/// Similar to Flutter's OverlayEntry - created once, lives across rebuilds,
/// managed programmatically via insert/remove.
///
/// # Thread Safety
///
/// OverlayEntry is thread-safe and can be shared across threads using Arc.
/// The builder function and all state are protected by Arc/Mutex.
///
/// # Example
///
/// ```rust,ignore
/// // Create persistent OverlayEntry object
/// let entry = OverlayEntry::new(|_ctx| {
///     Positioned::new()
///         .top(100.0)
///         .left(100.0)
///         .child(
///             Container::new()
///                 .color(Color::rgba(0, 0, 0, 128))
///                 .child(Text::new("Tooltip!"))
///         )
/// });
///
/// // Insert via OverlayState
/// ctx.overlay().unwrap().insert(entry.clone());
///
/// // Later: remove
/// entry.remove();
///
/// // Or rebuild overlay UI
/// entry.mark_needs_build();
/// ```
#[derive(Clone)]
pub struct OverlayEntry {
    id: OverlayId,
    inner: Arc<OverlayEntryInner>,
}

struct OverlayEntryInner {
    /// Builder function that creates the overlay widget
    builder: Arc<dyn Fn() -> AnyElement + Send + Sync>,

    /// Whether this entry completely obscures entries below it
    opaque: bool,

    /// Whether to maintain state even when obscured by opaque entries above
    maintain_state: bool,

    /// Change notifier for rebuild notifications (Listenable pattern)
    notifier: Arc<ChangeNotifier>,

    /// Mutable state
    state: Mutex<OverlayEntryState>,
}

struct OverlayEntryState {
    /// Whether this entry is currently in the overlay tree
    mounted: bool,

    /// Reference to the OverlayState managing this entry
    overlay_state: Option<Arc<OverlayState>>,
}

impl OverlayEntry {
    /// Create new overlay entry with builder function
    ///
    /// The builder is called whenever the overlay needs to rebuild its UI.
    /// It should be cheap to call repeatedly.
    pub fn new(
        builder: impl Fn() -> AnyElement + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: OverlayId::new(),
            inner: Arc::new(OverlayEntryInner {
                builder: Arc::new(builder),
                opaque: false,
                maintain_state: false,
                notifier: Arc::new(ChangeNotifier::new()),
                state: Mutex::new(OverlayEntryState {
                    mounted: false,
                    overlay_state: None,
                }),
            }),
        })
    }

    /// Builder-style method to set opaque flag
    ///
    /// When true, this overlay completely obscures entries below it,
    /// allowing them to skip rendering for performance.
    pub fn with_opaque(self: Arc<Self>, opaque: bool) -> Arc<Self> {
        Arc::new(Self {
            id: self.id,
            inner: Arc::new(OverlayEntryInner {
                builder: self.inner.builder.clone(),
                opaque,
                maintain_state: self.inner.maintain_state,
                notifier: self.inner.notifier.clone(),
                state: Mutex::new(OverlayEntryState {
                    mounted: false,
                    overlay_state: None,
                }),
            }),
        })
    }

    /// Builder-style method to set maintain_state flag
    ///
    /// When true, the entry's state is preserved even when hidden
    /// by opaque entries above it.
    pub fn with_maintain_state(self: Arc<Self>, maintain: bool) -> Arc<Self> {
        Arc::new(Self {
            id: self.id,
            inner: Arc::new(OverlayEntryInner {
                builder: self.inner.builder.clone(),
                opaque: self.inner.opaque,
                maintain_state: maintain,
                notifier: self.inner.notifier.clone(),
                state: Mutex::new(OverlayEntryState {
                    mounted: false,
                    overlay_state: None,
                }),
            }),
        })
    }

    /// Get overlay ID
    pub fn id(&self) -> OverlayId {
        self.id
    }

    /// Build the overlay UI
    ///
    /// Calls the builder function to create the overlay widget tree.
    pub fn build(&self) -> AnyElement {
        (self.inner.builder)()
    }

    /// Whether this entry is opaque (blocks entries below)
    pub fn is_opaque(&self) -> bool {
        self.inner.opaque
    }

    /// Whether this entry maintains state when hidden
    pub fn maintains_state(&self) -> bool {
        self.inner.maintain_state
    }

    /// Remove this overlay from its OverlayState
    ///
    /// This is the primary way to dismiss an overlay. After calling remove(),
    /// the entry is no longer mounted and will not be rendered.
    pub fn remove(&self) {
        let state = self.inner.state.lock();
        if let Some(overlay_state) = &state.overlay_state {
            overlay_state.remove(self);
        }
    }

    /// Mark overlay as needing rebuild
    ///
    /// Call this when the overlay UI should be regenerated (e.g., because
    /// some state it depends on has changed). This notifies listeners.
    pub fn mark_needs_build(&self) {
        self.inner.notifier.notify_listeners();

        let state = self.inner.state.lock();
        if let Some(overlay_state) = &state.overlay_state {
            overlay_state.mark_needs_build(self.id);
        }
    }

    /// Check if this overlay is currently mounted in an OverlayState
    pub fn is_mounted(&self) -> bool {
        self.inner.state.lock().mounted
    }

    /// Dispose this overlay entry
    ///
    /// Removes the entry and cleans up resources. After dispose, the entry
    /// should not be used.
    pub fn dispose(&self) {
        self.remove();
        // Notifier cleanup happens via Arc drop
    }

    // Internal: Mark as mounted (called by OverlayState)
    pub(crate) fn mark_mounted(&self, overlay_state: Arc<OverlayState>) {
        let mut state = self.inner.state.lock();
        state.mounted = true;
        state.overlay_state = Some(overlay_state);
    }

    // Internal: Mark as unmounted (called by OverlayState)
    pub(crate) fn mark_unmounted(&self) {
        let mut state = self.inner.state.lock();
        state.mounted = false;
        state.overlay_state = None;
    }
}

// Implement Listenable for OverlayEntry (like Flutter)
impl Listenable for OverlayEntry {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.inner.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.inner.notifier.remove_listener(id)
    }
}

impl std::fmt::Debug for OverlayEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayEntry")
            .field("id", &self.id)
            .field("opaque", &self.inner.opaque)
            .field("maintain_state", &self.inner.maintain_state)
            .field("mounted", &self.is_mounted())
            .finish()
    }
}

/// Overlay state - manages the stack of OverlayEntry objects
///
/// OverlayState is the mutable state for the Overlay widget (which is a
/// StatefulWidget). It maintains the stack of OverlayEntry objects and
/// handles their insertion/removal.
///
/// Typically created by an Overlay widget, accessed via `Overlay.of(context)`.
#[derive(Debug)]
pub struct OverlayState {
    entries: Arc<Mutex<Vec<Arc<OverlayEntry>>>>,
}

impl OverlayState {
    /// Create new overlay state (called by Overlay widget)
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Insert an overlay entry at the top of the stack
    ///
    /// The entry will be rendered above all existing overlays.
    /// Call `entry.remove()` or `state.remove(entry)` to dismiss.
    pub fn insert(self: &Arc<Self>, entry: Arc<OverlayEntry>) {
        let mut entries = self.entries.lock();

        // Mark as mounted
        entry.mark_mounted(self.clone());

        entries.push(entry);
    }

    /// Insert multiple overlay entries at the top of the stack
    ///
    /// Entries are inserted in order (first entry is lowest).
    pub fn insert_all(self: &Arc<Self>, new_entries: Vec<Arc<OverlayEntry>>) {
        let mut entries = self.entries.lock();

        for entry in new_entries {
            entry.mark_mounted(self.clone());
            entries.push(entry);
        }
    }

    /// Remove an overlay entry from the stack
    pub fn remove(&self, entry: &OverlayEntry) {
        let mut entries = self.entries.lock();
        entries.retain(|e| e.id != entry.id);

        // Mark as unmounted
        entry.mark_unmounted();
    }

    /// Get all overlay entries (bottom to top)
    pub fn entries(&self) -> Vec<Arc<OverlayEntry>> {
        self.entries.lock().clone()
    }

    /// Mark an overlay as needing rebuild
    pub fn mark_needs_build(&self, id: OverlayId) {
        // TODO: Integrate with build pipeline
        // For now, just trigger a rebuild of the overlay widget
        tracing::debug!("Overlay {:?} needs rebuild", id);
    }

    /// Clear all overlays from the stack
    pub fn clear(&self) {
        let mut entries = self.entries.lock();
        for entry in entries.iter() {
            entry.mark_unmounted();
        }
        entries.clear();
    }
}

/// Overlay ID - unique identifier for an overlay entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(NonZeroU64);

impl OverlayId {
    /// Create new unique overlay ID
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(NonZeroU64::new(id).expect("Overlay ID overflow"))
    }
}
```

**That's it for core!** The rest is in `flui_widgets`.

#### Widget Implementation (in `flui_widgets`)

**Overlay widget (`flui_widgets/src/overlay.rs`):**

```rust
/// Overlay widget - manages a stack of OverlayEntry objects
///
/// Similar to Flutter's Overlay widget (StatefulWidget). The Overlay widget
/// creates and manages an OverlayState, which renders all inserted entries
/// as a Stack.
///
/// Typically used by Navigator, but can be used directly for custom overlay needs.
pub struct Overlay {
    /// Initial entries to display (optional)
    initial_entries: Vec<Arc<OverlayEntry>>,
}

impl Overlay {
    /// Create new Overlay widget
    pub fn new() -> Self {
        Self {
            initial_entries: Vec::new(),
        }
    }

    /// Set initial overlay entries
    pub fn with_initial_entries(mut self, entries: Vec<Arc<OverlayEntry>>) -> Self {
        self.initial_entries = entries;
        self
    }
}

impl View for Overlay {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create OverlayState (persistent across rebuilds via use_ref hook)
        let state = use_ref(ctx, || OverlayState::new());

        // Insert initial entries on first build
        use_effect(ctx, {
            let state = state.clone();
            let entries = self.initial_entries;
            move || {
                if !entries.is_empty() {
                    state.insert_all(entries);
                }
                None // No cleanup
            }
        });

        // Render all entries as Stack
        // (OverlayStack is a custom render object that renders OverlayEntry objects)
        OverlayStack::new(state.clone())
    }
}

/// OverlayPortal widget - declarative overlay management
///
/// Similar to Flutter's OverlayPortal. OverlayPortal allows declarative
/// overlay management without manually creating OverlayEntry objects.
///
/// # Example
///
/// ```rust,ignore
/// OverlayPortal::new()
///     .overlay_child_builder(|_ctx| {
///         Positioned::new()
///             .top(100.0)
///             .left(100.0)
///             .child(Tooltip::new("Hello!"))
///     })
///     .child(Button::new("Hover me"))
/// ```
pub struct OverlayPortal {
    /// Whether to show the overlay
    showing: bool,

    /// Builder for overlay content
    overlay_child_builder: Option<Box<dyn Fn() -> AnyElement + Send + Sync>>,

    /// Main child widget
    child: AnyElement,
}

impl OverlayPortal {
    pub fn new() -> Self {
        Self {
            showing: false,
            overlay_child_builder: None,
            child: EmptyElement,
        }
    }

    /// Set whether overlay is showing
    pub fn showing(mut self, showing: bool) -> Self {
        self.showing = showing;
        self
    }

    /// Set overlay content builder
    pub fn overlay_child_builder(
        mut self,
        builder: impl Fn() -> AnyElement + Send + Sync + 'static,
    ) -> Self {
        self.overlay_child_builder = Some(Box::new(builder));
        self
    }

    /// Set main child
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into_element();
        self
    }
}

impl View for OverlayPortal {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create persistent OverlayEntry (survives rebuilds)
        let entry = use_ref(ctx, {
            let builder = self.overlay_child_builder.clone();
            move || {
                OverlayEntry::new(move || {
                    if let Some(ref builder) = builder {
                        builder()
                    } else {
                        EmptyElement
                    }
                })
            }
        });

        // Insert/remove based on showing flag
        use_effect(ctx, {
            let entry = entry.clone();
            let showing = self.showing;
            let overlay_state = ctx.overlay();

            move || {
                if showing {
                    if let Some(overlay) = overlay_state {
                        if !entry.is_mounted() {
                            overlay.insert(entry.clone());
                        }
                    }
                } else {
                    entry.remove();
                }

                // Cleanup: remove on unmount
                Some(move || {
                    entry.remove();
                })
            }
        });

        // Return main child
        self.child
    }
}
```

#### Usage Examples

**Example 1: Manual OverlayEntry (programmatic control)**
```rust
// Create persistent OverlayEntry
let tooltip_entry = OverlayEntry::new(|| {
    Positioned::new()
        .top(100.0)
        .left(100.0)
        .child(
            Container::new()
                .padding(EdgeInsets::all(8.0))
                .color(Color::BLACK.with_opacity(0.8))
                .child(Text::new("Tooltip!").color(Color::WHITE))
        )
});

// Show tooltip
struct MyView {
    tooltip_entry: Arc<OverlayEntry>,
}

impl View for MyView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Button::new("Show Tooltip")
            .on_press({
                let entry = self.tooltip_entry.clone();
                let overlay = ctx.overlay().unwrap();
                move || overlay.insert(entry.clone())
            })
    }
}
```

**Example 2: OverlayPortal (declarative)**
```rust
// Declarative overlay - no manual OverlayEntry
struct TooltipButton {
    showing_tooltip: Arc<Signal<bool>>,
}

impl View for TooltipButton {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let showing = self.showing_tooltip.get();

        OverlayPortal::new()
            .showing(showing)
            .overlay_child_builder(|| {
                Positioned::new()
                    .top(100.0)
                    .left(100.0)
                    .child(Tooltip::new("Hello!"))
            })
            .child(
                Button::new("Hover me")
                    .on_hover({
                        let signal = self.showing_tooltip.clone();
                        move |hovering| signal.set(hovering)
                    })
            )
    }
}
```

**Example 3: Overlay widget with initial entries (Navigator pattern)**
```rust
// Used internally by Navigator to manage routes
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let initial_routes = vec![
            OverlayEntry::new(|| HomePage::new()),
        ];

        Overlay::new()
            .with_initial_entries(initial_routes)
    }
}
```

#### BuildContext Integration

```rust
// In crates/flui_core/src/view/build_context.rs

impl BuildContext {
    /// Get the nearest overlay state
    pub fn overlay(&self) -> Option<Arc<OverlayState>> {
        // Walk up element tree to find Overlay widget
        todo!("Implement overlay state lookup")
    }

    /// Show an overlay
    pub fn show_overlay(&self, entry: Arc<OverlayEntry>) {
        if let Some(overlay) = self.overlay() {
            overlay.insert(entry);
        }
    }

    /// Hide an overlay
    pub fn hide_overlay(&self, entry: &OverlayEntry) {
        if let Some(overlay) = self.overlay() {
            overlay.remove(entry);
        }
    }
}
```

---

### 2.5 Platform Integration Traits

**Priority:** ⭐ NICE TO HAVE (P2)
**Estimated Size:** ~200 LOC
**Location:** `crates/flui_core/src/foundation/platform.rs`

#### Problem Statement

No abstraction for platform-specific operations:
- Clipboard access
- URL opening
- Locale/language
- Light/dark mode detection
- Platform-specific behaviors

#### Proposed API

```rust
// crates/flui_core/src/foundation/platform.rs

use crate::foundation::CoreError;
use once_cell::sync::OnceCell;
use std::fmt;
use std::sync::Arc;

/// Platform dispatcher - OS integration abstraction
///
/// Trait for platform-specific operations. Implementations are provided
/// by platform-specific crates (e.g., flui_platform_windows, flui_platform_web).
///
/// # Example
///
/// ```rust,ignore
/// // Set platform at startup
/// set_platform(Arc::new(WindowsPlatform::new()));
///
/// // Use in widget
/// let text = ctx.platform().clipboard_get_text()?;
/// ctx.platform().open_url("https://flui.rs")?;
/// ```
pub trait PlatformDispatcher: Send + Sync + fmt::Debug {
    /// Get clipboard text
    fn clipboard_get_text(&self) -> Result<String, PlatformError>;

    /// Set clipboard text
    fn clipboard_set_text(&self, text: &str) -> Result<(), PlatformError>;

    /// Open URL in default browser
    fn open_url(&self, url: &str) -> Result<(), PlatformError>;

    /// Get current locale (e.g., "en-US", "ja-JP")
    fn locale(&self) -> String;

    /// Get platform brightness (light/dark mode)
    fn brightness(&self) -> Brightness;

    /// Get platform type
    fn platform_type(&self) -> PlatformType;
}

/// Platform brightness (light/dark mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brightness {
    /// Light mode
    Light,
    /// Dark mode
    Dark,
}

/// Platform type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformType {
    /// Windows
    Windows,
    /// macOS
    MacOS,
    /// Linux
    Linux,
    /// Web (WASM)
    Web,
    /// Android
    Android,
    /// iOS
    iOS,
}

/// Platform error
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// Clipboard operation failed
    #[error("Clipboard error: {0}")]
    Clipboard(String),

    /// URL opening failed
    #[error("Failed to open URL: {0}")]
    UrlOpen(String),

    /// Not supported on this platform
    #[error("Operation not supported on this platform")]
    NotSupported,

    /// Other error
    #[error("Platform error: {0}")]
    Other(String),
}

/// Global platform instance
static PLATFORM: OnceCell<Arc<dyn PlatformDispatcher>> = OnceCell::new();

/// Get global platform dispatcher
pub fn platform() -> Option<&'static Arc<dyn PlatformDispatcher>> {
    PLATFORM.get()
}

/// Set global platform dispatcher (can only be called once)
pub fn set_platform(dispatcher: Arc<dyn PlatformDispatcher>) -> Result<(), CoreError> {
    PLATFORM
        .set(dispatcher)
        .map_err(|_| CoreError::AlreadyInitialized)
}

/// Stub platform implementation for testing
#[derive(Debug)]
pub struct StubPlatform {
    locale: String,
    brightness: Brightness,
    platform_type: PlatformType,
}

impl StubPlatform {
    /// Create stub platform with defaults
    pub fn new() -> Self {
        Self {
            locale: "en-US".to_string(),
            brightness: Brightness::Light,
            platform_type: PlatformType::Linux,
        }
    }
}

impl Default for StubPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformDispatcher for StubPlatform {
    fn clipboard_get_text(&self) -> Result<String, PlatformError> {
        Ok(String::new())
    }

    fn clipboard_set_text(&self, _text: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn open_url(&self, _url: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn locale(&self) -> String {
        self.locale.clone()
    }

    fn brightness(&self) -> Brightness {
        self.brightness
    }

    fn platform_type(&self) -> PlatformType {
        self.platform_type
    }
}
```

#### BuildContext Integration

```rust
// In crates/flui_core/src/view/build_context.rs

impl BuildContext {
    /// Get platform dispatcher
    pub fn platform(&self) -> Option<&'static Arc<dyn PlatformDispatcher>> {
        foundation::platform::platform()
    }

    /// Get clipboard (convenience method)
    pub fn clipboard_text(&self) -> Result<String, PlatformError> {
        self.platform()
            .ok_or(PlatformError::NotSupported)?
            .clipboard_get_text()
    }

    /// Set clipboard (convenience method)
    pub fn set_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        self.platform()
            .ok_or(PlatformError::NotSupported)?
            .clipboard_set_text(text)
    }

    /// Get current locale
    pub fn locale(&self) -> String {
        self.platform()
            .map(|p| p.locale())
            .unwrap_or_else(|| "en-US".to_string())
    }

    /// Get platform brightness (light/dark mode)
    pub fn brightness(&self) -> Brightness {
        self.platform()
            .map(|p| p.brightness())
            .unwrap_or(Brightness::Light)
    }
}
```

---

## 3. Implementation Plan

### Phase 1: Critical Infrastructure (Week 1-2)

**Goal:** Enable keyboard navigation and type-safe action foundation

1. **Focus Management Foundation** (~400 LOC in core, 2-3 days) **[CORE ONLY]**
   - Create `foundation/focus.rs` (persistent objects)
   - Implement FocusNode (Arc-based, like RenderObject)
   - Implement FocusManager (singleton)
   - Add focus tree logic (attach/detach/reparent)
   - Add BuildContext stubs (focus_node, request_focus, unfocus)
   - Write basic tests (node creation, focus request, manager)
   - **Note:** Actual `Focus` and `FocusScope` widgets go in `flui_widgets` later

2. **Action Foundation** (~150 LOC, 1-2 days) **[CORE ONLY]**
   - Create `foundation/action.rs`
   - Implement Action trait, ActionHandler trait, ActionContext
   - Add common actions (Copy, Paste, Cut, SelectAll, Undo, Redo)
   - Add BuildContext stubs (find_action_handler, dispatch_action)
   - Write basic tests
   - **Note:** Actual `Shortcuts` and `Actions` widgets go in `flui_widgets` later

**Deliverable:** FocusNode persistent objects work, Action foundation ready, both ready for widgets

### Phase 2: Scrolling Infrastructure (Week 3)

**Goal:** Enable programmatic scroll control

3. **Scroll Controller Foundation** (~350 LOC in core, 3-4 days) **[CORE ONLY]**
   - Create `foundation/scroll.rs` (persistent objects)
   - Implement ScrollController (Arc-based, extends Listenable via ChangeNotifier)
   - Implement ScrollPosition (scroll state struct)
   - Implement ScrollPhysics trait (Bouncing, Clamping, Never, Always)
   - Add BuildContext stub (scroll_controller)
   - Add dispose() method for cleanup
   - Write basic tests (controller creation, jump_to, position tracking, physics)
   - **Note:** Actual `ListView` and `ScrollView` widgets go in `flui_widgets` later
   - **Note:** Actual `RenderScrollable` render object goes in `flui_rendering` later

**Deliverable:** ScrollController persistent objects work, ready for widgets and render objects

### Phase 3: Overlays (Week 4)

**Goal:** Enable dialogs, tooltips, dropdowns

4. **Overlay System Foundation** (~250 LOC in core, 2-3 days) **[CORE ONLY]**
   - Create `foundation/overlay.rs` (persistent objects)
   - Implement OverlayEntry (Arc-based, implements Listenable)
   - Implement OverlayState (manages entry stack)
   - Implement OverlayId (unique identifiers)
   - Add BuildContext stubs (overlay, show_overlay, hide_overlay)
   - Write basic tests (entry creation, insert, remove, stacking)
   - **Note:** Actual `Overlay` and `OverlayPortal` widgets go in `flui_widgets` later

**Deliverable:** Overlays can be shown/hidden programmatically

### Phase 4: Platform Integration (Week 5)

**Goal:** Enable platform-specific features

5. **Platform Traits** (~200 LOC, 2 days)
   - Create `foundation/platform.rs`
   - Implement PlatformDispatcher trait
   - Add StubPlatform for testing
   - Add BuildContext integration
   - Write tests (clipboard, locale, brightness)
   - Update CLAUDE.md with platform patterns

**Deliverable:** Clipboard and platform info accessible

### Phase 5: Documentation & Examples (Week 6)

6. **Update Documentation**
   - Update `CLAUDE.md` with all new features
   - Add examples for each feature
   - Update `API_GUIDE.md`
   - Create migration guide

7. **Integration Examples**
   - Example: Keyboard shortcuts in app
   - Example: Scroll to item in ListView
   - Example: Show dialog overlay
   - Example: Copy/paste with clipboard

**Deliverable:** Complete documentation and working examples

---

## 4. Testing Strategy

### Unit Tests

Each module must have comprehensive unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_node_creation() { ... }

    #[test]
    fn test_focus_request() { ... }

    #[test]
    fn test_focus_traversal() { ... }

    #[test]
    fn test_action_dispatch() { ... }

    #[test]
    fn test_scroll_controller_jump() { ... }

    #[test]
    fn test_overlay_stacking() { ... }
}
```

### Integration Tests

Test interaction between systems:

```rust
#[test]
fn test_focus_with_overlay() {
    // Create overlay with focusable content
    // Verify focus moves to overlay
    // Close overlay
    // Verify focus returns to previous element
}

#[test]
fn test_action_with_focus() {
    // Focus a TextField
    // Dispatch Copy action
    // Verify clipboard contains text
}

#[test]
fn test_scroll_with_notifications() {
    // Create scroll controller
    // Scroll programmatically
    // Verify ScrollNotification dispatched
}
```

### Example-Based Testing

Create runnable examples that demonstrate each feature:

```rust
// examples/focus_navigation.rs
// examples/keyboard_shortcuts.rs
// examples/scroll_control.rs
// examples/dialog_overlay.rs
```

---

## 5. Success Metrics

### Functionality Metrics

- ✅ Keyboard navigation works (Tab, Shift+Tab, Arrow keys)
- ✅ Keyboard shortcuts work (Ctrl+C, Ctrl+V, etc.)
- ✅ Programmatic scrolling works (jump_to, animate_to)
- ✅ Overlays can be shown/hidden programmatically
- ✅ Clipboard operations work (copy, paste)
- ✅ Platform info accessible (locale, brightness)

### Code Quality Metrics

- ✅ All tests pass (`cargo test --workspace`)
- ✅ No warnings (`cargo clippy --workspace -- -D warnings`)
- ✅ Code formatted (`cargo fmt --all`)
- ✅ Documentation complete (all pub items documented)
- ✅ Examples run successfully

### API Quality Metrics

- ✅ API is consistent with existing FLUI patterns
- ✅ Thread-safe (uses Arc/Mutex where needed)
- ✅ No unnecessary allocations (prefer stack where possible)
- ✅ Clear error messages (use tracing for debug info)
- ✅ Follows Rust idioms (builder pattern, trait objects, etc.)

---

## 6. Risks & Mitigations

### Risk: Animation System Dependency

**Problem:** ScrollController.animate_to() requires animation system

**Mitigation:**
- Implement jump_to() first (no animation needed)
- Mark animate_to() as `todo!()` with clear error message
- Document that animation requires separate animation crate
- Implement animation system in Phase 2 of overall project

### Risk: BuildContext Access

**Problem:** Some features need element tree access

**Mitigation:**
- Store weak references to element tree in BuildContext
- Use Arc/Weak pattern to avoid cycles
- Document lifetime constraints clearly
- Add helper methods to BuildContext for common operations

### Risk: Platform Trait Implementation

**Problem:** Need platform-specific implementations

**Mitigation:**
- Provide StubPlatform for testing
- Document how to implement PlatformDispatcher
- Create example implementations for Windows/Linux
- Consider making platform crate optional (feature flag)

### Risk: Thread Safety

**Problem:** All features must be thread-safe

**Mitigation:**
- Use Arc/Mutex consistently
- Use parking_lot for performance
- Test with `cargo test -- --test-threads=16`
- Document thread-safety guarantees

---

## 7. Future Enhancements

### Not in Initial Scope (Post-MVP)

**Animation System** (separate crate `flui_animation`):
- AnimationController
- Tween system
- Curve library
- AnimatedWidget base

**Navigation System** (in `flui_widgets`):
- Navigator widget
- Route management
- Deep linking
- Named routes

**Focus Widgets** (in `flui_widgets`):
- `Focus` widget (manages FocusNode lifecycle)
- `FocusScope` widget (groups focus nodes)
- `FocusTraversalGroup` widget (custom Tab order)
- `FocusProvider` (Provider-based node lookup)

**Actions/Shortcuts Widgets** (in `flui_widgets`):
- `Shortcuts` widget (keyboard shortcut → Action mapping)
- `Actions` widget (Action → Handler registration)
- `ActionProvider` (Provider-based handler lookup)
- Integration with Focus system

**Overlay Widgets** (in `flui_widgets`):
- `Overlay` widget (manages OverlayState and renders entries)
- `OverlayPortal` widget (declarative overlay management)
- `OverlayStack` render object (renders OverlayEntry stack)
- Integration with Navigator for route management

**Scrollable Widgets** (in `flui_widgets` + `flui_rendering`):
- `ListView` widget (scrollable list with ScrollController)
- `ScrollView` widget (single scrollable child)
- `GridView` widget (scrollable grid)
- `RenderScrollable` render object (handles scroll gestures in flui_rendering)
- Integration with gesture system

**Gesture Recognition** (expand `flui_gestures`):
- Complete gesture arena
- Drag recognizers
- Scale recognizers
- Rotation recognizers

**Form Management** (in `flui_widgets`):
- Form widget
- FormField trait
- Validation system
- Input formatters

**Accessibility** (separate crate `flui_a11y`):
- Semantics tree
- Screen reader support
- High contrast mode
- Font scaling

---

## 8. Conclusion

This roadmap provides a clear path to completing `flui_core` with essential infrastructure features. The focus is on **core-level primitives** that enable higher-level features in other crates.

**Total effort:** ~5 weeks for a single developer (core only)
**Total LOC:** ~1,400 LOC in core (Focus: 400, Actions: 150, Scroll: 350, Overlay: 250, Platform: 200, BuildContext: ~50)

**Key Benefits:**
1. ✅ Complete feature parity with Flutter/GPUI for core infrastructure
2. ✅ Enables keyboard navigation and accessibility
3. ✅ Provides programmatic control (scroll, focus, overlay)
4. ✅ Maintains FLUI's thread-safe architecture
5. ✅ Clean API consistent with existing patterns

**Next Steps:**
1. Review and approve this roadmap
2. Create GitHub issues for each phase
3. Start Phase 1: Focus Management
4. Iterate based on feedback

---

## Appendix A: Related Documents

- `CLAUDE.md` - Project guidelines and conventions
- `docs/FINAL_ARCHITECTURE_V2.md` - Overall architecture
- `docs/PIPELINE_ARCHITECTURE.md` - Pipeline design
- `docs/API_GUIDE.md` - Comprehensive API guide
- `crates/flui_core/src/hooks/RULES.md` - Hook usage rules

## Appendix B: Glossary

- **Action:** Type-safe command that can be invoked via shortcuts or programmatically (trait in core)
- **Focus Node:** Persistent object representing a focusable element (like RenderObject, NOT a widget)
- **Focus Scope:** Manages focus within a subtree (e.g., dialog, form) via FocusScope widget
- **OverlayEntry:** Persistent object representing a place in an Overlay stack (implements Listenable)
- **OverlayState:** Manages the stack of OverlayEntry objects (insert/remove operations)
- **Overlay Widget:** StatefulWidget that manages OverlayState and renders entries as Stack
- **ScrollController:** Persistent object that controls scrollable widgets (extends ChangeNotifier/Listenable)
- **ScrollPosition:** Struct tracking scroll state (pixels, min/max extents, viewport dimension)
- **ScrollPhysics:** Trait defining scroll behavior (bouncing, clamping, etc.)
- **Platform Dispatcher:** Trait for OS-specific operations (clipboard, locale, etc.)

## Appendix C: Version History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 0.1.0 | 2025-11-10 | Claude | Initial roadmap |
| 0.1.1 | 2025-11-10 | Claude | Updated Actions to widget-based architecture (like Flutter) |
| 0.1.2 | 2025-11-10 | Claude | Updated Focus to hybrid approach (persistent FocusNode + Focus widget) |
| 0.1.3 | 2025-11-10 | Claude | Updated Overlay to hybrid approach (persistent OverlayEntry + Overlay widget) |
| 0.1.4 | 2025-11-10 | Claude | Updated Scroll to persistent object pattern (ScrollController + widgets/rendering) |

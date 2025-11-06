# Keyboard Events Architecture

## Problem: Keyboard ≠ Pointer

Keyboard events are fundamentally different from pointer events:

| Aspect | Pointer Events | Keyboard Events |
|--------|---------------|-----------------|
| **Targeting** | Spatial (where clicked) | Focus-based (which widget has focus) |
| **Hit Testing** | Required | Not applicable |
| **Widget** | GestureDetector | Focus + KeyboardListener |
| **Global State** | None needed | FocusManager tracks focused widget |

## Flutter's Keyboard System

### 1. FocusNode - Focus Management

```dart
class MyWidget extends StatefulWidget {
  @override
  State<MyWidget> createState() => _MyWidgetState();
}

class _MyWidgetState extends State<MyWidget> {
  final FocusNode _focusNode = FocusNode();

  @override
  void dispose() {
    _focusNode.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      focusNode: _focusNode,
      onKey: (node, event) {
        if (event.logicalKey == LogicalKeyboardKey.enter) {
          print('Enter pressed!');
          return KeyEventResult.handled;
        }
        return KeyEventResult.ignored;
      },
      child: TextField(),
    );
  }
}
```

### 2. Shortcuts - Hotkey Registration

```dart
Shortcuts(
  shortcuts: {
    // Ctrl+S → Save
    LogicalKeySet(LogicalKeyboardKey.control, LogicalKeyboardKey.keyS): SaveIntent(),
    // Ctrl+Z → Undo
    LogicalKeySet(LogicalKeyboardKey.control, LogicalKeyboardKey.keyZ): UndoIntent(),
    // F5 → Refresh
    LogicalKeySet(LogicalKeyboardKey.f5): RefreshIntent(),
  },
  child: Actions(
    actions: {
      SaveIntent: CallbackAction(onInvoke: (_) => save()),
      UndoIntent: CallbackAction(onInvoke: (_) => undo()),
      RefreshIntent: CallbackAction(onInvoke: (_) => refresh()),
    },
    child: MyApp(),
  ),
)
```

### 3. KeyboardListener - Low-level Events

```dart
KeyboardListener(
  focusNode: _focusNode,
  autofocus: true,
  onKeyEvent: (KeyEvent event) {
    if (event is KeyDownEvent) {
      print('Key down: ${event.logicalKey}');
    }
    if (event is KeyUpEvent) {
      print('Key up: ${event.logicalKey}');
    }
  },
  child: Container(...),
)
```

## FLUI Architecture

### Component Layout

```
crates/
├── flui_types/
│   └── src/
│       └── events.rs                    ✅ KeyEvent already defined
│
├── flui_engine/
│   └── src/
│       └── event_router.rs              ✅ Routes Key events to focused layer
│
├── flui_widgets/
│   └── src/
│       ├── focus/                       ⏳ NEW MODULE
│       │   ├── mod.rs
│       │   ├── focus_manager.rs        → Global focus state
│       │   ├── focus_node.rs           → Focus handle for widgets
│       │   └── focus_scope.rs          → Focus scope boundaries
│       │
│       └── interaction/                 ⏳ EXTEND
│           ├── focus.rs                → Focus widget
│           ├── keyboard_listener.rs    → KeyboardListener widget
│           ├── shortcuts.rs            → Shortcuts widget
│           └── actions.rs              → Actions widget
```

### 1. FocusManager (Global Singleton)

```rust
// crates/flui_widgets/src/focus/focus_manager.rs

use parking_lot::RwLock;
use std::sync::Arc;

/// Global focus manager
///
/// Tracks which widget currently has keyboard focus.
pub struct FocusManager {
    /// Currently focused node
    focused: Arc<RwLock<Option<FocusNodeId>>>,
}

impl FocusManager {
    /// Get the global focus manager
    pub fn global() -> &'static FocusManager {
        static INSTANCE: once_cell::sync::Lazy<FocusManager> =
            once_cell::sync::Lazy::new(|| FocusManager {
                focused: Arc::new(RwLock::new(None)),
            });
        &INSTANCE
    }

    /// Request focus for a node
    pub fn request_focus(&self, node_id: FocusNodeId) {
        *self.focused.write() = Some(node_id);
    }

    /// Get currently focused node
    pub fn focused(&self) -> Option<FocusNodeId> {
        *self.focused.read()
    }

    /// Clear focus
    pub fn unfocus(&self) {
        *self.focused.write() = None;
    }
}
```

### 2. FocusNode

```rust
// crates/flui_widgets/src/focus/focus_node.rs

use std::sync::Arc;
use flui_types::events::KeyEvent;

pub type KeyEventCallback = Arc<dyn Fn(&KeyEvent) -> KeyEventResult + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusNodeId(u64);

/// Result of key event handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventResult {
    /// Event was handled
    Handled,
    /// Event was ignored, continue propagation
    Ignored,
}

/// Focus node for a widget
#[derive(Clone)]
pub struct FocusNode {
    id: FocusNodeId,
    on_key: Option<KeyEventCallback>,
}

impl FocusNode {
    /// Create new focus node
    pub fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: FocusNodeId(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
            on_key: None,
        }
    }

    /// Set key event callback
    pub fn with_on_key<F>(mut self, callback: F) -> Self
    where
        F: Fn(&KeyEvent) -> KeyEventResult + Send + Sync + 'static,
    {
        self.on_key = Some(Arc::new(callback));
        self
    }

    /// Request focus for this node
    pub fn request_focus(&self) {
        FocusManager::global().request_focus(self.id);
    }

    /// Remove focus from this node
    pub fn unfocus(&self) {
        if FocusManager::global().focused() == Some(self.id) {
            FocusManager::global().unfocus();
        }
    }

    /// Check if this node has focus
    pub fn has_focus(&self) -> bool {
        FocusManager::global().focused() == Some(self.id)
    }

    /// Handle key event
    pub fn handle_key(&self, event: &KeyEvent) -> KeyEventResult {
        if let Some(callback) = &self.on_key {
            callback(event)
        } else {
            KeyEventResult::Ignored
        }
    }

    /// Get node ID
    pub fn id(&self) -> FocusNodeId {
        self.id
    }
}
```

### 3. Focus Widget

```rust
// crates/flui_widgets/src/interaction/focus.rs

use flui_core::view::{AnyView, BuildContext, IntoElement, View};
use crate::focus::{FocusNode, KeyEventResult};
use flui_types::events::KeyEvent;

/// Focus widget - makes child focusable
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::{Focus, FocusNode};
/// use flui_types::events::PhysicalKey;
///
/// let focus_node = FocusNode::new()
///     .with_on_key(|event| {
///         if event.physical_key() == PhysicalKey::Enter {
///             println!("Enter pressed!");
///             KeyEventResult::Handled
///         } else {
///             KeyEventResult::Ignored
///         }
///     });
///
/// Focus::new(focus_node)
///     .autofocus(true)
///     .child(TextField::new())
///     .build()
/// ```
#[derive(Clone)]
pub struct Focus {
    focus_node: FocusNode,
    autofocus: bool,
    child: Box<dyn AnyView>,
}

impl Focus {
    pub fn new(focus_node: FocusNode) -> FocusBuilder {
        FocusBuilder {
            focus_node,
            autofocus: false,
            child: None,
        }
    }
}

pub struct FocusBuilder {
    focus_node: FocusNode,
    autofocus: bool,
    child: Option<Box<dyn AnyView>>,
}

impl FocusBuilder {
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    pub fn child(mut self, child: impl View + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    pub fn build(self) -> Focus {
        Focus {
            focus_node: self.focus_node,
            autofocus: self.autofocus,
            child: self.child.expect("Focus requires a child"),
        }
    }
}

impl View for Focus {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Request focus if autofocus is true
        if self.autofocus {
            self.focus_node.request_focus();
        }

        // TODO: Create RenderFocus that registers with FocusManager
        // For now, just return child
        self.child
    }
}
```

### 4. Shortcuts Widget

```rust
// crates/flui_widgets/src/interaction/shortcuts.rs

use std::collections::HashMap;
use flui_types::events::{PhysicalKey, KeyModifiers};

/// Keyboard shortcut
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySet {
    pub key: PhysicalKey,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl KeySet {
    pub fn new(key: PhysicalKey) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn meta(mut self) -> Self {
        self.meta = true;
        self
    }
}

/// Intent triggered by keyboard shortcut
pub trait Intent: Send + Sync + 'static {
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Shortcuts widget - maps keyboard shortcuts to intents
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::{Shortcuts, KeySet};
/// use flui_types::events::PhysicalKey;
///
/// #[derive(Clone)]
/// struct SaveIntent;
/// impl Intent for SaveIntent {}
///
/// Shortcuts::new()
///     .add(KeySet::new(PhysicalKey::KeyS).ctrl(), SaveIntent)
///     .add(KeySet::new(PhysicalKey::F5), RefreshIntent)
///     .child(MyApp)
///     .build()
/// ```
pub struct Shortcuts {
    shortcuts: HashMap<KeySet, Box<dyn Intent>>,
    child: Box<dyn AnyView>,
}

// Implementation similar to Focus widget
```

### 5. EventRouter Integration

EventRouter already routes keyboard events:

```rust
// crates/flui_engine/src/event_router.rs (ALREADY EXISTS!)

match event {
    Event::Key(key_event) => {
        // Keyboard events go to focused layer
        if let Some(focused_id) = FocusManager::global().focused() {
            // Find layer with this focus node and dispatch
            // ...
        } else {
            // No focus, send to root
            root.handle_event(event)
        }
    }
}
```

## Usage Examples

### Example 1: Simple Keyboard Listener

```rust
use flui_widgets::{Focus, FocusNode, TextField};
use flui_types::events::PhysicalKey;

let focus_node = FocusNode::new()
    .with_on_key(|event| {
        match event.physical_key() {
            PhysicalKey::Enter => {
                println!("Submit!");
                KeyEventResult::Handled
            }
            PhysicalKey::Escape => {
                println!("Cancel!");
                KeyEventResult::Handled
            }
            _ => KeyEventResult::Ignored,
        }
    });

Focus::new(focus_node)
    .autofocus(true)
    .child(TextField::new())
    .build()
```

### Example 2: Global Hotkeys

```rust
use flui_widgets::{Shortcuts, Actions, KeySet};
use flui_types::events::PhysicalKey;

#[derive(Clone)]
struct SaveIntent;
impl Intent for SaveIntent {}

#[derive(Clone)]
struct UndoIntent;
impl Intent for UndoIntent {}

Shortcuts::new()
    .add(KeySet::new(PhysicalKey::KeyS).ctrl(), SaveIntent)
    .add(KeySet::new(PhysicalKey::KeyZ).ctrl(), UndoIntent)
    .add(KeySet::new(PhysicalKey::F5), RefreshIntent)
    .child(
        Actions::new()
            .on::<SaveIntent>(|_| save_document())
            .on::<UndoIntent>(|_| undo())
            .on::<RefreshIntent>(|_| refresh())
            .child(MyApp)
            .build()
    )
    .build()
```

### Example 3: Form with Focus Management

```rust
use flui_widgets::{Focus, FocusNode, Column, TextField};

let name_focus = FocusNode::new();
let email_focus = FocusNode::new();
let password_focus = FocusNode::new();

Column::new()
    .children(vec![
        Box::new(
            Focus::new(name_focus.clone())
                .autofocus(true)
                .child(TextField::new().label("Name"))
                .build()
        ),
        Box::new(
            Focus::new(email_focus.clone())
                .child(TextField::new().label("Email"))
                .build()
        ),
        Box::new(
            Focus::new(password_focus.clone())
                .child(TextField::new().label("Password").obscure(true))
                .build()
        ),
        Box::new(
            Button::new("Submit")
                .on_pressed(|| {
                    // Submit form
                })
        ),
    ])
```

## Implementation Priority

### Phase 1: Basic Focus (~4 hours)
- ✅ EventRouter already routes KeyEvent to focused layer
- ⏳ FocusManager (global singleton)
- ⏳ FocusNode (focus handle)
- ⏳ Focus widget (makes child focusable)

### Phase 2: Shortcuts (~3 hours)
- ⏳ KeySet (keyboard shortcut)
- ⏳ Intent trait
- ⏳ Shortcuts widget (shortcut registration)
- ⏳ Actions widget (intent handlers)

### Phase 3: Advanced (~3 hours)
- ⏳ FocusScope (focus boundaries)
- ⏳ FocusTraversalPolicy (Tab navigation)
- ⏳ DefaultFocusTraversal (automatic Tab order)

**Total: ~10 hours**

## Key Design Decisions

1. **Global FocusManager** - Only one widget can have focus at a time
2. **FocusNode separate from widget** - Reusable across rebuilds
3. **Intent-Action pattern** - Decouples shortcuts from handlers (like Flutter)
4. **Focus != GestureDetector** - Completely separate systems

## Comparison with GestureDetector

| Feature | GestureDetector | Focus + Keyboard |
|---------|----------------|------------------|
| **Events** | Pointer (mouse, touch) | Keyboard |
| **Targeting** | Hit testing (spatial) | Focus (which widget) |
| **Global State** | None | FocusManager |
| **Widget Tree** | Leaf concern | Tree-wide concern |
| **Crate** | flui_gestures | flui_widgets |

## Summary

**GestureDetector is ONLY for pointer events (tap, drag, etc.)**

**For keyboard:**
- Use **Focus** widget to make widget focusable
- Use **Shortcuts** + **Actions** for hotkeys
- Use **KeyboardListener** for low-level key events

Both systems are independent but work together through EventRouter!


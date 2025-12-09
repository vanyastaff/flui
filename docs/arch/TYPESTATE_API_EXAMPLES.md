# Typestate API Usage Examples

## Current API (Before Typestate)

```rust
// demos/counter/src/main.rs - CURRENT
use flui_widgets::{Padding, Text};

fn main() {
    // Problem: Need .leaf() to convert to ViewObject
    let padding = Padding::all(32.0)
        .with_child(Text::headline("Hello, FLUI!").leaf());  // ❌ .leaf() required

    run_app_element(padding);
}
```

**Problems:**
- `.leaf()` is verbose and non-Flutter-like
- `Child` stores `ViewObject` (state), not config
- Can't hot-reload (config lost after conversion)
- Can't reconcile (can't compare ViewObjects)

---

## New API (With Typestate)

### Option 1: User-Facing API Stays Clean (Recommended)

```rust
// demos/counter/src/main.rs - NEW (user code unchanged!)
use flui_widgets::{Padding, Text};

fn main() {
    // ✅ Clean API - no .leaf() needed!
    let padding = Padding::all(32.0)
        .child(Text::headline("Hello, FLUI!"));

    run_app_element(padding);
}
```

**How it works internally:**
- `Padding` and `Text` are just config structs (no typestate marker!)
- `Child::new()` automatically creates `ViewHandle<Unmounted>`
- Framework handles mounting when building tree

**User doesn't see typestate!** It's an internal implementation detail.

---

### Option 2: Explicit Typestate in Widget API (Advanced)

```rust
// demos/counter/src/main.rs - NEW (explicit typestate)
use flui_widgets::{Padding, Text};
use flui_tree::{Unmounted, Mounted};

fn main() {
    // Create widget configs (no state yet)
    let text_config = Text::headline("Hello, FLUI!");
    let padding_config = Padding::all(32.0).child(text_config);

    // Framework converts to ViewHandle<Unmounted> internally
    // then mounts when adding to tree
    run_app_element(padding_config);
}
```

**User still doesn't see typestate markers!** They work with plain config structs.

---

## Internal Implementation (How Framework Uses Typestate)

### Before: Child Stores State (Wrong!)

```rust
// flui-view/src/children/child.rs - CURRENT (WRONG!)

pub struct Child {
    inner: Option<Box<dyn ViewObject>>,  // ❌ State, not config!
}

impl Child {
    pub fn new<V: IntoView>(view: V) -> Self {
        Self {
            inner: Some(view.into_view()),  // Config immediately lost!
        }
    }
}
```

**Problems:**
- Config lost after `into_view()`
- Can't hot-reload (no way to recreate)
- Can't reconcile (can't compare types)

### After: Child Stores Config with Typestate (Correct!)

```rust
// flui-view/src/children/child.rs - NEW (CORRECT!)

use flui_tree::{Unmounted, NodeState};

/// Type-erased unmounted view for heterogeneous children.
pub trait AnyUnmountedView: Send + Sync {
    fn mount(self: Box<Self>, parent: Option<usize>) -> Box<dyn ViewObject>;
    fn type_id(&self) -> TypeId;
    fn clone_config(&self) -> Box<dyn AnyUnmountedView>;
}

pub struct Child {
    // ✅ Stores config (unmounted), not state!
    inner: Option<Box<dyn AnyUnmountedView>>,
}

impl Child {
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        // Wrap in ViewHandle<Unmounted>
        let handle = ViewHandle::<Unmounted>::new(AnyView::new(view));

        Self {
            inner: Some(Box::new(handle)),  // ✅ Config preserved!
        }
    }

    /// Mount the child (called by framework during build)
    pub fn mount(&mut self, parent: Option<usize>) -> Option<Box<dyn ViewObject>> {
        if let Some(unmounted) = self.inner.take() {
            Some(unmounted.mount(parent))
        } else {
            None
        }
    }

    /// Hot-reload: recreate ViewObject from stored config
    pub fn hot_reload(&mut self) -> Option<Box<dyn ViewObject>> {
        if let Some(unmounted) = &self.inner {
            Some(unmounted.clone_config().mount(None))
        } else {
            None
        }
    }
}
```

**Benefits:**
- ✅ Config preserved in `ViewHandle<Unmounted>`
- ✅ Can mount multiple times (hot-reload)
- ✅ Can compare types for reconciliation
- ✅ Framework controls mounting timing

---

## Widget Implementation (How Widgets Use Typestate Internally)

### Text Widget (Leaf - No Children)

```rust
// flui_widgets/src/basic/text.rs - NEW

use flui_tree::{Leaf, Unmounted, Mounted, NodeState};

/// Text widget configuration (no typestate marker for users!)
#[derive(Clone, Debug)]
pub struct Text {
    content: String,
    style: TextStyle,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
        }
    }

    pub fn headline(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::headline(),
        }
    }
}

// Users just pass Text to .child() - framework handles rest!
impl IntoView for Text {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(TextViewObject { config: self })
    }
}

// Internal ViewObject (framework uses this)
struct TextViewObject {
    config: Text,  // Store config for hot-reload
}

impl ViewObject for TextViewObject {
    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        None  // Leaf widget - no children
    }
}
```

**User API:**
```rust
Text::headline("Hello!")  // Just a config struct - no typestate!
```

### Padding Widget (Single Child)

```rust
// flui_widgets/src/basic/padding.rs - NEW

use flui_tree::{Single, Unmounted};

/// Padding widget configuration (no typestate marker for users!)
#[derive(Clone, Debug)]
pub struct Padding {
    padding: EdgeInsets,
    child: Child,  // Child stores ViewHandle<Unmounted> internally
}

impl Padding {
    pub fn all(padding: f32) -> Self {
        Self {
            padding: EdgeInsets::all(padding),
            child: Child::none(),
        }
    }

    /// Add a child (returns new Padding with child)
    pub fn child<V: IntoView + Clone>(mut self, view: V) -> Self {
        self.child = Child::new(view);  // Creates ViewHandle<Unmounted> internally
        self
    }
}

impl IntoView for Padding {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(PaddingViewObject { config: self })
    }
}

struct PaddingViewObject {
    config: Padding,
}

impl ViewObject for PaddingViewObject {
    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Mount child (ViewHandle<Unmounted> → ViewHandle<Mounted>)
        self.config.child.mount(ctx.element_id())
    }
}
```

**User API (unchanged!):**
```rust
Padding::all(32.0).child(Text::headline("Hello!"))  // Clean and simple!
```

---

## Complete Example: Counter App

### User Code (demos/counter/src/main.rs)

```rust
// User code - NO TYPESTATE VISIBLE!
use flui_widgets::{Column, Padding, Text, Button};

fn counter_app(count: i32) -> impl IntoView {
    Column::new()
        .children(vec![
            Padding::all(16.0)
                .child(Text::headline(format!("Count: {}", count))),

            Button::new("Increment")
                .on_click(move |_| {
                    // increment count
                }),
        ])
}

fn main() {
    run_app(counter_app(0));
}
```

**User sees:**
- ✅ Clean, Flutter-like API
- ✅ No `.leaf()` needed
- ✅ No typestate markers
- ✅ Just config structs

### Framework Code (flui_core/src/pipeline/tree_coordinator.rs)

```rust
// Framework code - USES TYPESTATE INTERNALLY
use flui_tree::{Unmounted, Mounted, Mountable};

impl TreeCoordinator {
    /// Mount phase: Unmounted → Mounted
    fn mount_view(&mut self, view_handle: ViewHandle<Unmounted>, parent: Option<ElementId>) -> ElementId {
        // Create element
        let element_id = self.create_element();

        // Mount view (Unmounted → Mounted)
        let mounted_view = view_handle.mount(Some(parent.unwrap_or(0)));

        // Store mounted view in element
        self.elements.insert(element_id, Element {
            view_object: Some(mounted_view.view_object()),
            tree_info: mounted_view.tree_info().clone(),
        });

        element_id
    }

    /// Build phase: Create children from mounted view
    fn build_view(&mut self, element_id: ElementId) {
        let element = self.elements.get_mut(element_id);
        let view_object = element.view_object.as_mut().unwrap();

        // Build returns child configs (as ViewHandle<Unmounted>)
        if let Some(child_view_obj) = view_object.build(ctx) {
            // Mount child recursively
            let child_id = self.mount_view(child_handle, Some(element_id));
            element.tree_info.add_child(child_id);
        }
    }

    /// Hot-reload: Recreate from config
    fn hot_reload(&mut self, element_id: ElementId) {
        let element = self.elements.get_mut(element_id);

        // Unmount (Mounted → Unmounted)
        let unmounted = element.unmount();

        // Re-mount with fresh ViewObject
        let mounted = unmounted.mount(element.tree_info.parent);

        element.view_object = Some(mounted.view_object());
    }
}
```

**Framework uses:**
- ✅ `ViewHandle<Unmounted>` to store configs
- ✅ `.mount()` to create live ViewObjects
- ✅ `.unmount()` for hot-reload
- ✅ Type-safe state transitions

---

## Key Takeaways

### For Users (Widget Authors)

**Nothing changes!** The API is exactly the same:

```rust
// Before and after - SAME!
Padding::all(32.0).child(Text::headline("Hello, FLUI!"))
```

### For Framework Developers

**Big improvements:**

1. **Config Preservation**: `Child` stores `ViewHandle<Unmounted>` with config
2. **Hot-Reload**: Can recreate `ViewObject` from config
3. **Reconciliation**: Can compare types for efficient updates
4. **Type Safety**: Compile-time guarantees about mount state

### Architecture Benefits

```
Old (Wrong):
User → Padding::all(32).child(Text::new()) → Child stores ViewObject → Config lost! ❌

New (Correct):
User → Padding::all(32).child(Text::new())
     → Child stores ViewHandle<Unmounted>
     → Config preserved!
     → Framework mounts when needed
     → Can hot-reload/reconcile ✅
```

---

## Summary

**User-facing API stays clean:**
```rust
// No typestate markers visible to users!
Padding::all(32.0).child(Text::headline("Hello!"))
```

**Framework uses typestate internally:**
```rust
// Framework code
ViewHandle<Unmounted> → .mount() → ViewHandle<Mounted>
                                  ↓
                            ViewObject for rendering
```

**Best of both worlds:**
- ✅ Users get clean, Flutter-like API
- ✅ Framework gets type-safe lifecycle management
- ✅ Config preserved for hot-reload/reconciliation
- ✅ Compile-time guarantees prevent bugs

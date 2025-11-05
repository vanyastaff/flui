# Quick Start - Creating Widgets with FLUI

## 🚀 5-Minute Guide

### Step 1: Choose Your Element Type

| Widget Type | Use | Element Type |
|-------------|-----|--------------|
| **Composite** (99%) | Combining other widgets | `ComponentElement` |
| **Render** (1%) | Custom layout/paint | `RenderElement` |
| **Provider** (rare) | Context data | `InheritedElement` |

### Step 2: Create Your Widget

#### Example: Composite Widget (Button)

```rust
use flui_core::view::{View, BuildContext, ChangeFlags};
use flui_core::element::ComponentElement;

#[derive(Clone, PartialEq)]  // ← Always implement Clone and PartialEq!
pub struct Button {
    text: String,
    on_press: Option<Callback>,
}

impl View for Button {
    type Element = ComponentElement;  // ← Composite widget!
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Compose from other widgets
        let content = Text::new(&self.text);
        let padded = Padding::new(8.0, content);
        let background = ColoredBox::new(Color::BLUE, padded);

        // Build child and create element
        let (child_element, _) = background.build(ctx);
        let element = ComponentElement::new(
            Box::new(self.clone()),
            Box::new(())
        );
        // ... attach child ...

        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // ⚡ Performance: Only rebuild if props changed!
        if self == *prev {
            return ChangeFlags::NONE;  // ← 10-100x faster!
        }

        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

### Step 3: Add State (if needed)

```rust
use flui_core::hooks::use_signal;
use flui_core::element::ComponentElement;

#[derive(Clone)]
pub struct Counter {
    initial: i32,
}

impl View for Counter {
    type Element = ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Reactive state with hooks
        let count = use_signal(ctx, self.initial);

        // Clone for callbacks
        let count_inc = count.clone();

        // Build UI
        let text = Text::new(format!("Count: {}", count.get_untracked()));
        let button = Button::new("Increment", move || {
            count_inc.update(|n| *n += 1);
        });

        let column = Column::new(vec![text, button]);
        // ...

        (element, ())
    }
}
```

## 📋 Checklist

When creating a widget:

- [ ] Derive `Clone` (required by View trait)
- [ ] Derive `PartialEq` (for rebuild optimization)
- [ ] Choose correct Element type:
  - `ComponentElement` for composite widgets (most common)
  - `RenderElement` when wrapping RenderObject
  - `InheritedElement` for context providers
- [ ] Override `rebuild()` if widget is expensive
- [ ] Return `ChangeFlags::NONE` when nothing changed
- [ ] Use `use_signal` for state, not `State` associated type
- [ ] Clone signals before moving into closures
- [ ] Follow the 3 Rules of Hooks (if using hooks)

## 🎯 Common Patterns

### Pattern 1: Simple Stateless Widget

```rust
use flui_core::element::RenderElement;

#[derive(Clone, PartialEq)]
pub struct Text {
    text: String,
}

impl View for Text {
    type Element = RenderElement;  // ← Wraps RenderText
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let render = RenderText::new(self.text);
        let element = RenderElement::new(RenderNode::new(render));
        (element, ())
    }

    fn rebuild(self, prev: &Self, _: &mut (), element: &mut Self::Element) -> ChangeFlags {
        if self.text == prev.text {
            ChangeFlags::NONE  // ← Skip rebuild!
        } else {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        }
    }
}
```

### Pattern 2: Widget with Hooks

```rust
use flui_core::element::ComponentElement;

#[derive(Clone)]
pub struct TextField {
    placeholder: String,
}

impl View for TextField {
    type Element = ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // State
        let text = use_signal(ctx, String::new());

        // Derived state
        let is_empty = use_memo(ctx, |hook_ctx| {
            text.get(hook_ctx).is_empty()
        });

        // Build UI based on state
        // ...

        (element, ())
    }
}
```

### Pattern 3: Generic Widget

```rust
use flui_core::element::ComponentElement;

#[derive(Clone)]
pub struct Padding<V: View> {
    padding: f32,
    child: V,
}

impl<V: View> View for Padding<V> {
    type Element = ComponentElement;
    type State = V::State;  // ← Forward child state!

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child
        let (child_element, child_state) = self.child.build(ctx);

        // Create element with child
        // ...

        (element, child_state)
    }
}
```

## 🚫 Common Mistakes

### ❌ Don't: Call hooks conditionally
```rust
// WRONG!
if self.show_count {
    let count = use_signal(ctx, 0);  // ← Hooks order changes!
}
```

### ✅ Do: Make values conditional
```rust
// CORRECT!
let count = use_signal(ctx, 0);
let display = if self.show_count {
    format!("Count: {}", count.get_untracked())
} else {
    String::new()
};
```

### ❌ Don't: Always mark dirty
```rust
// WRONG - rebuilds every frame!
fn rebuild(self, _: &Self, _: &mut (), element: &mut Self::Element) -> ChangeFlags {
    element.mark_dirty();
    ChangeFlags::NEEDS_BUILD
}
```

### ✅ Do: Check if changed
```rust
// CORRECT - only rebuild when needed!
fn rebuild(self, prev: &Self, _: &mut (), element: &mut Self::Element) -> ChangeFlags {
    if self == *prev {
        return ChangeFlags::NONE;  // ← 10-100x faster!
    }
    element.mark_dirty();
    ChangeFlags::NEEDS_BUILD
}
```

### ❌ Don't: Move signals directly
```rust
// WRONG - signal moved into closure!
Button::new("Inc", move || {
    count.update(|n| *n += 1);  // ← count moved!
})
// count is no longer available!
```

### ✅ Do: Clone signals first
```rust
// CORRECT - clone before moving!
let count_clone = count.clone();
Button::new("Inc", move || {
    count_clone.update(|n| *n += 1);
})
// count still available!
```

## 📚 Next Steps

1. Read [ARCHITECTURE.md](./ARCHITECTURE.md) for deep dive
2. Read [VIEW_GUIDE.md](./VIEW_GUIDE.md) for all View patterns
3. Read [HOOKS_GUIDE.md](./HOOKS_GUIDE.md) for state management
4. Run examples:
   ```bash
   cargo run --example 01_architecture_demo
   cargo run --example 04_dx_improvements
   ```

## 💡 Pro Tips

1. **Always implement PartialEq** - Enables rebuild optimization
2. **Use type aliases** - `WidgetElement` is clearer than `ComponentElement`
3. **Profile before optimizing** - Not all widgets need custom rebuild()
4. **Keep views small** - Break complex widgets into smaller pieces
5. **Follow Flutter patterns** - Architecture is similar, patterns transfer

## 🎯 You're Ready!

You now know enough to start creating widgets. The most important concepts:

1. **Views are immutable** - Created fresh each frame
2. **Elements are mutable** - Hold state and lifecycle
3. **Choose right Element type** - WidgetElement for most widgets
4. **Optimize rebuild()** - Check if props changed
5. **Use hooks for state** - Not the State associated type

Happy coding! 🚀

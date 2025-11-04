# Rules of Hooks

This document defines the **Rules of Hooks** for flui-core, which are critical for correct behavior of the hook system.

## TL;DR

1. ✅ **Always call hooks in the same order**
2. ❌ **Never call hooks conditionally**
3. ❌ **Never call hooks in loops with variable iterations**
4. ✅ **Only call hooks during component rendering**

**Breaking these rules causes panics!**

---

## Why These Rules Exist

flui's hook system (inspired by React) identifies hooks by their **call position** within a component, not by their name or type. Each hook call gets a sequential index (0, 1, 2, ...) based on the order it appears during rendering.

On subsequent renders, the hook system expects **the same hooks in the same order**. If the order changes, the system will try to match the wrong hook state with the wrong hook type, causing a panic.

### Example: Why Order Matters

```rust
// First render:
let count = use_signal(ctx, 0);     // Hook index 0
let name = use_signal(ctx, "");     // Hook index 1

// Second render (same order):
let count = use_signal(ctx, 0);     // Hook index 0 ✅ Matches!
let name = use_signal(ctx, "");     // Hook index 1 ✅ Matches!
```

If you break the order, the system gets confused:

```rust
// First render:
let count = use_signal(ctx, 0);     // Hook index 0

// Second render (different order):
let name = use_signal(ctx, "");     // Hook index 0 ❌ PANIC!
// System expects Signal<i32> but got Signal<&str>
```

---

## Rule #1: Always Call Hooks in the Same Order

**Every render must call the exact same hooks in the exact same order.**

### ✅ Correct Example

```rust
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        let count = use_signal(ctx, 0);
        let double = use_memo(ctx, |hook_ctx| {
            count.get() * 2
        });

        Text::new(format!("Count: {}, Double: {}", count.get(), double.get())).into()
    }
}
```

Every render calls:
1. `use_signal` (index 0)
2. `use_memo` (index 1)

Same order, every time ✅

---

## Rule #2: Never Call Hooks Conditionally

**Don't put hooks inside `if` statements, `match` expressions, or any conditional logic.**

### ❌ Wrong: Conditional Hook

```rust
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        let count = use_signal(ctx, 0);

        // ❌ WRONG: Hook only called sometimes
        if count.get() > 10 {
            let message = use_signal(ctx, "Too high!");
        }

        Text::new(format!("Count: {}", count.get())).into()
    }
}
```

**Why it breaks:**

- **First render** (count = 0): Calls 1 hook (use_signal for count)
- **After count = 11**: Calls 2 hooks (use_signal for count, use_signal for message)
- **After count = 5**: Calls 1 hook again (use_signal for count)

Hook indices change between renders! PANIC!

### ✅ Correct: Conditional Value, Not Hook

```rust
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        let count = use_signal(ctx, 0);
        let message = use_signal(ctx, "");

        // ✅ CORRECT: Hook always called, value is conditional
        if count.get() > 10 {
            message.set("Too high!");
        } else {
            message.set("");
        }

        Text::new(format!("Count: {}", count.get())).into()
    }
}
```

---

## Rule #3: Never Call Hooks in Loops with Variable Iterations

**Don't call hooks inside loops where the number of iterations can change between renders.**

### ❌ Wrong: Hooks in Variable Loop

```rust
impl View for ItemList {
    fn build(self, ctx: &BuildContext) -> View {
        let items = use_signal(ctx, vec![1, 2, 3]);

        // ❌ WRONG: Number of hook calls changes with items.len()
        for item in items.get().iter() {
            let item_signal = use_signal(ctx, *item);
        }

        Container::new().into()
    }
}
```

**Why it breaks:**

- **First render** (3 items): Calls 4 hooks (1 for items + 3 for loop)
- **After adding item**: Calls 5 hooks (1 for items + 4 for loop)

Hook count changes! PANIC!

### ✅ Correct: Fixed Number of Hooks

```rust
impl View for ItemList {
    fn build(self, ctx: &BuildContext) -> View {
        let items = use_signal(ctx, vec![1, 2, 3]);

        // ✅ CORRECT: Use a single signal for the whole list
        Column::new()
            .children(items.get().iter().map(|item| {
                Text::new(item.to_string()).into()
            }))
            .into()
    }
}
```

---

## Rule #4: Only Call Hooks During Component Rendering

**Hooks must be called directly in the component's `build()` method, not in:**

- ❌ Event handlers
- ❌ Async callbacks
- ❌ `use_effect` callbacks
- ❌ Constructors or setup functions

### ❌ Wrong: Hook in Event Handler

```rust
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        Button::new("Click", move |_| {
            // ❌ WRONG: Hook called in event handler
            let count = use_signal(ctx, 0);
            count.update(|n| n + 1);
        }).into()
    }
}
```

### ✅ Correct: Hook in Build, Handler Uses It

```rust
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        // ✅ CORRECT: Hook called during rendering
        let count = use_signal(ctx, 0);

        Button::new("Click", move |_| {
            // ✅ CORRECT: Handler uses the signal, doesn't create it
            count.update(|n| n + 1);
        }).into()
    }
}
```

---

## What Happens When You Break the Rules?

When you violate the Rules of Hooks, you'll get a panic with a message like:

```
Hook state type mismatch at component ComponentId(42) index 2.
Expected: SignalState<i32>
This usually means hooks are called conditionally or in different order between renders.
```

### Debugging Steps

1. **Check for conditional hooks**: Search for `if`, `match`, or `?` before hook calls
2. **Check for loops**: Look for `for` or `while` loops containing hooks
3. **Count your hooks**: Every render should call the exact same number of hooks
4. **Check call order**: Hooks should appear in the same order in every code path

### Common Mistakes

| Mistake | Example | Fix |
|---------|---------|-----|
| Conditional hook | `if x { use_signal(...) }` | Move hook outside `if`, make value conditional |
| Early return | `if !ready { return; } use_signal(...)` | Call hook before early return |
| Loop hook | `for item in list { use_signal(...) }` | Use one signal for entire list |
| Nested hook | `use_memo(..., \|\| use_signal(...))` | Call both hooks at top level |

---

## Advanced Patterns

### Dynamic Lists: Use Keys, Not Hook Arrays

For rendering dynamic lists of components, use **view keys** instead of trying to create a hook per item:

```rust
// ✅ CORRECT: Let the framework handle dynamic children
Column::new()
    .children(items.iter().map(|item| {
        Text::new(item.name.clone())
            .key(Key::from_u64(item.id))  // Key identifies the view
            .into()
    }))
    .into()
```

The framework uses keys to track which components correspond to which items, even as the list changes.

### Computed Values: Use Memo

If you need to compute values based on signals, use `use_memo`:

```rust
let count = use_signal(ctx, 0);
let is_even = use_memo(ctx, |hook_ctx| {
    count.get() % 2 == 0
});
```

### Side Effects: Use Effect

For running code after rendering (like logging or API calls):

```rust
let count = use_signal(ctx, 0);

use_effect(ctx, move || {
    println!("Count changed to: {}", count.get());
    None  // No cleanup needed
});
```

---

## Compile-Time Enforcement

**Future work:** We're considering a proc macro lint that enforces these rules at compile time:

```rust
#[enforce_hook_rules]
impl View for Counter {
    fn build(self, ctx: &BuildContext) -> View {
        if condition {
            let count = use_signal(ctx, 0);  // ← Compile error!
        }
        // ...
    }
}
```

---

## Related Documentation

- [`hook_context.rs`](hook_context.rs) - Hook context implementation
- [`signal.rs`](signal.rs) - Signal hook implementation
- [`memo.rs`](memo.rs) - Memoization hook implementation
- [`effect.rs`](effect.rs) - Effect hook implementation

---

## Summary

**The Golden Rule:** If you call the same hooks in the same order every render, you'll never have problems.

When in doubt, ask yourself:
- "Does this code path always call the same hooks?"
- "Could the number of hooks change between renders?"

If the answer is "no" or "maybe", refactor before you panic! 🚨

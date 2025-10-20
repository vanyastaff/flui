# Future Enhancement: Derive Macros for Widget Implementation

## Current State (Declarative Macros)

Currently, StatefulWidget and InheritedWidget require manual macro invocation:

```rust
#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

// Required manual step:
impl_widget_for_stateful!(Counter);
```

## Future Enhancement: Procedural Derive Macros

### Goal

Make widget implementation automatic and seamless:

```rust
#[derive(Debug, Clone, StatefulWidget)]  // ✨ Just add to derive!
struct Counter {
    initial: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

// ✅ No manual macro call needed!
// Derive automatically generates Widget impl
```

### Benefits

1. **Cleaner API** - No extra line needed
2. **Less boilerplate** - Derive does it all
3. **Better DX** - Just add `#[derive(...)]`
4. **Familiar pattern** - Like `#[derive(Debug, Clone)]`
5. **Compile-time** - Zero runtime cost

### Implementation Plan

#### Phase 1: Create `flui_macros` Crate

```toml
[workspace]
members = [
    "crates/flui_core",
    "crates/flui_macros",  # ← New crate
    # ...
]
```

#### Phase 2: Implement Derive Macros

Create three derive macros:

##### 1. `#[derive(StatefulWidget)]`

```rust
// In flui_macros/src/lib.rs
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(StatefulWidget)]
pub fn derive_stateful_widget(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl ::flui_core::Widget for #name {
            type Element = ::flui_core::StatefulElement<#name>;

            fn into_element(self) -> Self::Element {
                ::flui_core::StatefulElement::new(self)
            }
        }
    };

    TokenStream::from(expanded)
}
```

##### 2. `#[derive(InheritedWidget)]`

```rust
#[proc_macro_derive(InheritedWidget)]
pub fn derive_inherited_widget(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl ::flui_core::Widget for #name {
            type Element = ::flui_core::InheritedElement<#name>;

            fn into_element(self) -> Self::Element {
                ::flui_core::InheritedElement::new(self)
            }
        }
    };

    TokenStream::from(expanded)
}
```

##### 3. (Optional) `#[derive(RenderObjectWidget)]`

For render object widgets in the future.

#### Phase 3: Update Dependencies

```toml
# In flui_core/Cargo.toml
[dependencies]
flui_macros = { path = "../flui_macros" }

# In flui_macros/Cargo.toml
[lib]
proc-macro = true

[dependencies]
syn = "2.0"
quote = "1.0"
proc-macro2 = "1.0"
```

#### Phase 4: Migration Guide

##### For Users

**Before (declarative macro):**
```rust
impl StatefulWidget for MyWidget { ... }
impl_widget_for_stateful!(MyWidget);
```

**After (derive macro):**
```rust
#[derive(StatefulWidget)]
impl StatefulWidget for MyWidget { ... }
```

##### Backward Compatibility

Keep declarative macros for backward compatibility:
- `impl_widget_for_stateful!` - still works
- `impl_widget_for_inherited!` - still works
- Mark as `#[deprecated]` with migration hint

### Examples

#### StatefulWidget with Derive

```rust
#[derive(Debug, Clone, StatefulWidget)]
struct TodoList {
    items: Vec<String>,
}

impl StatefulWidget for TodoList {
    type State = TodoListState;
    fn create_state(&self) -> Self::State {
        TodoListState {
            items: self.items.clone(),
            selected: None,
        }
    }
}

// ✅ Widget impl generated automatically!
```

#### InheritedWidget with Derive

```rust
#[derive(Debug, Clone, InheritedWidget)]
struct Theme {
    primary_color: Color,
    child: Box<dyn AnyWidget>,
}

impl InheritedWidget for Theme {
    type Data = Color;
    fn data(&self) -> &Color { &self.primary_color }
    fn child(&self) -> &dyn AnyWidget { &*self.child }
    fn update_should_notify(&self, old: &Self) -> bool {
        self.primary_color != old.primary_color
    }
}

// ✅ Widget impl generated automatically!
```

#### Multiple Derives

```rust
// Works seamlessly with other derives
#[derive(Debug, Clone, PartialEq, Eq, StatefulWidget)]
struct Counter {
    value: i32,
}
```

### Testing Strategy

1. **Unit tests** - Test macro expansion
2. **Integration tests** - Test with real widgets
3. **Compile-fail tests** - Test error messages
4. **Doc tests** - Ensure examples compile

### Timeline

- **Phase 1 (Create crate):** 1-2 hours
- **Phase 2 (Implement macros):** 3-4 hours
- **Phase 3 (Update deps):** 1 hour
- **Phase 4 (Migration guide):** 1-2 hours
- **Testing:** 2-3 hours

**Total estimated time:** 8-12 hours

### Priority

- **Priority:** Medium
- **Impact:** High (improves DX significantly)
- **Effort:** Medium
- **Risk:** Low (doesn't change core architecture)

### Dependencies

- ✅ Element associated types implementation (DONE)
- ✅ Declarative macros working (DONE)
- ⏸️ Needs separate `flui_macros` crate
- ⏸️ Needs proc-macro dependencies

### Notes

- Proc macros must be in separate crate (Rust requirement)
- Can't be in `flui_core` directly
- Will increase compile times slightly (proc macro overhead)
- But massively improves developer experience

### Alternative: Attribute Macro

Could also use attribute macro style:

```rust
#[stateful_widget]
struct Counter {
    initial: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State { ... }
}
```

But derive is more idiomatic and composable.

---

## Decision: Defer to Future Release

### Reasons to Wait

1. ✅ **Current macros work fine** - Declarative macros are stable and functional
2. ✅ **No blocking issues** - One extra line is acceptable
3. ✅ **Core architecture complete** - Focus on stability first
4. ⏸️ **Proc macros add complexity** - New crate, more dependencies
5. ⏸️ **Can add later** - Non-breaking enhancement

### When to Implement

Consider adding derive macros when:
- User feedback requests it
- flui_core is stable (v1.0+)
- Have capacity for proc macro maintenance
- Want to improve DX further

---

## Conclusion

**Derive macros are the future!** But for now:
- ✅ Declarative macros work perfectly
- ✅ One line of boilerplate is acceptable
- ✅ Can add derives in future release
- ✅ Non-breaking change

**Current priority:** Stabilize core, fix remaining tests, then consider derive macros for v0.2 or v1.0.

---

**Status:** 📋 **Planned for Future Release**

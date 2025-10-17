# Evaluation: Should We Use `bon` Library?

**Date**: 2025-10-16
**Context**: Beginning of nebula-ui development, first widget (Container) being implemented
**Question**: Should we adopt `bon` for builder pattern generation?

## Current Situation

- ✅ First widget (Container) mostly complete with 16 builder methods
- ✅ 491 tests passing
- 🎯 Goal: Build Flutter-like widget system for egui

## `bon` Library Analysis

### What is `bon`?

`bon` is a proc-macro library that generates builder patterns at compile-time using typestate pattern.

**Key Features:**
- Compile-time-checked builders (no runtime overhead)
- Three approaches: function builders, struct builders, method builders
- Type-safe: ensures all required parameters filled at compile time
- Supports async, generics, impl Trait

**Limitations:**
- Focused on auto-generating setters for struct fields
- **Does NOT support custom chaining methods** (like `.child()`)
- Requires proc-macro dependency

## Comparison: Current API vs `bon`

### Current Approach (Manual Builders)

```rust
Container::new()
    .with_width(300.0)
    .with_height(200.0)
    .with_decoration(
        BoxDecoration::new()
            .with_color(Color::from_rgb(100, 150, 255))
            .with_border_radius(BorderRadius::circular(12.0))
    )
    .with_padding(EdgeInsets::all(20.0))
    .with_transform(Transform::rotate_degrees(5.0))
    .child(|ui| {
        ui.label("Hello!");
    })
    .ui(ui);
```

**Pros:**
- ✅ Full API control
- ✅ Custom methods (`.child()`, `.ui()`) work naturally
- ✅ Flutter-like naming (`.with_*`)
- ✅ No proc-macro overhead
- ✅ Clear error messages
- ✅ Fast compilation
- ✅ Easy to debug

**Cons:**
- ❌ ~16 methods written manually per widget (boilerplate)
- ❌ More code to maintain

**Implementation effort per widget:**
```rust
// For each optional field, write:
pub fn with_fieldname(mut self, fieldname: impl Into<Type>) -> Self {
    self.fieldname = Some(fieldname.into());
    self
}
// ~5 lines × 16 fields = ~80 lines of boilerplate
```

### With `bon` Approach

```rust
#[derive(bon::Builder)]
pub struct Container {
    #[builder(default)]
    decoration: Option<BoxDecoration>,
    #[builder(default)]
    padding: Option<EdgeInsets>,
    // ... rest of fields
}

// Generated API:
Container::builder()
    .width(300.0)
    .height(200.0)
    .decoration(BoxDecoration::new().color(Color::from_rgb(100, 150, 255)))
    .padding(EdgeInsets::all(20.0))
    .build()  // ← Required call
    // .child() ← PROBLEM: How to add this?
    .ui(ui);
```

**Pros:**
- ✅ Less boilerplate (just annotate struct)
- ✅ Auto-generated documentation
- ✅ Compile-time validation

**Cons:**
- ❌ **`.child()` method difficult/impossible to integrate** (bon doesn't support custom chaining)
- ❌ Requires `.build()` call (not Flutter-like)
- ❌ Different API style (`.width()` vs `.with_width()`)
- ❌ Proc-macro compilation overhead
- ❌ Less flexible for custom patterns
- ❌ Harder to debug (macro-generated code)
- ❌ Not idiomatic for Flutter-like API

## Critical Problem: Custom Methods

Container needs custom methods that `bon` cannot generate:

```rust
impl Container {
    // bon CAN'T generate this:
    pub fn child(mut self, child: impl FnOnce(&mut egui::Ui) -> egui::Response) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    // bon CAN'T generate this:
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Implementation...
    }
}
```

**Workaround attempts:**
```rust
// Option 1: Mix bon with manual methods (awkward API)
Container::builder()
    .width(300.0)
    .build()  // Must call .build() first
    .child(|ui| { ... })  // Then add custom methods
    .ui(ui);

// Option 2: Don't use bon for child() - inconsistent API
Container::builder()
    .width(300.0)
    // Can't chain .child() here without .build()
```

## Future Widget Considerations

You'll build many widgets. Let's estimate:

**Planned widgets (~50-100+):**
- Primitives: Container, Text, Image, Icon, Spacer
- Layout: Row, Column, Stack, Wrap, Flex
- Input: Button, TextField, Checkbox, Radio, Switch
- ... many more

**With manual builders:**
- ~80 lines boilerplate per widget
- Total: ~4,000-8,000 lines of builder code
- BUT: Consistent, debuggable, flexible API

**With `bon`:**
- ~10 lines annotation per widget
- Total: ~500-1,000 lines
- BUT: Inconsistent API, limited flexibility

## Performance Comparison

| Aspect | Manual Builders | bon |
|--------|----------------|-----|
| **Runtime Performance** | Zero overhead | Zero overhead (compile-time) |
| **Compile Time** | Fast | Slower (proc-macros) |
| **IDE Support** | Excellent | Good (but macro-generated) |
| **Error Messages** | Clear | Can be cryptic (macro errors) |

## Decision Matrix

| Criteria | Weight | Manual | bon | Winner |
|----------|--------|--------|-----|--------|
| **API Consistency** | ⭐⭐⭐⭐⭐ | 10/10 | 6/10 | Manual |
| **Custom Methods Support** | ⭐⭐⭐⭐⭐ | 10/10 | 3/10 | Manual |
| **Flutter-like API** | ⭐⭐⭐⭐⭐ | 10/10 | 5/10 | Manual |
| **Less Boilerplate** | ⭐⭐⭐ | 5/10 | 9/10 | bon |
| **Compile Time** | ⭐⭐⭐ | 9/10 | 6/10 | Manual |
| **Debuggability** | ⭐⭐⭐⭐ | 10/10 | 6/10 | Manual |
| **Flexibility** | ⭐⭐⭐⭐ | 10/10 | 5/10 | Manual |
| **Learning Curve** | ⭐⭐ | 8/10 | 7/10 | Tie |

**Weighted Score:**
- Manual: **8.9/10**
- bon: **5.7/10**

## Alternative: Custom Internal Macro

Instead of `bon`, create a **simple internal macro**:

```rust
// In src/macros.rs
macro_rules! builder_methods {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident: Option<$type:ty>
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: Option<$type>,
            )*
        }

        impl $name {
            $(
                paste::paste! {
                    #[doc = concat!("Set the `", stringify!($field), "` property.")]
                    pub fn [<with_ $field>](mut self, $field: impl Into<$type>) -> Self {
                        self.$field = Some($field.into());
                        self
                    }
                }
            )*
        }
    };
}

// Usage:
builder_methods! {
    pub struct Container {
        pub width: Option<f32>,
        pub height: Option<f32>,
        pub padding: Option<EdgeInsets>,
        // ... rest
    }
}

// Still allows custom methods:
impl Container {
    pub fn child(mut self, child: impl FnOnce(&mut egui::Ui) -> egui::Response) -> Self {
        self.child = Some(Box::new(child));
        self
    }
}
```

**Pros:**
- ✅ Reduces boilerplate
- ✅ No external dependencies
- ✅ Full control over generated code
- ✅ Can coexist with custom methods
- ✅ Maintains `.with_*` naming

**Cons:**
- ❌ Need to implement the macro (but simpler than bon)
- ❌ Less feature-rich than bon

## Final Recommendation

### ❌ **DO NOT use `bon`**

**Primary reasons:**

1. **`.child()` method incompatibility** - This is a deal-breaker. Container's `.child()` method is essential and bon cannot support it cleanly.

2. **Flutter API mismatch** - bon generates `.field()` not `.with_field()`, and requires `.build()` call. Not idiomatic for Flutter-style API.

3. **You're at the beginning** - Yes, but you've already established the pattern. Switching now would be a breaking change.

4. **Future flexibility** - Many widgets will need custom methods. Manual builders give you complete control.

### ✅ **RECOMMENDED: Keep manual builders**

**Rationale:**
- Already working perfectly
- Matches Flutter idioms exactly
- Full control over API
- Easy to debug
- No external dependencies
- Faster compilation

**To reduce boilerplate (optional future enhancement):**
- Consider the internal macro approach above
- Or accept the boilerplate as "explicit is better than implicit"

### 📝 **Boilerplate is not that bad**

Consider:
- 80 lines per widget seems like a lot
- But most of it is copy-paste with field name changes
- Your IDE likely has snippets to generate these
- The explicitness makes code easy to understand
- No "magic" - everything is visible

## Decision

**VERDICT: Do NOT add `bon` dependency**

Stick with manual builder methods because:
1. ⭐⭐⭐⭐⭐ Custom methods (`.child()`) are critical
2. ⭐⭐⭐⭐⭐ Flutter-like API is the goal
3. ⭐⭐⭐⭐ Flexibility for future widgets
4. ⭐⭐⭐⭐ Compilation speed matters
5. ⭐⭐⭐ Debuggability is important

The boilerplate is a reasonable tradeoff for API quality and flexibility.

---

**Status**: ✅ Evaluated, Decision Made
**Next Steps**: Continue with manual builders, consider internal macro only if boilerplate becomes painful

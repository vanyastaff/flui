# Ergonomic InheritedWidget API

**Rust-idiomatic short names for InheritedWidget access**

---

## 🎯 Problem

Flutter-style names are too verbose for Rust:

```rust
// ❌ Too verbose! Not Rust-idiomatic
let theme = context.depend_on_inherited_widget_of_exact_type::<Theme>()
    .expect("No theme");
```

## ✨ Solution

Short, ergonomic methods in pure Rust style:

```rust
// ✅ Short and sweet!
let theme = context.inherit::<Theme>().expect("No theme");

// ✅ Or React-style
let theme = context.watch::<Theme>().expect("No theme");
```

---

## API Reference

### With Dependency (Rebuilds on Changes)

| Method | Style | Example |
|--------|-------|---------|
| `context.inherit::<T>()` | **Recommended** | `context.inherit::<Theme>()` |
| `context.watch::<T>()` | React-style | `context.watch::<Theme>()` |
| `context.depend_on_inherited_widget_of_exact_type::<T>()` | Flutter-style (verbose) | Not recommended |

**All create dependency** - widget rebuilds when inherited data changes.

### Without Dependency (One-Time Read)

| Method | Style | Example |
|--------|-------|---------|
| `context.read_inherited::<T>()` | **Recommended** | `context.read_inherited::<Theme>()` |
| `context.read::<T>()` | React-style | `context.read::<Theme>()` |
| `context.get_inherited_widget_of_exact_type::<T>()` | Flutter-style (verbose) | Not recommended |

**No dependency** - widget does NOT rebuild when data changes.

---

## Usage Examples

### Example 1: Basic Usage

```rust
use flui_core::*;

#[derive(Debug, Clone)]
struct MyButton {
    text: String,
}

impl StatelessWidget for MyButton {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // SHORT API! 🎉
        let theme = context.inherit::<Theme>().expect("No theme");

        Box::new(Button {
            color: theme.color,
            text: self.text.clone(),
        })
    }
}
```

### Example 2: Optional Theme

```rust
impl StatelessWidget for MyButton {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Use default if no theme
        let color = context.inherit::<Theme>()
            .map(|t| t.color)
            .unwrap_or(Color::BLACK);

        Box::new(Button {
            color,
            text: self.text.clone(),
        })
    }
}
```

### Example 3: React-Style Hooks

```rust
impl StatelessWidget for Counter {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // React developers will feel at home!
        let theme = context.watch::<Theme>().expect("No theme");
        let locale = context.watch::<Locale>().expect("No locale");

        Box::new(Text::new(
            format!("{}: {}", locale.get("counter"), self.count)
        ))
    }
}
```

### Example 4: Read Without Dependency

```rust
impl StatelessWidget for AppInitializer {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Read once, no rebuilds
        if let Some(theme) = context.read_inherited::<Theme>() {
            println!("App started with theme: {:?}", theme.color);
        }

        Box::new(MyApp)
    }
}
```

### Example 5: Multiple Inherited Widgets

```rust
impl StatelessWidget for LocalizedButton {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        // Get multiple inherited widgets
        let theme = context.inherit::<Theme>().expect("No theme");
        let locale = context.inherit::<Locale>().expect("No locale");
        let media = context.inherit::<MediaQuery>().expect("No media");

        Box::new(Button {
            color: theme.color,
            text: locale.translate(self.text_key),
            size: media.text_scale_factor,
        })
    }
}
```

---

## Comparison Table

### Flutter vs Flui

| Flutter | Flui (Recommended) | Flui (Alternative) |
|---------|-------------------|-------------------|
| `Theme.of(context)` | `context.inherit::<Theme>()` | `context.watch::<Theme>()` |
| `Theme.maybeOf(context)` | `context.inherit::<Theme>()` | `context.watch::<Theme>()` |
| `context.dependOnInheritedWidgetOfExactType<Theme>()` | `context.inherit::<Theme>()` | `context.watch::<Theme>()` |
| `context.getInheritedWidgetOfExactType<Theme>()` | `context.read_inherited::<Theme>()` | `context.read::<Theme>()` |

### React Hooks vs Flui

| React Hook | Flui Equivalent | Behavior |
|-----------|-----------------|----------|
| `useContext(ThemeContext)` | `context.watch::<Theme>()` | Rebuilds on change |
| `useContext(ThemeContext)` (read once) | `context.read::<Theme>()` | No rebuild |

---

## Creating Custom `.of()` Methods

You can still add Flutter-style `.of()` for familiarity:

```rust
impl Theme {
    /// Flutter-style of() - short and familiar
    pub fn of(context: &Context) -> Self {
        context.inherit::<Theme>().expect("No Theme found")
    }

    /// Flutter-style maybeOf() - returns Option
    pub fn maybe_of(context: &Context) -> Option<Self> {
        context.inherit::<Theme>()
    }
}

// Usage:
let theme = Theme::of(context);  // Panics if not found
let theme = Theme::maybe_of(context);  // Returns Option
```

---

## Performance

All short methods are **zero-cost abstractions** - they compile to the same code as verbose versions:

```rust
// These compile to IDENTICAL code:
context.inherit::<Theme>()
context.watch::<Theme>()
context.depend_on_inherited_widget_of_exact_type::<Theme>()
```

---

## Best Practices

### ✅ Recommended

```rust
// 1. Use inherit() for dependencies
let theme = context.inherit::<Theme>()?;

// 2. Use read_inherited() for one-time reads
let initial_theme = context.read_inherited::<Theme>()?;

// 3. Handle None gracefully
let color = context.inherit::<Theme>()
    .map(|t| t.color)
    .unwrap_or(Color::BLACK);
```

### ❌ Not Recommended

```rust
// Too verbose
let theme = context.depend_on_inherited_widget_of_exact_type::<Theme>()?;

// Unclear intent
let theme = context.subscribe_to::<Theme>()?;  // Old API, still works but unclear
```

---

## Migration Guide

### From Old API

```rust
// Before (old API):
let theme = context.subscribe_to::<Theme>()
    .expect("No theme");

// After (new ergonomic API):
let theme = context.inherit::<Theme>()
    .expect("No theme");
```

### From Flutter

```dart
// Flutter:
final theme = Theme.of(context);

// Flui (option 1 - short):
let theme = context.inherit::<Theme>().expect("No theme");

// Flui (option 2 - add .of() method):
impl Theme {
    pub fn of(context: &Context) -> Self {
        context.inherit::<Theme>().expect("No Theme")
    }
}
let theme = Theme::of(context);
```

---

## Summary

| API | Length | Recommendation |
|-----|--------|---------------|
| `context.inherit::<T>()` | ⭐⭐⭐⭐⭐ | **Best for most cases** |
| `context.watch::<T>()` | ⭐⭐⭐⭐⭐ | **Good for React developers** |
| `context.read_inherited::<T>()` | ⭐⭐⭐⭐ | Good for one-time reads |
| `context.read::<T>()` | ⭐⭐⭐⭐ | Good for React developers |
| `context.depend_on_inherited_widget_of_exact_type::<T>()` | ⭐ | Too verbose |

**Recommendation:** Use `context.inherit::<T>()` for dependencies and `context.read_inherited::<T>()` for one-time reads.

---

**Last Updated:** 2025-10-20
**Related:** Phase 6 - Enhanced InheritedWidget System

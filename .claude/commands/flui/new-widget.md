---
name: New Widget
description: Create a new FLUI widget following project conventions
---

Create a new widget named: **$ARGUMENTS**

Follow these conventions from CLAUDE.md:

1. **View struct**: Immutable, cheap to clone
```rust
pub struct MyWidget {
    // Use Arc for shared data
    // Use String for text (cheap to move)
}
```

2. **Implement View trait**: Single `build()` method
```rust
impl View for MyWidget {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        // Return child elements
    }
}
```

3. **Use tracing for logging**: Never println!
```rust
#[tracing::instrument]
fn render(&self) {
    tracing::debug!("Rendering widget");
}
```

4. **Place in appropriate module**: `crates/flui_widgets/src/`

5. **Export in mod.rs**: Add to public API

6. **Add tests**: Use `#[cfg(test)]` module

Create the widget file and update relevant mod.rs files.

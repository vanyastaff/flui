# flui_widgets Migration Status

## Current Status: NOT STARTED

**Compilation Status**: ❌ 76 errors
**Files to Migrate**: 31 files
**Estimated Effort**: Large (full architectural refactor)

---

## Problem Summary

flui_widgets is built on the **old trait-based Widget architecture**, but flui_core has migrated to an **enum-based Widget architecture**. This is a fundamental incompatibility that requires complete refactoring.

---

## Architecture Comparison

### OLD Architecture (flui_widgets current state)

```rust
// Widget is a trait
pub trait Widget {}

pub trait RenderObjectWidget: Widget {
    type Arity;  // SingleArity, MultiArity, LeafArity
    fn create_render_object(&self) -> Box<dyn RenderObject<Arity = Self::Arity>>;
}

pub trait SingleChildRenderObjectWidget: RenderObjectWidget {
    type Arity = SingleArity;
}

// Wrapper type
pub struct BoxedWidget(Box<dyn Widget>);

// Example widget
pub struct Align { ... }
impl Widget for Align {}
impl RenderObjectWidget for Align {
    type Arity = SingleArity;
    fn create_render_object(&self) -> Box<dyn RenderObject<Arity = SingleArity>> {
        Box::new(RenderAlign::new(self.alignment, ...))
    }
}
```

### NEW Architecture (flui_core)

```rust
// Widget is an enum!
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
    Inherited(Box<dyn InheritedWidget>),
    Render(Box<dyn RenderWidget>),      // ← for render widgets
    ParentData(Box<dyn ParentDataWidget>),
}

// New trait for render widgets
pub trait RenderWidget: Send + Sync + Debug + 'static {
    fn create_render(&self) -> RenderNode;
    fn key(&self) -> Option<&Key> { None }
}

// RenderNode is also an enum!
pub enum RenderNode {
    Leaf(Box<dyn LeafRender>),
    Single(Box<dyn SingleRender>),
    Multi(Box<dyn MultiRender>),
}

// Example widget
pub struct Align { ... }
impl RenderWidget for Align {
    fn create_render(&self) -> RenderNode {
        RenderNode::Single(Box::new(RenderAlign::new(self.alignment, ...)))
    }
}

// Usage
let widget = Widget::render_object(Align { ... });
```

---

## Migration Strategy

### Phase 1: Update Imports (All 31 files)

**Remove:**
```rust
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, ...};
use flui_rendering::{SingleArity, MultiArity, LeafArity};
```

**Add:**
```rust
use flui_core::{Widget, RenderWidget, StatelessWidget};
use flui_core::render::{RenderNode, LeafRender, SingleRender, MultiRender};
use flui_types::constraints::BoxConstraints;
```

### Phase 2: Convert Widget Implementations

#### For Render Widgets (most common)

**Before:**
```rust
pub struct Align { ... }
impl Widget for Align {}
impl RenderObjectWidget for Align {
    type Arity = SingleArity;
    fn create_render_object(&self) -> Box<dyn RenderObject<Arity = SingleArity>> {
        Box::new(RenderAlign::new(...))
    }
}
```

**After:**
```rust
pub struct Align { ... }
impl RenderWidget for Align {
    fn create_render(&self) -> RenderNode {
        RenderNode::Single(Box::new(RenderAlign::new(...)))
    }
}
```

#### For Stateless Widgets

**Before:**
```rust
impl Widget for Container {}
impl StatelessWidget for Container {
    fn build(&self, ctx: &BuildContext) -> BoxedWidget {
        BoxedWidget::new(/* child widget */)
    }
}
```

**After:**
```rust
impl StatelessWidget for Container {
    fn build(&self, ctx: &BuildContext) -> Widget {
        Widget::render_object(/* child widget */)
    }

    fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

### Phase 3: Update Child Fields

**Before:**
```rust
pub struct Align {
    pub child: Option<BoxedWidget>,
}
```

**After:**
```rust
pub struct Align {
    pub child: Option<Widget>,
}
```

### Phase 4: Update Builders

**Before:**
```rust
impl AlignBuilder {
    pub fn child<W: Widget + 'static>(self, child: W) -> Self {
        self.child_internal(BoxedWidget::new(child))
    }
}
```

**After:**
```rust
impl AlignBuilder {
    pub fn child(self, child: Widget) -> Self {
        self.child_internal(Some(child))
    }
}
```

---

## File Categories

### Render Widgets (Single Child) - 15 files
- `basic/align.rs`
- `basic/center.rs`
- `basic/opacity.rs`
- `basic/padding.rs`
- `basic/sized_box.rs`
- `basic/container.rs`
- `material/card.rs`
- `material/ink_well.rs`
- `layout/aspect_ratio.rs`
- `layout/constrained_box.rs`
- `layout/fractionally_sized_box.rs`
- `layout/limited_box.rs`
- `layout/positioned.rs`
- `layout/transform.rs`
- `layout/clip.rs`

### Render Widgets (Multi Child) - 5 files
- `layout/column.rs`
- `layout/row.rs`
- `layout/stack.rs`
- `layout/wrap.rs`
- `layout/flex.rs`

### Render Widgets (Leaf) - 2 files
- `basic/placeholder.rs`
- `text/text.rs`

### Stateless Widgets - 7 files
- `basic/builder.rs`
- `layout/expanded.rs`
- `layout/flexible.rs`
- `layout/spacer.rs`
- `material/divider.rs`
- `material/scaffold.rs`
- Other wrapper widgets

### Other - 2 files
- `lib.rs` - module exports
- `prelude.rs` - convenience imports

---

## Migration Checklist

### Per-File Checklist:
- [ ] Update imports (remove BoxedWidget, add Widget enum)
- [ ] Remove `impl Widget for X`
- [ ] Change `RenderObjectWidget` → `RenderWidget`
- [ ] Update `create_render_object()` → `create_render()` returning `RenderNode`
- [ ] Change `type Arity` → appropriate RenderNode variant
- [ ] Update child fields: `BoxedWidget` → `Widget`
- [ ] Update builder methods
- [ ] Fix return types in methods
- [ ] Add `clone_boxed()` and `as_any()` if needed

### Global Changes:
- [ ] Update lib.rs exports
- [ ] Update prelude.rs
- [ ] Fix all type aliases
- [ ] Update documentation examples
- [ ] Run `cargo fix` for automated fixes
- [ ] Test compilation

---

## Known Issues to Watch For

1. **Builder Pattern Changes**: The bon builders generate code that expects specific types
2. **Child Widget Wrapping**: Need to wrap child widgets in `Widget::render_object()` or appropriate variant
3. **Type Aliases**: Any `type WidgetType = BoxedWidget` needs updating
4. **Documentation**: All doc examples showing `BoxedWidget::new()` need updating

---

## Testing Strategy

After migration:
1. `cargo build -p flui_widgets` - should compile with 0 errors
2. Check that all 31 files compile individually
3. Verify builder patterns still work
4. Test basic widget construction
5. Integration test with flui_core

---

## Next Session Action Plan

1. **Start with simplest files**: `basic/placeholder.rs`, `basic/center.rs`
2. **Create migration script** for common patterns
3. **Use agents** for bulk import updates
4. **Manually verify** render widget implementations
5. **Test incrementally** - compile after each 5-10 files

---

## Dependencies Status

- ✅ flui_core - Updated with new Widget enum
- ✅ flui_engine - No changes needed
- ✅ flui_painting - Migrated to new Paint API
- ✅ flui_rendering - Migrated to new render traits (LeafRender/SingleRender/MultiRender)
- ✅ flui_types - No changes needed
- ❌ flui_widgets - **NEEDS MIGRATION** (this document)

---

## Estimated Time

- **Import fixes**: ~1 hour (can be automated)
- **Widget trait migrations**: ~3-4 hours (31 files, some complex)
- **Builder updates**: ~1 hour
- **Testing & fixes**: ~1 hour
- **Total**: ~6-7 hours

---

## Notes

- This is a **breaking change** for anyone using flui_widgets
- The new enum-based architecture is more performant (no vtable lookups)
- Once migrated, the API will be more type-safe with exhaustive pattern matching
- This aligns flui_widgets with the rest of the flui ecosystem

---

*Document created: 2025-01-29*
*Status: Migration not started, ready for next session*

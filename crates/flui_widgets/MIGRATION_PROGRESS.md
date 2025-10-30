# flui_widgets Migration Progress

## Status: IN PROGRESS (1/31 files migrated)

**Started**: 2025-01-29
**Current Errors**: 71 (down from 76)
**Files Migrated**: 1/31 (center.rs)

---

## ✅ Completed

### center.rs - MIGRATED
- Updated imports (BoxedWidget → Widget, RenderObjectWidget → RenderWidget)
- Changed child field type: `Option<BoxedWidget>` → `Option<Widget>`
- Simplified set_child method
- Updated builder pattern
- Migrated to RenderWidget trait
- Updated tests
- **Status**: ✅ Compiles

---

## Migration Pattern (Verified with center.rs)

### Step 1: Update Imports
```rust
// REMOVE
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{RenderXxx, SingleArity};

// ADD
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::RenderXxx;  // Keep render object imports
```

### Step 2: Update Child Fields
```rust
// OLD
pub child: Option<BoxedWidget>,

// NEW
pub child: Option<Widget>,
```

### Step 3: Simplify set_child Method
```rust
// OLD
pub fn set_child<W>(&mut self, child: W)
where
    W: Widget + std::fmt::Debug + Send + Sync + Clone + 'static,
{
    self.child = Some(BoxedWidget::new(child));
}

// NEW
pub fn set_child(&mut self, child: Widget) {
    self.child = Some(child);
}
```

### Step 4: Remove `impl Widget for X {}`
Delete this line completely.

### Step 5: Update Builder
```rust
// OLD
pub fn child<W: Widget + 'static>(self, child: W) -> XBuilder<SetChild<S>> {
    self.child_internal(BoxedWidget::new(child))
}

// NEW
pub fn child(self, child: Widget) -> XBuilder<SetChild<S>> {
    self.child_internal(Some(child))
}
```

### Step 6: Migrate RenderObjectWidget → RenderWidget
```rust
// OLD
impl RenderObjectWidget for X {
    type RenderObject = RenderXxx;
    type Arity = SingleArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderXxx::new(...)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_xxx(...);
    }
}

impl SingleChildRenderObjectWidget for X {
    fn child(&self) -> &BoxedWidget {
        self.child.as_ref().unwrap_or_else(|| panic!("..."))
    }
}

// NEW
impl RenderWidget for X {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::Single(Box::new(RenderXxx::new(...)))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single(render) = render_object {
            if let Some(xxx) = render.downcast_mut::<RenderXxx>() {
                xxx.set_xxx(...);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}
```

### Step 7: Update Tests
```rust
// OLD MockWidget
impl RenderObjectWidget for MockWidget {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        Box::new(RenderPadding::new(EdgeInsets::ZERO))
    }
    fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
}
impl LeafRenderObjectWidget for MockWidget {}

// NEW MockWidget
impl RenderWidget for MockWidget {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::Single(Box::new(RenderPadding::new(EdgeInsets::ZERO)))
    }
    fn update_render_object(&self, _context: &BuildContext, _render_object: &mut RenderNode) {}
}

// In test usage:
// OLD: MockWidget
// NEW: Widget::from(MockWidget)
```

---

## 📋 Remaining Files to Migrate

### Single-Child Render Widgets (14 files)
- [ ] basic/align.rs
- [ ] basic/padding.rs
- [ ] basic/sized_box.rs
- [ ] basic/aspect_ratio.rs
- [ ] basic/container.rs (complex - has multiple render configs)
- [ ] basic/decorated_box.rs
- [ ] layout/constrained_box.rs
- [ ] layout/fractionally_sized_box.rs
- [ ] layout/limited_box.rs
- [ ] layout/positioned.rs
- [ ] layout/transform.rs
- [ ] layout/clip.rs
- [ ] material/card.rs
- [ ] material/ink_well.rs

### Multi-Child Render Widgets (5 files)
- [ ] layout/column.rs (uses MultiArity)
- [ ] layout/row.rs (uses MultiArity)
- [ ] layout/stack.rs (uses MultiArity)
- [ ] layout/wrap.rs (uses MultiArity)
- [ ] layout/flex.rs (base for column/row)

**Pattern for Multi-Child:**
```rust
// child field becomes children:
pub children: Vec<Widget>,

// RenderNode becomes Multi:
RenderNode::Multi(Box::new(RenderFlex::new(...)))

// children() method returns slice:
fn children(&self) -> Option<&[Widget]> {
    Some(&self.children)
}
```

### Leaf Render Widgets (2 files)
- [ ] basic/text.rs (LeafArity)
- [ ] basic/button.rs (might be StatelessWidget wrapping gesture detector)

**Pattern for Leaf:**
```rust
// No children field

// RenderNode becomes Leaf:
RenderNode::Leaf(Box::new(RenderParagraph::new(...)))

// No child() or children() override needed
```

### Stateless Widgets (7 files)
- [ ] layout/expanded.rs (wraps Flexible)
- [ ] layout/flexible.rs (ParentDataWidget)
- [ ] layout/spacer.rs (wraps SizedBox)
- [ ] material/divider.rs
- [ ] material/scaffold.rs
- [ ] gestures/* (2 files)
- [ ] interaction/* (4 files)

**Pattern for StatelessWidget:**
```rust
impl StatelessWidget for X {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        // Return Widget::render_object(...) or other Widget variants
        Widget::render_object(SomeRenderWidget { ... })
    }

    fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

### Infrastructure (2 files)
- [ ] lib.rs - update exports
- [ ] prelude.rs - update imports

---

## Automation Script

For batch migration, use this agent prompt pattern:

```
Migrate [file.rs] to new Widget API following the pattern from center.rs:
1. Update imports (remove BoxedWidget/RenderObjectWidget, add Widget/RenderWidget)
2. Change child field type to Option<Widget>
3. Update set_child method
4. Remove impl Widget for X
5. Update builder child() method
6. Convert RenderObjectWidget → RenderWidget (use RenderNode::Single)
7. Update tests
```

---

## Quick Commands

```bash
# Check current error count
cargo build -p flui_widgets 2>&1 | grep "^error\[" | wc -l

# Test specific file
cargo build -p flui_widgets 2>&1 | grep "filename.rs"

# List files with errors
cargo build -p flui_widgets 2>&1 | grep "\.rs:" | cut -d: -f1 | sort -u
```

---

## Known Issues

1. **Widget::from()** - Tests may need `Widget::from(MockWidget)` instead of bare `MockWidget`
2. **downcast_mut** - Use pattern matching on RenderNode variant before downcasting
3. **BuildContext** - All create/update methods now require `&BuildContext` parameter
4. **Optional children** - Use `child() -> Option<&Widget>` instead of panicking

---

## Next Session

1. **Continue with align.rs** (similar to center.rs)
2. **Batch migrate padding, sized_box** (simple single-child)
3. **Handle complex ones**: container, decorated_box
4. **Multi-child widgets**: column, row, stack
5. **Leaf widgets**: text
6. **Fix lib.rs exports**

---

*Progress saved: 2025-01-29*
*1/31 files migrated (3.2% complete)*
*71 errors remaining*

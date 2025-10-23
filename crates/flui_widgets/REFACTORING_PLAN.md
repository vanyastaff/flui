# flui_widgets Refactoring Plan

## Goal
Migrate all widgets from old Widget API to new Widget API with associated types.

## API Changes

### Old API
```rust
impl Widget for MyWidget {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(RenderObjectElement::new(self.clone()))
    }
}

pub child: Option<Box<dyn Widget>>,
```

### New API
```rust
impl Widget for MyWidget {
    type Element = SingleChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        SingleChildRenderObjectElement::new(self)
    }
}

pub child: Option<Box<dyn DynWidget>>,
```

## Import Changes

### Types renamed:
- `RenderObject` → `DynRenderObject`
- `Box<dyn Widget>` → `Box<dyn DynWidget>`

### Types moved to flui_types:
- `FlexFit` → `flui_types::layout::FlexFit`
- `StackFit` → `flui_types::layout::StackFit`

### Types that don't exist yet:
- `BuildContext` - needs to be added to flui_core
- `FlexParentData` - needs to be added somewhere
- `MouseRegionCallbacks` - needs to be added somewhere
- `RenderBox` trait - removed, use DynRenderObject

## Element Types

### For different widget categories:

1. **LeafRenderObjectWidget** (no children):
   - Element type: `LeafRenderObjectElement<Self>`
   - Example: Text, Image

2. **SingleChildRenderObjectWidget** (one child):
   - Element type: `SingleChildRenderObjectElement<Self>`
   - Example: Padding, Opacity, Transform
   - Must implement: `fn child(&self) -> &dyn DynWidget`

3. **MultiChildRenderObjectWidget** (multiple children):
   - Element type: `MultiChildRenderObjectElement<Self>`
   - Example: Column, Row, Stack
   - Must implement: `fn children(&self) -> &[Box<dyn DynWidget>]`

4. **StatelessWidget** (builds other widgets):
   - Element type: `ComponentElement<Self>`
   - Must implement: `fn build(&self) -> Box<dyn DynWidget>`

5. **StatefulWidget** (has state):
   - Element type: `StatefulElement<Self>`
   - Must implement: `type State: State` and `fn create_state(&self) -> Self::State`

## Refactoring Strategy

### Phase 1: Fix imports (CURRENT)
- [ ] Remove non-existent imports (BuildContext, RenderBox, etc.)
- [ ] Update imports to use flui_types where appropriate
- [ ] Add missing type definitions where needed

### Phase 2: Simple widgets (one at a time)
Start with single-child widgets (easiest):
- [ ] Padding
- [ ] Opacity
- [ ] Transform
- [ ] Center
- [ ] Align
- [ ] SizedBox
- [ ] AspectRatio

### Phase 3: Multi-child widgets
- [ ] Column
- [ ] Row
- [ ] Stack
- [ ] Wrap

### Phase 4: Stateless widgets
- [ ] Container
- [ ] Button

### Phase 5: Complex widgets
- [ ] Text (LeafRenderObjectWidget)
- [ ] GestureDetector
- [ ] MouseRegion

## Example Migration

### Before (Padding):
```rust
pub struct Padding {
    pub child: Option<Box<dyn Widget>>,
    pub padding: EdgeInsets,
}

impl Widget for Padding {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(RenderObjectElement::new(self.clone()))
    }
}

impl RenderObjectWidget for Padding {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(RenderPadding::new(self.padding))
    }
}
```

### After (Padding):
```rust
pub struct Padding {
    pub child: Option<Box<dyn DynWidget>>,
    pub padding: EdgeInsets,
}

impl Widget for Padding {
    type Element = SingleChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        SingleChildRenderObjectElement::new(self)
    }
}

impl RenderObjectWidget for Padding {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        Box::new(RenderPadding::new(self.padding))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        if let Some(padding) = render_object.downcast_mut::<RenderPadding>() {
            padding.set_padding(self.padding);
        }
    }
}

impl SingleChildRenderObjectWidget for Padding {
    fn child(&self) -> &dyn DynWidget {
        self.child
            .as_ref()
            .map(|b| &**b as &dyn DynWidget)
            .unwrap_or_else(|| panic!("Padding requires a child"))
    }
}
```

## Builder Pattern Updates

The `bon` builder needs updates:
```rust
// Old
impl<S: State> PaddingBuilder<S> {
    pub fn child(self, child: impl Widget + 'static) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}

// New
impl<S: State> PaddingBuilder<S> {
    pub fn child<W: Widget + 'static>(self, child: W) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Some(Box::new(child) as Box<dyn DynWidget>))
    }
}
```

## Testing Strategy

For each migrated widget:
1. Ensure it compiles
2. Ensure basic tests pass
3. Test builder pattern
4. Test element creation

## Notes

- All widgets must implement `Clone` (required by Widget trait)
- Element types are generic over the widget type
- DynWidget is auto-implemented for all Widget types
- No more Box<dyn Element> - use concrete types!

# Flui Enum Architecture - Complete Design

## 🎯 Vision: The Holy Trinity

```
Widget (enum) → Element (enum) → RenderObject (enum)
```

All three core types as enums for perfect architectural symmetry.

## 📊 Complete Architecture

### The Three Trees

```rust
// ============================================================================
// Tree 1: Widget (Immutable Configuration)
// ============================================================================

pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
    Inherited(Box<dyn InheritedWidget>),
    RenderObject(Box<dyn RenderObjectWidget>),
    ParentData(Box<dyn ParentDataWidget>),
}

// ============================================================================
// Tree 2: Element (Mutable State)
// ============================================================================

pub enum Element {
    Component(ComponentElement),
    Stateful(StatefulElement),
    Inherited(InheritedElement),
    Render(RenderElement),
    ParentData(ParentDataElement),
}

// ============================================================================
// Tree 3: RenderObject (Layout & Paint)
// ============================================================================

pub enum RenderObject {
    Leaf(Box<dyn LeafRenderObject>),
    Single {
        render: Box<dyn SingleChildRenderObject>,
        child: Box<RenderObject>,
    },
    Multi {
        render: Box<dyn MultiChildRenderObject>,
        children: Vec<RenderObject>,
    },
}
```

## 🔄 Data Flow

```
User Code
   ↓
Widget::stateless(MyWidget)          ← enum variant
   ↓
Element::Component(ComponentElement)  ← enum variant
   ↓
MyWidget.build() → Widget::render_object(Text)
   ↓
Element::Render(RenderElement)
   ↓
RenderObject::leaf(RenderText)        ← enum variant
   ↓
Layout → Paint → Layers
```

## 💡 Key Design Principles

### 1. No Blanket Impl Conflicts ✅

**Before (Trait Hierarchy):**
```rust
// ❌ Coherence conflict!
impl<W: StatelessWidget> Widget for W { }
impl<W: StatefulWidget> Widget for W { }
```

**After (Enum):**
```rust
// ✅ No conflict - Widget is an enum, not a trait!
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
}
```

### 2. Exhaustive Pattern Matching ✅

```rust
match widget {
    Widget::Stateless(w) => w.build(ctx),
    Widget::Stateful(w) => {
        let state = w.create_state();
        state.build(ctx)
    },
    Widget::Inherited(w) => w.child(),
    Widget::RenderObject(w) => {
        let ro = w.create_render_object(ctx);
        // ...
    },
    Widget::ParentData(w) => w.child(),
    // ✅ Compiler ensures all variants handled!
}
```

### 3. Arity at Type Level ✅

```rust
// Widget arity encoded in trait type
trait StatelessWidget { }  // → Element::Component (no render object)
trait RenderObjectWidget { }  // → Element::Render → RenderObject

// RenderObject arity encoded in enum variant
RenderObject::Leaf(...)      // → 0 children
RenderObject::Single { ... } // → 1 child
RenderObject::Multi { ... }  // → N children
```

### 4. Object-Safe Traits ✅

```rust
// All traits are object-safe (no associated types in trait definition)
pub trait StatelessWidget: Debug + Send + Sync + 'static {
    fn build(&self, ctx: &BuildContext) -> Widget;  // ← Returns enum, not associated type
    fn clone_boxed(&self) -> Box<dyn StatelessWidget>;
    fn as_any(&self) -> &dyn Any;
}

pub trait LeafRenderObject: Debug + Send + Sync + 'static {
    fn layout(&mut self, constraints: BoxConstraints) -> Size;
    fn paint(&self, size: Size) -> BoxedLayer;
    fn clone_boxed(&self) -> Box<dyn LeafRenderObject>;
    fn as_any(&self) -> &dyn Any;
}
```

## 📝 Complete Example

```rust
// ============================================================================
// 1. Define a Stateless Widget
// ============================================================================

#[derive(Debug, Clone)]
struct HelloWorld {
    name: String,
}

impl StatelessWidget for HelloWorld {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        // Return a RenderObject widget
        Widget::render_object(Text::new(format!("Hello, {}!", self.name)))
    }

    fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// 2. Define a RenderObject Widget
// ============================================================================

#[derive(Debug, Clone)]
struct Text {
    text: String,
    style: TextStyle,
}

impl Text {
    fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::default(),
        }
    }
}

impl RenderObjectWidget for Text {
    fn create_render_object(&self, _ctx: &BuildContext) -> Box<dyn DynRenderObject> {
        // In new architecture, this would return RenderObject enum:
        // RenderObject::leaf(RenderText { ... })
        Box::new(RenderText {
            text: self.text.clone(),
            font_size: self.style.font_size,
            color: self.style.color,
        })
    }

    fn update_render_object(&self, _ctx: &BuildContext, ro: &mut dyn DynRenderObject) {
        if let Some(render_text) = ro.as_any_mut().downcast_mut::<RenderText>() {
            render_text.text = self.text.clone();
            render_text.font_size = self.style.font_size;
        }
    }

    fn clone_boxed(&self) -> Box<dyn RenderObjectWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// 3. Define a Leaf RenderObject
// ============================================================================

#[derive(Debug, Clone)]
struct RenderText {
    text: String,
    font_size: f32,
    color: Color,
}

impl LeafRenderObject for RenderText {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let width = self.text.len() as f32 * self.font_size * 0.6;
        let height = self.font_size * 1.2;
        constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, size: Size) -> BoxedLayer {
        let mut layer = PictureLayer::new();
        layer.draw_text(
            Rect::from_size(size),
            &self.text,
            self.font_size,
            self.color,
        );
        Box::new(layer)
    }

    fn clone_boxed(&self) -> Box<dyn LeafRenderObject> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// 4. Usage
// ============================================================================

fn main() {
    // Create widget
    let widget = Widget::stateless(HelloWorld {
        name: "Flui".into(),
    });

    // Pattern match to get concrete type
    if let Widget::Stateless(stateless) = &widget {
        // Downcast if needed
        if let Some(hello) = stateless.as_any().downcast_ref::<HelloWorld>() {
            println!("Name: {}", hello.name);
        }
    }

    // Element tree would hold this
    // Element::Component(ComponentElement::new(widget))
}
```

## 🎨 Complete Type Hierarchy

```
Widget Enum
├─ Stateless(Box<dyn StatelessWidget>)
│  └─ Examples: HelloWorld, UserProfile, ProductCard
├─ Stateful(Box<dyn StatefulWidget>)
│  └─ Examples: Counter, Form, AnimationController
├─ Inherited(Box<dyn InheritedWidget>)
│  └─ Examples: Theme, MediaQuery, Directionality
├─ RenderObject(Box<dyn RenderObjectWidget>)
│  └─ Examples: Text, Container, CustomPaint
└─ ParentData(Box<dyn ParentDataWidget>)
   └─ Examples: Positioned, Flexible, TableCell

Element Enum
├─ Component(ComponentElement)
│  └─ For StatelessWidget (builds child widget)
├─ Stateful(StatefulElement)
│  └─ For StatefulWidget (manages State object)
├─ Inherited(InheritedElement)
│  └─ For InheritedWidget (propagates data)
├─ Render(RenderElement)
│  └─ For RenderObjectWidget (creates RenderObject)
└─ ParentData(ParentDataElement)
   └─ For ParentDataWidget (modifies parent data)

RenderObject Enum
├─ Leaf(Box<dyn LeafRenderObject>)
│  └─ Examples: RenderText, RenderImage, RenderCustomPaint
├─ Single { render, child }
│  └─ Examples: RenderOpacity, RenderTransform, RenderPadding
└─ Multi { render, children }
   └─ Examples: RenderFlex, RenderStack, RenderWrap
```

## ✅ Benefits Summary

### Architectural Benefits
1. ✅ **Perfect Symmetry** - Widget, Element, RenderObject all enums
2. ✅ **No Blanket Impl Conflicts** - Enums don't have coherence issues
3. ✅ **Exhaustive Matching** - Compiler guarantees all cases handled
4. ✅ **Type-Level Arity** - Child count encoded in enum variants

### Developer Experience
5. ✅ **Clear Semantics** - Variant names show purpose
6. ✅ **Easy Navigation** - Pattern match to access children
7. ✅ **Simple Downcast** - Built into enum methods
8. ✅ **Consistent API** - Same pattern for all three trees

### Performance
9. ✅ **Enum Optimization** - Compiler optimizes match statements
10. ✅ **Cache Friendly** - Enum variants are contiguous in memory
11. ✅ **No vtable Double-Indirection** - One Box, one vtable

### Rust-ness
12. ✅ **Idiomatic Rust** - Enums are the Rust way for sum types
13. ✅ **Object-Safe Traits** - Can use Box<dyn Trait>
14. ✅ **No Coherence Hacks** - Clean, simple design

## 🚀 Migration Strategy

### Phase 1: Widget Enum (DONE ✅)
- [x] Create Widget enum
- [x] Create object-safe widget traits
- [x] Add examples and tests

### Phase 2: RenderObject Enum (NEXT)
- [ ] Create RenderObject enum
- [ ] Create Leaf/Single/Multi traits
- [ ] Migrate existing render objects
- [ ] Update RenderElement to use enum

### Phase 3: Integration
- [ ] Update Element enum to work with both
- [ ] Update examples to use new architecture
- [ ] Performance testing and optimization
- [ ] Documentation and migration guide

### Phase 4: Cleanup
- [ ] Remove deprecated traits
- [ ] Remove old blanket impls
- [ ] Final testing and release

## 📚 Comparison with Other Frameworks

| Framework | Widget Type | Element Type | Render Type |
|-----------|------------|--------------|-------------|
| **Flui (New)** | Enum | Enum | Enum |
| Flutter | Class Hierarchy | Class Hierarchy | Class Hierarchy |
| Xilem | Single Trait | N/A | N/A |
| Dioxus | Enum (VNode) | N/A | N/A |
| Yew | Enum (VNode) | N/A | N/A |

Flui's three-enum architecture is unique - combining:
- **Enum benefits** (like Dioxus/Yew)
- **Three-tree separation** (like Flutter)
- **Object-safe traits** (like Xilem)

Best of all worlds! 🎯

## 🎓 Conclusion

The enum architecture solves the fundamental coherence problem while providing:
- Clean, idiomatic Rust code
- Perfect architectural symmetry
- Excellent performance characteristics
- Great developer experience

This is the **Rust Way** to build a UI framework! 🦀

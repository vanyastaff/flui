# RenderObject Enum Design: Deep Dive

## 🎯 Текущая реализация (Associated Type Arity)

### Структура:

```rust
pub trait RenderObject: Send + Sync + Sized + 'static {
    type Arity: Arity;  // ← LeafArity | SingleArity | MultiArity

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size;
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer;
}
```

### Проблемы:

#### 1. ❌ Not Object-Safe

```rust
// ❌ НЕ РАБОТАЕТ!
let render: Box<dyn RenderObject> = Box::new(RenderParagraph { ... });
//          ^^^^^^^^^^^^^^^^^^^
// Error: `RenderObject` cannot be made into an object
// because it has generic type parameters (associated type)
```

**Причина:** Associated type `Arity` делает trait не object-safe.

#### 2. ❌ Type Erasure требует обёртки

```rust
// Нужен DynRenderObject!
pub enum DynRenderObject {
    Leaf(Box<dyn RenderObject<Arity = LeafArity>>),
    Single(Box<dyn RenderObject<Arity = SingleArity>>),
    Multi(Box<dyn RenderObject<Arity = MultiArity>>),
}

// Теперь можем хранить в дереве
pub struct RenderObjectNode {
    render: DynRenderObject, // ← Обёртка
    children: Vec<RenderObjectNode>,
}
```

**Проблема:** Дополнительный уровень индирекции.

#### 3. ❌ Невозможно хранить children в RenderObject

```rust
pub struct RenderOpacity {
    pub opacity: f32,
    // ❌ Где хранить child?
    // Нельзя: pub child: Box<dyn RenderObject>
}
```

**Решение:** Хранить в отдельной структуре (RenderObjectNode), но это усложняет код.

#### 4. ❌ Сложный API для пользователей

```rust
// Пользователь должен понимать Arity
impl RenderObject for MyWidget {
    type Arity = ???; // LeafArity or SingleArity or MultiArity?

    // И работать с типизированными контекстами
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // ...
    }
}
```

---

## 🚀 Новый дизайн: Enum-Based RenderObject

### Ключевая идея:

**RenderObject SAM - это enum, который содержит trait objects**

```rust
pub enum RenderObject {
    /// Leaf - нет детей
    Leaf(Box<dyn LeafRenderObject>),

    /// Single - один ребёнок
    Single {
        render: Box<dyn SingleChildRenderObject>,
        child: Box<RenderObject>, // ← Ребёнок внутри!
    },

    /// Multi - несколько детей
    Multi {
        render: Box<dyn MultiChildRenderObject>,
        children: Vec<RenderObject>, // ← Дети внутри!
    },
}
```

### Object-Safe Traits:

```rust
/// Trait для leaf render objects (нет детей)
pub trait LeafRenderObject: Debug + Send + Sync {
    fn layout(&mut self, constraints: BoxConstraints) -> Size;
    fn paint(&self, size: Size, offset: Offset) -> BoxedLayer;
}

/// Trait для single-child render objects
pub trait SingleChildRenderObject: Debug + Send + Sync {
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        child: &mut RenderObject,
    ) -> Size;

    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layer: BoxedLayer,
    ) -> BoxedLayer;
}

/// Trait для multi-child render objects
pub trait MultiChildRenderObject: Debug + Send + Sync {
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        children: &mut [RenderObject],
    ) -> Size;

    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layers: Vec<BoxedLayer>,
    ) -> BoxedLayer;
}
```

---

## 📊 Сравнение подходов

| Критерий | Associated Type | Enum-Based |
|----------|-----------------|------------|
| **Object-safe** | ❌ Нет | ✅ Да |
| **Type erasure** | ❌ Нужен DynRenderObject | ✅ Встроенный |
| **Children storage** | ❌ Отдельная структура | ✅ В enum |
| **Pattern matching** | ❌ Через downcast | ✅ Нативный match |
| **API простота** | ❌ Сложный (Arity) | ✅ Простой (traits) |
| **Compile-time checks** | ✅ Да | ✅ Да (enum) |
| **Zero-cost** | ✅ Да | 🟡 Почти (1 match) |
| **Extensibility** | ✅ Легко | 🟡 Добавить вариант |

---

## 💡 Детальный дизайн Enum-Based

### 1. Core Enum

```rust
#[derive(Debug)]
pub enum RenderObject {
    /// Leaf render object (no children)
    ///
    /// Examples: Text, Image, Placeholder
    Leaf(Box<dyn LeafRenderObject>),

    /// Single-child render object
    ///
    /// Examples: Opacity, Transform, ClipRect, Padding
    Single {
        render: Box<dyn SingleChildRenderObject>,
        child: Box<RenderObject>,
    },

    /// Multi-child render object
    ///
    /// Examples: Row, Column, Stack, Flex
    Multi {
        render: Box<dyn MultiChildRenderObject>,
        children: Vec<RenderObject>,
    },
}
```

### 2. Traits

```rust
/// Leaf RenderObject trait
///
/// For render objects with no children (Text, Image, etc.)
pub trait LeafRenderObject: Debug + Send + Sync {
    /// Compute layout
    ///
    /// # Returns
    /// Size that satisfies the constraints
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// # Arguments
    /// * `size` - The size computed by layout
    /// * `offset` - The offset to paint at
    ///
    /// # Returns
    /// A layer representing the visual output
    fn paint(&self, size: Size, offset: Offset) -> BoxedLayer;

    /// Optional: hit testing
    fn hit_test(&self, size: Size, position: Offset) -> bool {
        Rect::from_origin_size(Offset::ZERO, size).contains(position)
    }
}

/// Single-child RenderObject trait
///
/// For render objects with exactly one child (Opacity, Transform, etc.)
pub trait SingleChildRenderObject: Debug + Send + Sync {
    /// Compute layout
    ///
    /// # Arguments
    /// * `constraints` - Parent constraints
    /// * `child` - Mutable reference to child (can call child.layout())
    ///
    /// # Returns
    /// Size that satisfies the constraints
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        child: &mut RenderObject,
    ) -> Size;

    /// Paint this render object
    ///
    /// # Arguments
    /// * `size` - The size computed by layout
    /// * `offset` - The offset to paint at
    /// * `child_layer` - The layer from painting the child
    ///
    /// # Returns
    /// A layer representing the visual output (often wraps child_layer)
    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layer: BoxedLayer,
    ) -> BoxedLayer;

    /// Optional: hit testing
    fn hit_test(&self, size: Size, position: Offset, child: &RenderObject) -> bool {
        // Default: delegate to child
        child.hit_test(position)
    }
}

/// Multi-child RenderObject trait
///
/// For render objects with multiple children (Row, Column, Stack, etc.)
pub trait MultiChildRenderObject: Debug + Send + Sync {
    /// Compute layout
    ///
    /// # Arguments
    /// * `constraints` - Parent constraints
    /// * `children` - Mutable slice of children (can call child.layout())
    ///
    /// # Returns
    /// Size that satisfies the constraints
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        children: &mut [RenderObject],
    ) -> Size;

    /// Paint this render object
    ///
    /// # Arguments
    /// * `size` - The size computed by layout
    /// * `offset` - The offset to paint at
    /// * `child_layers` - Layers from painting children
    ///
    /// # Returns
    /// A layer representing the visual output
    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layers: Vec<BoxedLayer>,
    ) -> BoxedLayer;

    /// Optional: hit testing
    fn hit_test(
        &self,
        size: Size,
        position: Offset,
        children: &[RenderObject],
    ) -> bool {
        // Default: test all children
        children.iter().any(|child| child.hit_test(position))
    }
}
```

### 3. RenderObject Implementation

```rust
impl RenderObject {
    /// Create a leaf render object
    pub fn leaf(render: impl LeafRenderObject + 'static) -> Self {
        RenderObject::Leaf(Box::new(render))
    }

    /// Create a single-child render object
    pub fn single(
        render: impl SingleChildRenderObject + 'static,
        child: RenderObject,
    ) -> Self {
        RenderObject::Single {
            render: Box::new(render),
            child: Box::new(child),
        }
    }

    /// Create a multi-child render object
    pub fn multi(
        render: impl MultiChildRenderObject + 'static,
        children: Vec<RenderObject>,
    ) -> Self {
        RenderObject::Multi {
            render: Box::new(render),
            children,
        }
    }

    /// Perform layout
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        match self {
            RenderObject::Leaf(render) => render.layout(constraints),

            RenderObject::Single { render, child } => {
                render.layout(constraints, child)
            }

            RenderObject::Multi { render, children } => {
                render.layout(constraints, children)
            }
        }
    }

    /// Paint this render object
    pub fn paint(&self, size: Size, offset: Offset) -> BoxedLayer {
        match self {
            RenderObject::Leaf(render) => render.paint(size, offset),

            RenderObject::Single { render, child } => {
                // First paint child
                let child_size = child.size(); // Need to store size from layout
                let child_layer = child.paint(child_size, offset);

                // Then wrap with parent's paint
                render.paint(size, offset, child_layer)
            }

            RenderObject::Multi { render, children } => {
                // Paint all children
                let child_layers: Vec<_> = children
                    .iter()
                    .map(|child| {
                        let child_size = child.size();
                        child.paint(child_size, offset)
                    })
                    .collect();

                // Composite in parent
                render.paint(size, offset, child_layers)
            }
        }
    }

    /// Hit test
    pub fn hit_test(&self, position: Offset) -> bool {
        match self {
            RenderObject::Leaf(render) => {
                render.hit_test(self.size(), position)
            }

            RenderObject::Single { render, child } => {
                render.hit_test(self.size(), position, child)
            }

            RenderObject::Multi { render, children } => {
                render.hit_test(self.size(), position, children)
            }
        }
    }

    /// Get the size (cached from layout)
    pub fn size(&self) -> Size {
        // TODO: Store size in enum?
        // Or in wrapper struct?
        todo!()
    }
}
```

---

## 🎨 Example Implementations

### Example 1: RenderParagraph (Leaf)

```rust
use flui_engine::PictureLayer;

#[derive(Debug)]
pub struct RenderParagraph {
    pub text: String,
    pub font_size: f32,
}

impl LeafRenderObject for RenderParagraph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Calculate text size
        let width = self.text.len() as f32 * self.font_size * 0.6;
        let height = self.font_size * 1.2;

        constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, size: Size, offset: Offset) -> BoxedLayer {
        let mut picture = PictureLayer::new();

        picture.draw_text(
            Rect::from_xywh(
                offset.x,
                offset.y,
                size.width,
                size.height,
            ),
            &self.text,
            self.font_size,
            Paint::default(),
        );

        Box::new(picture)
    }
}

// Usage:
let render = RenderObject::leaf(RenderParagraph {
    text: "Hello".to_string(),
    font_size: 16.0,
});
```

### Example 2: RenderOpacity (Single)

```rust
use flui_engine::OpacityLayer;

#[derive(Debug)]
pub struct RenderOpacity {
    pub opacity: f32,
}

impl SingleChildRenderObject for RenderOpacity {
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        child: &mut RenderObject,
    ) -> Size {
        // Just delegate to child
        child.layout(constraints)
    }

    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layer: BoxedLayer,
    ) -> BoxedLayer {
        // Wrap child layer with opacity
        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }
}

// Usage:
let render = RenderObject::single(
    RenderOpacity { opacity: 0.5 },
    RenderObject::leaf(RenderParagraph {
        text: "Faded".to_string(),
        font_size: 16.0,
    }),
);
```

### Example 3: RenderFlex (Multi)

```rust
use flui_engine::ContainerLayer;

#[derive(Debug)]
pub struct RenderFlex {
    pub spacing: f32,
    pub direction: Axis,
}

impl MultiChildRenderObject for RenderFlex {
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        children: &mut [RenderObject],
    ) -> Size {
        let mut total_main = 0.0;
        let mut max_cross = 0.0;

        for child in children.iter_mut() {
            // Layout each child
            let child_size = child.layout(constraints);

            match self.direction {
                Axis::Horizontal => {
                    total_main += child_size.width + self.spacing;
                    max_cross = max_cross.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main += child_size.height + self.spacing;
                    max_cross = max_cross.max(child_size.width);
                }
            }
        }

        // Remove last spacing
        if !children.is_empty() {
            total_main -= self.spacing;
        }

        match self.direction {
            Axis::Horizontal => Size::new(total_main, max_cross),
            Axis::Vertical => Size::new(max_cross, total_main),
        }
    }

    fn paint(
        &self,
        size: Size,
        offset: Offset,
        child_layers: Vec<BoxedLayer>,
    ) -> BoxedLayer {
        let mut container = ContainerLayer::new();

        let mut current_offset = offset;

        for layer in child_layers {
            // Position child layer
            container.add_child_at(layer, current_offset);

            // Move offset for next child
            match self.direction {
                Axis::Horizontal => {
                    current_offset.x += layer.size().width + self.spacing;
                }
                Axis::Vertical => {
                    current_offset.y += layer.size().height + self.spacing;
                }
            }
        }

        Box::new(container)
    }
}

// Usage:
let render = RenderObject::multi(
    RenderFlex {
        spacing: 10.0,
        direction: Axis::Vertical,
    },
    vec![
        RenderObject::leaf(RenderParagraph {
            text: "First".to_string(),
            font_size: 16.0,
        }),
        RenderObject::leaf(RenderParagraph {
            text: "Second".to_string(),
            font_size: 16.0,
        }),
    ],
);
```

---

## 🤔 Потенциальные проблемы и решения

### Проблема 1: Хранение размера

**Вопрос:** Где хранить `Size` после layout?

**Решение A:** В enum

```rust
pub enum RenderObject {
    Leaf {
        render: Box<dyn LeafRenderObject>,
        size: Cell<Option<Size>>, // ← Кэшируем
    },
    Single {
        render: Box<dyn SingleChildRenderObject>,
        child: Box<RenderObject>,
        size: Cell<Option<Size>>,
    },
    Multi {
        render: Box<dyn MultiChildRenderObject>,
        children: Vec<RenderObject>,
        size: Cell<Option<Size>>,
    },
}
```

**Решение B:** В обёртке

```rust
pub struct RenderObjectNode {
    render: RenderObject,
    size: Cell<Option<Size>>,
    offset: Cell<Offset>,
}
```

**Рекомендация:** Решение B (чище, enum проще)

---

### Проблема 2: Offset для детей

**Вопрос:** Как передать offset детям при paint?

**Решение:** RenderObject должен знать свой offset

```rust
pub struct RenderObjectNode {
    render: RenderObject,
    layout_data: Cell<LayoutData>,
}

struct LayoutData {
    size: Size,
    offset: Offset,
    // Возможно, другие данные layout
}
```

---

### Проблема 3: Extensibility

**Вопрос:** Что если нужен render object с динамическим количеством детей?

**Ответ:** Multi уже поддерживает `Vec<RenderObject>` - это динамическое количество!

**Вопрос:** Что если нужен новый тип (не Leaf/Single/Multi)?

**Ответ:** Можно добавить вариант в enum:

```rust
pub enum RenderObject {
    Leaf(Box<dyn LeafRenderObject>),
    Single { /* ... */ },
    Multi { /* ... */ },

    // Новый вариант
    Custom {
        render: Box<dyn CustomRenderObject>,
        data: CustomData,
    },
}
```

Но это breaking change. Альтернатива - использовать Multi с custom logic.

---

## ✅ Преимущества Enum-Based

### 1. ✅ Простой API

```rust
// Было (Associated Type):
impl RenderObject for MyRender {
    type Arity = LeafArity; // ← Нужно понимать Arity
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size { /* ... */ }
}

// Стало (Enum):
impl LeafRenderObject for MyRender {
    fn layout(&mut self, constraints: BoxConstraints) -> Size { /* ... */ }
}
// ← Просто impl нужный trait!
```

### 2. ✅ Pattern Matching

```rust
// Было:
match dyn_render {
    DynRenderObject::Leaf(r) => r.layout(...),
    DynRenderObject::Single(r) => r.layout(...),
    DynRenderObject::Multi(r) => r.layout(...),
}

// Стало:
match render {
    RenderObject::Leaf(r) => r.layout(...),
    RenderObject::Single { render: r, child } => r.layout(..., child),
    RenderObject::Multi { render: r, children } => r.layout(..., children),
}
// ← Дети доступны сразу!
```

### 3. ✅ Type Safety

```rust
// Компилятор проверяет exhaustiveness
match render {
    RenderObject::Leaf(_) => {},
    RenderObject::Single { .. } => {},
    // ❌ Missing Multi variant - compile error!
}
```

### 4. ✅ Консистентность с Widget/Element

```rust
// Вся архитектура - enum!
pub enum Widget { /* ... */ }
pub enum Element { /* ... */ }
pub enum RenderObject { /* ... */ }

// Единообразный паттерн
```

---

## ❌ Недостатки Enum-Based

### 1. ❌ Один уровень indirection

```rust
// Associated Type (теоретически zero-cost):
struct RenderOpacity {
    opacity: f32,
    // Нет child здесь
}

// Enum (one Box):
RenderObject::Single {
    render: Box<dyn SingleChildRenderObject>, // ← Box
    child: Box<RenderObject>,                 // ← Box
}
```

**Оценка:** Negligible на практике (один match + два dereference).

### 2. ❌ Сложнее добавить новый вариант

**Проблема:** Добавление нового варианта в enum - breaking change.

**Решение:** Редко нужно. Leaf/Single/Multi покрывают 99% случаев.

### 3. ❌ Нельзя impl методы на concrete types

```rust
// Associated Type:
impl RenderOpacity {
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity;
    }
}

// Enum:
// RenderOpacity внутри Box<dyn SingleChildRenderObject>
// Нужен downcast для доступа
```

**Решение:** Использовать trait методы или getters/setters в trait.

---

## 🎯 Рекомендация

**Используем Enum-Based дизайн!**

### Почему:

1. ✅ **Object-safe** - не нужен DynRenderObject
2. ✅ **Простой API** - понятные traits
3. ✅ **Children встроены** - в enum variants
4. ✅ **Pattern matching** - exhaustive checks
5. ✅ **Консистентность** - Widget/Element/RenderObject все enum
6. ✅ **Практичность** - минимальный overhead

### План миграции:

1. ✅ Определить новые traits (LeafRenderObject, etc)
2. ✅ Создать enum RenderObject
3. ✅ Реализовать базовые render objects (Paragraph, Opacity, Flex)
4. ✅ Интегрировать с Element
5. ✅ Тесты и benchmarks

**Это правильный путь! 🚀**

# Решение БЕЗ RenderContext

## Ключевая Идея (от пользователя)

`MultiChildRenderObjectWidget` уже имеет метод:
```rust
fn children(&self) -> &[Box<dyn DynWidget>];
```

Это значит:
1. **Widget знает своих детей** (Widget children)
2. **Element создаёт детей** через `widget.children()` → child Elements
3. **Element может собрать child RenderObjects** и передать их в свой RenderObject

## Новый Подход: Element как Посредник

Вместо того чтобы RenderObject искал детей, **Element предоставляет доступ к детям**.

### Вариант 1: Callback-based API

```rust
pub trait DynRenderObject {
    /// Layout with access to children via callback
    fn layout<F>(
        &mut self,
        constraints: BoxConstraints,
        for_each_child: F,
    ) -> Size
    where
        F: FnMut(ElementId, &mut dyn DynRenderObject);
}

// RenderFlex использует:
impl DynRenderObject for RenderFlex {
    fn layout<F>(&mut self, constraints: BoxConstraints, mut for_each_child: F) -> Size
    where
        F: FnMut(ElementId, &mut dyn DynRenderObject),
    {
        let mut total_size = 0.0;

        // Вызов callback для каждого ребёнка
        for_each_child(|child_id, child_ro| {
            let child_size = child_ro.layout(child_constraints, ...);
            total_size += child_size.height;
        });

        Size::new(constraints.max_width, total_size)
    }
}

// Element вызывает:
impl MultiChildRenderObjectElement {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let tree = self.tree.as_ref().unwrap();

        // Callback который предоставляет доступ к каждому ребёнку
        let for_each_child = |mut f: &mut dyn FnMut(ElementId, &mut dyn DynRenderObject)| {
            for &child_id in &self.children {
                let mut tree_guard = tree.write();
                if let Some(child_elem) = tree_guard.get_mut(child_id) {
                    if let Some(child_ro) = child_elem.render_object_mut() {
                        f(child_id, child_ro);
                    }
                }
            }
        };

        self.render_object.as_mut().unwrap().layout(constraints, for_each_child)
    }
}
```

**Проблема:** Сложные generic constraints, рекурсия сложная.

### Вариант 2: Передать список ElementIds + accessor функцию

```rust
pub struct ChildrenAccessor<'a> {
    child_ids: &'a [ElementId],
    tree: Arc<RwLock<ElementTree>>,
}

impl<'a> ChildrenAccessor<'a> {
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(ElementId, BoxConstraints) -> Size,
    {
        for &child_id in self.child_ids {
            // Element tree layouts the child and returns size
            let mut tree = self.tree.write();
            if let Some(size) = tree.layout_element(child_id, constraints) {
                f(child_id, size);
            }
        }
    }
}

pub trait DynRenderObject {
    fn layout(&mut self, constraints: BoxConstraints, children: ChildrenAccessor) -> Size;
}

// RenderFlex:
impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints, children: ChildrenAccessor) -> Size {
        let mut total_size = 0.0;

        children.for_each(|child_id, child_constraints| {
            let child_size = // как вызвать child.layout()?
        });

        Size::new(constraints.max_width, total_size)
    }
}
```

**Проблема:** Всё ещё нужен способ вызвать `child.layout()`.

### Вариант 3: **Element выполняет layout детей ДО вызова parent.layout()**

Это самый простой подход!

```rust
pub trait DynRenderObject {
    /// Layout with pre-computed child sizes
    fn layout(&mut self, constraints: BoxConstraints, child_sizes: &[Size]) -> Size;
}

// RenderFlex:
impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints, child_sizes: &[Size]) -> Size {
        // Дети уже layout'нуты! Просто используем их размеры
        let mut total_size = 0.0;
        for &child_size in child_sizes {
            total_size += child_size.height;
        }
        Size::new(constraints.max_width, total_size)
    }
}

// Element:
impl MultiChildRenderObjectElement {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // 1. Layout всех детей СНАЧАЛА
        let mut child_sizes = Vec::new();
        for &child_id in &self.children {
            let size = self.tree.write().layout_element(child_id, child_constraints);
            child_sizes.push(size);
        }

        // 2. Затем layout parent с уже известными размерами детей
        self.render_object.as_mut().unwrap().layout(constraints, &child_sizes)
    }
}
```

**Проблема:** Слишком упрощённо. Parent нужен контроль над constraints для детей.

### Вариант 4: **Двухфазный layout через LayoutProtocol**

Самый гибкий подход:

```rust
/// Protocol для layout детей
pub trait LayoutProtocol {
    /// Request layout for a child with specific constraints
    fn layout_child(&mut self, child_id: ElementId, constraints: BoxConstraints) -> Size;
}

pub trait DynRenderObject {
    fn layout(&mut self, constraints: BoxConstraints, protocol: &mut dyn LayoutProtocol) -> Size;
}

// RenderFlex:
impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints, protocol: &mut dyn LayoutProtocol) -> Size {
        let mut total_size = 0.0;

        // Parent контролирует constraints для каждого ребёнка
        for child_id in &self.child_ids {  // Откуда берутся child_ids?
            let child_constraints = BoxConstraints::tight_for(
                constraints.max_width,
                f32::INFINITY,
            );

            // Запрашиваем layout через protocol
            let child_size = protocol.layout_child(child_id, child_constraints);
            total_size += child_size.height;
        }

        Size::new(constraints.max_width, total_size)
    }
}

// Element реализует protocol:
impl LayoutProtocol for MultiChildRenderObjectElement {
    fn layout_child(&mut self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // Element вызывает layout на child Element
        self.tree.write().layout_element(child_id, constraints)
    }
}
```

**Проблема:** RenderObject не знает child_ids! Ему нужен список детей.

### Вариант 5: **RenderObject хранит child_ids** ✅

ЛУЧШЕЕ РЕШЕНИЕ:

```rust
pub struct ContainerRenderBox<T> {
    pub data: T,
    pub state: RenderState,
    pub child_ids: Vec<ElementId>,  // ← Вместо Vec<BoxedRenderObject>!
}

/// Protocol для layout/paint детей
pub trait RenderProtocol {
    fn layout_child(&mut self, child_id: ElementId, constraints: BoxConstraints) -> Size;
    fn paint_child(&self, child_id: ElementId, painter: &egui::Painter, offset: Offset);
}

pub trait DynRenderObject {
    fn layout(&mut self, constraints: BoxConstraints, protocol: &mut dyn RenderProtocol) -> Size;
    fn paint(&self, painter: &egui::Painter, offset: Offset, protocol: &dyn RenderProtocol);
}

// RenderFlex:
impl DynRenderObject for ContainerRenderBox<FlexData> {
    fn layout(&mut self, constraints: BoxConstraints, protocol: &mut dyn RenderProtocol) -> Size {
        let mut total_size = 0.0;

        // RenderFlex знает своих детей через child_ids
        for &child_id in &self.child_ids {
            let child_constraints = BoxConstraints::tight_for(
                constraints.max_width,
                f32::INFINITY,
            );

            // Layout через protocol
            let child_size = protocol.layout_child(child_id, child_constraints);
            total_size += child_size.height;
        }

        Size::new(constraints.max_width, total_size)
    }
}

// Element реализует protocol:
impl RenderProtocol for MultiChildRenderObjectElement {
    fn layout_child(&mut self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        let mut tree = self.tree.as_ref().unwrap().write();
        tree.layout_element(child_id, constraints)
    }

    fn paint_child(&self, child_id: ElementId, painter: &egui::Painter, offset: Offset) {
        let tree = self.tree.as_ref().unwrap().read();
        if let Some(child_elem) = tree.get(child_id) {
            if let Some(child_ro) = child_elem.render_object() {
                child_ro.paint(painter, offset, self); // Рекурсия!
            }
        }
    }
}

// Заполнение child_ids:
impl MultiChildRenderObjectElement {
    fn rebuild(&mut self) {
        // После создания child Elements, populate child_ids в RenderObject
        if let Some(render_object) = self.render_object.as_mut() {
            if let Some(container) = render_object.downcast_mut::<ContainerRenderBox<FlexData>>() {
                container.child_ids = self.children.to_vec();
            }
        }
    }
}
```

## Преимущества Варианта 5

✅ **Нет RenderContext** - используется protocol pattern
✅ **RenderObject владеет child_ids** - не RenderObjects, а IDs
✅ **Element предоставляет доступ** через RenderProtocol
✅ **Чистая архитектура** - явные зависимости
✅ **Нет borrow checker проблем** - protocol берёт tree по требованию
✅ **Гибкость** - parent контролирует constraints
✅ **Testable** - можно mock RenderProtocol

## Сравнение

| Подход | RenderObject хранит | Layout доступ к детям | Borrow checker |
|--------|---------------------|----------------------|----------------|
| Текущий (adopt_child) | `Vec<BoxedRenderObject>` | Прямой доступ | ❌ Нарушает владение |
| RenderContext | Ничего | `ctx.tree.get(child_id)` | ✅ Работает |
| **RenderProtocol** | `Vec<ElementId>` | `protocol.layout_child(id)` | ✅ Работает |

## Итог

**RenderProtocol - это правильное решение БЕЗ RenderContext!**

Это тот же концепт, но:
- Вместо `RenderContext` с полями → `RenderProtocol` с методами
- Вместо `ctx.tree.get()` → `protocol.layout_child()`
- Element реализует protocol и предоставляет доступ

По сути это **dependency injection через trait**, а не через struct.

# Element-Driven Layout: Rust-Way Решение

## Ключевая Идея

**Element управляет процессом layout/paint, а не RenderObject!**

## Сравнение Подходов

### ❌ Текущий подход (RenderObject-driven):

```rust
// RenderObject управляет и ищет детей
impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Проблема: как найти детей?
        for child in &self.children {  // children пустой!
            child.layout(...)
        }
    }
}
```

### ✅ Новый подход (Element-driven):

```rust
// Element управляет процессом!
impl MultiChildRenderObjectElement {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Element знает детей через self.children: Vec<ElementId>
        // Element имеет tree через self.tree: Arc<RwLock<ElementTree>>
        // Element владеет render_object: Option<Box<dyn DynRenderObject>>

        // 1. Element спрашивает у RenderObject как layout детей
        let layout_strategy = self.render_object.create_layout_strategy(constraints);

        // 2. Element делает layout детей используя tree
        let mut child_results = Vec::new();
        for (index, &child_id) in self.children.iter().enumerate() {
            let child_constraints = layout_strategy.constraints_for_child(index);

            // Element вызывает layout через tree
            let mut tree = self.tree.as_ref().unwrap().write();
            let child_size = tree.layout_element(child_id, child_constraints);

            child_results.push(ChildLayoutResult {
                index,
                size: child_size,
                id: child_id,
            });
        }

        // 3. Element передаёт результаты в RenderObject для вычисления финального размера
        let size = self.render_object.compute_size(constraints, &child_results);

        size
    }
}
```

## Преимущества

✅ **Element имеет всю информацию**
- Знает детей (`self.children`)
- Имеет tree (`self.tree`)
- Владеет RenderObject (`self.render_object`)

✅ **RenderObject остаётся чистой логикой**
- Не нужен доступ к tree
- Не нужен RenderContext/Protocol
- Просто вычислительная логика

✅ **Нет borrow checker проблем**
- Element может взять `tree.write()` когда нужно
- Не нужно передавать ссылки в RenderObject

✅ **Чёткое разделение ответственности**
- Element = координатор процесса
- RenderObject = логика layout

## Детальная Реализация

### 1. LayoutStrategy trait (вместо прямого layout)

```rust
/// Стратегия layout для multi-child RenderObject
pub trait LayoutStrategy {
    /// Constraints для конкретного ребёнка
    fn constraints_for_child(&self, index: usize) -> BoxConstraints;

    /// Вычислить итоговый размер на основе результатов layout детей
    fn compute_size(&self, parent_constraints: BoxConstraints, children: &[ChildLayoutResult]) -> Size;
}

pub struct ChildLayoutResult {
    pub index: usize,
    pub id: ElementId,
    pub size: Size,
}
```

### 2. DynRenderObject изменённый API

```rust
pub trait DynRenderObject {
    // Leaf RenderObject - простой layout
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.compute_intrinsic_size(constraints)
    }

    // Multi-child RenderObject - создаёт стратегию
    fn create_layout_strategy(&self, constraints: BoxConstraints) -> Box<dyn LayoutStrategy> {
        // Default: нет детей
        Box::new(NoChildrenStrategy)
    }

    // Paint тоже через callback от Element
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    // Paint child вызывается Element
    fn paint_child(&self, painter: &egui::Painter, offset: Offset, child_index: usize, child_size: Size) {
        // Вычислить offset для ребёнка
        // Element сам paint ребёнка в этом offset
    }
}
```

### 3. RenderFlex реализация

```rust
pub struct FlexLayoutStrategy {
    direction: Axis,
    main_axis_size: MainAxisSize,
    cross_axis_alignment: CrossAxisAlignment,
    parent_constraints: BoxConstraints,
}

impl LayoutStrategy for FlexLayoutStrategy {
    fn constraints_for_child(&self, index: usize) -> BoxConstraints {
        match self.direction {
            Axis::Vertical => BoxConstraints::new(
                0.0, self.parent_constraints.max_width,
                0.0, f32::INFINITY,
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0, f32::INFINITY,
                0.0, self.parent_constraints.max_height,
            ),
        }
    }

    fn compute_size(&self, parent_constraints: BoxConstraints, children: &[ChildLayoutResult]) -> Size {
        let mut total_main = 0.0;
        let mut max_cross = 0.0;

        for child in children {
            match self.direction {
                Axis::Vertical => {
                    total_main += child.size.height;
                    max_cross = max_cross.max(child.size.width);
                }
                Axis::Horizontal => {
                    total_main += child.size.width;
                    max_cross = max_cross.max(child.size.height);
                }
            }
        }

        match self.direction {
            Axis::Vertical => Size::new(max_cross, total_main),
            Axis::Horizontal => Size::new(total_main, max_cross),
        }
    }
}

impl DynRenderObject for ContainerRenderBox<FlexData> {
    fn create_layout_strategy(&self, constraints: BoxConstraints) -> Box<dyn LayoutStrategy> {
        Box::new(FlexLayoutStrategy {
            direction: self.data().direction,
            main_axis_size: self.data().main_axis_size,
            cross_axis_alignment: self.data().cross_axis_alignment,
            parent_constraints: constraints,
        })
    }
}
```

### 4. Element делает layout

```rust
impl<W: MultiChildRenderObjectWidget> MultiChildRenderObjectElement<W> {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let render_object = self.render_object.as_ref().unwrap();

        // Получить стратегию от RenderObject
        let strategy = render_object.create_layout_strategy(constraints);

        // Layout всех детей
        let mut child_results = Vec::new();
        for (index, &child_id) in self.children.iter().enumerate() {
            let child_constraints = strategy.constraints_for_child(index);

            // Вызвать layout через tree
            let child_size = self.layout_child(child_id, child_constraints);

            child_results.push(ChildLayoutResult {
                index,
                id: child_id,
                size: child_size,
            });
        }

        // Вычислить финальный размер
        let size = strategy.compute_size(constraints, &child_results);

        // Сохранить размер в RenderObject (через mut reference)
        let render_object_mut = self.render_object.as_mut().unwrap();
        render_object_mut.set_size(size);

        size
    }

    fn layout_child(&mut self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        let tree = self.tree.as_ref().unwrap();
        let mut tree_guard = tree.write();

        // Получить child element
        if let Some(child_elem) = tree_guard.get_mut(child_id) {
            // Если это RenderObjectElement, вызвать perform_layout
            if let Some(child_ro) = child_elem.render_object_mut() {
                return child_ro.layout(constraints);
            }
        }

        Size::ZERO
    }
}
```

## Почему это лучше чем RenderContext?

| Критерий | RenderContext | Element-Driven |
|----------|---------------|----------------|
| **API изменения** | Breaking change в `layout()` | Только внутри Element |
| **RenderObject доступ к tree** | Да (через ctx) | Нет (не нужен!) |
| **Кто координирует** | RenderObject | Element |
| **Borrow checker** | Сложнее (рекурсия с ctx) | Проще (Element владеет всем) |
| **Testability** | Mock RenderContext | Mock LayoutStrategy |
| **Rust-idiomatic** | Средне | ✅ Высоко |

## Почему это правильно для Rust?

1. **Ownership принцип**: Element владеет и RenderObject, и детьми - он и координирует
2. **Separation of Concerns**: RenderObject = чистая логика, Element = координация
3. **No context passing**: Нет необходимости передавать context через все методы
4. **Strategy Pattern**: LayoutStrategy - чистый Rust pattern, легко тестировать
5. **No global state**: Всё локально в Element

## Вывод

**Element-Driven Layout - это истинное Rust-way решение!**

Это не OOP подход (где объект всё делает сам), и не процедурный (где функции передают context).
Это **композиция + ownership + traits** - идиоматичный Rust подход.

Element имеет:
- Ownership RenderObject
- Знание о детях
- Доступ к tree

→ Element и должен управлять процессом!

RenderObject предоставляет:
- Логику вычисления constraints
- Логику вычисления размера
- Логику позиционирования

→ RenderObject остаётся чистой, тестируемой логикой!

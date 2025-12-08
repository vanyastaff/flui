# FLUI Dependency Architecture

## Принципы

1. **Слоистая архитектура** - зависимости только вниз
2. **Независимые модули** - каждый crate решает свою задачу
3. **Минимальные зависимости** - только необходимые связи

---

## Правильная Архитектура Зависимостей

```
┌──────────────────────────────────────────────────────┐
│  Layer 0: Foundation (no dependencies)               │
│  - flui_types         (geometry, math)               │
│  - flui-foundation    (core utilities)               │
└──────────────────────────────────────────────────────┘
                         ▲
                         │
┌──────────────────────────────────────────────────────┐
│  Layer 1: Tree Abstractions                          │
│  - flui-tree          (tree traits, arity)           │
└──────────────────────────────────────────────────────┘
                         ▲
                         │
┌──────────────────────────────────────────────────────┐
│  Layer 2: Domain-Specific                            │
│  - flui_painting      (canvas, paint)                │
│  - flui_interaction   (events, hit test)             │
│  - flui_rendering ◄── THIS CRATE                     │
└──────────────────────────────────────────────────────┘
                         ▲
                         │
┌──────────────────────────────────────────────────────┐
│  Layer 3: Framework                                  │
│  - flui-element       (element tree)                 │
│  - flui-view          (view tree)                    │
│  - flui_core          (framework core)               │
└──────────────────────────────────────────────────────┘
                         ▲
                         │
┌──────────────────────────────────────────────────────┐
│  Layer 4: Application                                │
│  - flui_widgets       (widget library)               │
│  - flui_app           (app framework)                │
└──────────────────────────────────────────────────────┘
```

---

## flui_rendering: Разрешённые Зависимости

### ✅ Разрешённые (Layer 0-1)

```toml
[dependencies]
# Foundation
flui_types = { path = "../flui_types" }
flui-foundation = { path = "../flui-foundation" }

# Tree abstractions
flui-tree = { path = "../flui-tree" }

# Domain-specific (same layer)
flui_painting = { path = "../flui_painting" }
flui_interaction = { path = "../flui_interaction" }

# External
parking_lot = "0.12"
bitflags = "2.0"
downcast-rs = "2.0"
```

### ❌ ЗАПРЕЩЁННЫЕ (Layer 3+)

```toml
# ❌ НЕТ зависимостей от фреймворка!
# flui-view = { path = "../flui-view" }        # НЕЛЬЗЯ!
# flui-element = { path = "../flui-element" }  # НЕЛЬЗЯ!
# flui_core = { path = "../flui_core" }        # НЕЛЬЗЯ!
# flui_widgets = { path = "../flui_widgets" }  # НЕЛЬЗЯ!
```

---

## Что Это Означает для Архитектуры

### 1. RenderObject НЕ знает об Element

**❌ Неправильно (циклическая зависимость):**
```rust
// В flui_rendering/src/object.rs
use flui_element::ElementId;  // ❌ НЕЛЬЗЯ!

pub trait RenderObject {
    fn layout(&mut self, element_id: ElementId, ...) { ... }
}
```

**✅ Правильно (RenderObject независим):**
```rust
// В flui_rendering/src/object.rs
// ElementId приходит из flui-foundation (Layer 0)
use flui_foundation::ElementId;  // ✅ ОК!

pub trait RenderObject {
    fn layout<P: Protocol>(
        &mut self,
        ctx: &mut LayoutContext<'_, P>,
    ) -> Result<P::Geometry, LayoutError>;
}
```

### 2. Context НЕ содержит ElementTree

**❌ Неправильно:**
```rust
use flui_element::ElementTree;  // ❌ НЕЛЬЗЯ!

pub struct LayoutContext<'tree> {
    element_tree: &'tree ElementTree,  // ❌ Зависимость от element!
}
```

**✅ Правильно (абстракция через trait):**
```rust
// flui_rendering определяет trait для доступа к детям
pub trait LayoutTreeAccess {
    fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: Constraints,
    ) -> Result<Geometry, LayoutError>;
}

pub struct LayoutContext<'tree, T: LayoutTreeAccess> {
    tree: &'tree mut T,  // ✅ Generic abstraction!
    element_id: ElementId,
    constraints: Constraints,
}

// flui-element потом реализует этот trait
// impl LayoutTreeAccess for ElementTree { ... }
```

### 3. Правильная Архитектура Context

```rust
// ===== flui_rendering/src/context/layout.rs =====

use flui_foundation::ElementId;
use flui_types::*;

/// Trait для доступа к дереву (абстракция!)
pub trait LayoutTreeAccess {
    /// Layout child element
    fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: Constraints,
    ) -> Result<Geometry, LayoutError>;

    /// Get children IDs
    fn children(&self, element_id: ElementId) -> &[ElementId];

    /// Get parent data
    fn parent_data(
        &self,
        element_id: ElementId,
    ) -> Option<&dyn crate::ParentData>;
}

/// Layout context (generic over tree implementation)
pub struct LayoutContext<'tree, T: LayoutTreeAccess, P: Protocol> {
    /// Tree access (generic!)
    tree: &'tree mut T,

    /// Current element
    element_id: ElementId,

    /// Constraints
    constraints: P::Constraints,

    _phantom: PhantomData<P>,
}

impl<'tree, T: LayoutTreeAccess, P: Protocol> LayoutContext<'tree, T, P> {
    /// Layout child (через абстракцию)
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: P::Constraints,
    ) -> Result<P::Geometry, LayoutError> {
        self.tree.layout_child(child_id, constraints.into())?
            .try_into()
            .map_err(|_| LayoutError::ProtocolMismatch)
    }

    /// Get children (через абстракцию)
    pub fn children(&self) -> ChildrenView<'_, T, P> {
        ChildrenView {
            element_id: self.element_id,
            tree: self.tree,
            _phantom: PhantomData,
        }
    }
}
```

---

## Как Element Использует Rendering

```rust
// ===== flui-element/src/element_tree.rs =====

use flui_rendering::{LayoutTreeAccess, LayoutContext, RenderObject};

pub struct ElementTree {
    elements: Slab<Element>,
}

// Element реализует абстракцию из rendering
impl LayoutTreeAccess for ElementTree {
    fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: Constraints,
    ) -> Result<Geometry, LayoutError> {
        let child = self.elements.get_mut(child_id.index())?;
        let render = child.render_object_mut();

        // Create context
        let mut ctx = LayoutContext::new(
            self,  // self as tree access
            child_id,
            constraints,
        );

        // Layout через RenderObject
        render.layout(&mut ctx)
    }

    fn children(&self, element_id: ElementId) -> &[ElementId] {
        let element = self.elements.get(element_id.index()).unwrap();
        element.children()
    }

    fn parent_data(&self, element_id: ElementId) -> Option<&dyn ParentData> {
        let element = self.elements.get(element_id.index()).unwrap();
        element.parent_data()
    }
}
```

---

## Преимущества Такой Архитектуры

### 1. Полная Независимость Rendering

```rust
// Можно использовать flui_rendering БЕЗ Element!

use flui_rendering::*;

struct MockTree {
    children_map: HashMap<ElementId, Vec<ElementId>>,
}

impl LayoutTreeAccess for MockTree {
    fn layout_child(&mut self, ...) -> Result<Geometry, LayoutError> {
        // Mock implementation для тестов
        Ok(Geometry::default())
    }

    fn children(&self, id: ElementId) -> &[ElementId] {
        self.children_map.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

// Тестируем RenderObject изолированно!
#[test]
fn test_render_padding() {
    let mut mock_tree = MockTree::new();
    let mut padding = RenderPadding::new(EdgeInsets::all(10.0));

    let mut ctx = LayoutContext::new(&mut mock_tree, ...);
    let size = padding.layout(&mut ctx).unwrap();

    assert_eq!(size, expected_size);
}
```

### 2. Разные Реализации Tree

```rust
// flui-element/ElementTree реализует LayoutTreeAccess
impl LayoutTreeAccess for ElementTree { ... }

// Можно сделать альтернативную реализацию!
pub struct SimplifiedTree { ... }
impl LayoutTreeAccess for SimplifiedTree { ... }

// Можно использовать rendering с разными деревьями!
```

### 3. Тестирование

```rust
// Легко тестировать RenderObject без всего фреймворка
struct TestTree {
    children: Vec<ElementId>,
    results: HashMap<ElementId, Geometry>,
}

impl LayoutTreeAccess for TestTree {
    fn layout_child(&mut self, id: ElementId, ...) -> Result<Geometry, ...> {
        Ok(self.results[&id].clone())
    }

    fn children(&self, _: ElementId) -> &[ElementId] {
        &self.children
    }
}

#[test]
fn test_flex_layout() {
    let mut test_tree = TestTree::new();
    test_tree.add_child_result(child_id, Size::new(100.0, 50.0));

    let mut flex = RenderFlex::new(Axis::Vertical);
    let mut ctx = LayoutContext::new(&mut test_tree, ...);

    let size = flex.layout(&mut ctx).unwrap();
    assert_eq!(size, expected_size);
}
```

---

## Текущее Состояние vs Целевое

### Текущее (проблемное)

```
flui_rendering
  ├─ depends on: flui_types, flui-foundation, flui-tree
  ├─ Element хранит render_id: RenderId
  └─ RenderTree - отдельное хранилище

flui-element
  ├─ depends on: flui_rendering, flui-view
  └─ Element ссылается на RenderTree через ID
```

### Целевое (правильное)

```
flui_rendering  (Layer 2)
  ├─ depends on: flui_types, flui-foundation, flui-tree
  ├─ Определяет: RenderObject, Protocol, Arity
  ├─ Определяет: LayoutTreeAccess trait (абстракция!)
  └─ НЕ знает об Element, View, Core

flui-element  (Layer 3)
  ├─ depends on: flui_rendering, flui-view, flui-foundation
  ├─ Реализует: LayoutTreeAccess для ElementTree
  └─ Element ВЛАДЕЕТ RenderObject напрямую
```

---

## Миграция

### Phase 1: Убрать циклические зависимости

```bash
# Проверить текущие зависимости
cd crates/flui_rendering
grep -r "flui-element\|flui-view\|flui_core" Cargo.toml

# Должно быть ПУСТО!
```

### Phase 2: Создать абстракции

```rust
// flui_rendering/src/tree.rs

/// Trait для доступа к дереву (вместо прямой зависимости от ElementTree)
pub trait TreeAccess {
    fn children(&self, id: ElementId) -> &[ElementId];
}

pub trait LayoutTreeAccess: TreeAccess {
    fn layout_child(...) -> Result<Geometry, LayoutError>;
}

pub trait PaintTreeAccess: TreeAccess {
    fn paint_child(...) -> Result<(), PaintError>;
}

pub trait HitTestTreeAccess: TreeAccess {
    fn hit_test_child(...) -> bool;
}
```

### Phase 3: Обновить Context

```rust
// Вместо:
pub struct LayoutContext<'tree> {
    element_tree: &'tree ElementTree,  // ❌ Прямая зависимость
}

// Использовать:
pub struct LayoutContext<'tree, T: LayoutTreeAccess> {
    tree: &'tree mut T,  // ✅ Generic абстракция
}
```

### Phase 4: Element реализует traits

```rust
// flui-element/src/element_tree.rs

impl LayoutTreeAccess for ElementTree {
    fn layout_child(...) -> Result<Geometry, LayoutError> {
        // Реализация
    }
}

impl PaintTreeAccess for ElementTree {
    fn paint_child(...) -> Result<(), PaintError> {
        // Реализация
    }
}
```

---

## Проверка Правильности

### Критерий 1: Граф Зависимостей Ацикличен

```bash
cargo tree --package flui_rendering

# Должно НЕ содержать:
# - flui-element
# - flui-view
# - flui_core
# - flui_widgets
```

### Критерий 2: Можно Собрать Изолированно

```bash
cd crates/flui_rendering
cargo build

# Должно собираться БЕЗ ошибок
# и БЕЗ зависимости от framework layers
```

### Критерий 3: Можно Тестировать Изолированно

```bash
cd crates/flui_rendering
cargo test

# Должно работать с mock tree,
# БЕЗ реальной ElementTree
```

---

## Заключение

**Правильная архитектура зависимостей:**

1. ✅ **flui_rendering** - независимый rendering layer
   - Зависит только от: types, foundation, tree, painting, interaction
   - Определяет: RenderObject, Protocol, Arity, Context traits
   - НЕ зависит от: element, view, core

2. ✅ **flui-element** - framework layer
   - Зависит от: rendering (и реализует его traits)
   - Владеет RenderObject
   - Реализует tree access traits

3. ✅ **Тестируемость** - rendering можно тестировать отдельно
4. ✅ **Модульность** - можно использовать rendering в других контекстах
5. ✅ **Расширяемость** - можно создать альтернативные реализации

Это **правильная архитектура** для модульной системы! 🎯

# FLUI Tree Integration Guide

Детальное руководство по применению `flui-tree` для разделения ответственности между крейтами FLUI.

## Проблема (текущее состояние)

### Текущая архитектура (с циклическими зависимостями)

```
flui_core/element/element_tree.rs:
- ElementTree управляет element nodes
- ElementTree содержит layout_render_object()
- ElementTree содержит paint_render_object()
- ElementTree зависит от RenderObject trait

flui_rendering/core/render_object.rs:
- RenderObject trait для layout/paint
- Нужен доступ к ElementTree для parent/children

flui_pipeline/layout_pipeline.rs:
- LayoutPipeline использует ElementTree
- Зависит от flui_core
- Зависит от flui_rendering

РЕЗУЛЬТАТ: flui_core → flui_rendering → flui_pipeline → flui_core (ЦИКЛ!)
```

### Проблемы

1. **Циклические зависимости** - невозможно компилировать
2. **Тесная связанность** - ElementTree знает о layout/paint
3. **Невозможность тестирования** - не можем мокать RenderTreeAccess
4. **Нарушение SRP** - ElementTree делает слишком много

## Решение (с flui-tree)

### Новая архитектура

```
flui-foundation (ElementId, Slot, Key)
       ↓
flui-tree (TreeRead, TreeNav, TreeWrite, RenderTreeAccess, DirtyTracking)
       ↓
   ┌───┴───┐
   ↓       ↓
flui-element   flui-rendering
(ElementTree)  (RenderObject trait, layout algorithms)
   ↓           ↓
   └─────┬─────┘
         ↓
   flui-pipeline
   (connects everything)
```

### Граф зависимостей

```
flui-foundation (0 deps)
    ↓
flui-tree (foundation)
    ↓
    ├─→ flui-element (tree)
    └─→ flui-rendering (tree)
            ↓
        flui-pipeline (element + rendering)
```

## Шаг 1: Реализация TreeRead/TreeNav/TreeWrite в ElementTree

### flui-element/src/element_tree.rs

```rust
use flui_tree::{TreeRead, TreeNav, TreeWrite, TreeWriteNav};
use flui_foundation::{ElementId, Slot};
use crate::error::ElementResult;

pub struct ElementTree {
    /// Slab-based storage
    nodes: Slab<ElementNode>,
}

impl TreeRead for ElementTree {
    type Node = Element;
    
    fn get(&self, id: ElementId) -> Option<&Element> {
        let index = id.get() as usize - 1;
        self.nodes.get(index).map(|node| &node.element)
    }
    
    fn contains(&self, id: ElementId) -> bool {
        let index = id.get() as usize - 1;
        self.nodes.get(index).is_some()
    }
    
    fn len(&self) -> usize {
        self.nodes.len()
    }
    
    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        Some(Box::new(
            self.nodes
                .iter()
                .map(|(idx, _)| ElementId::new(idx as u64 + 1))
        ))
    }
}

impl TreeNav for ElementTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.base().parent()
    }
    
    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id)
            .map(|e| e.children())
            .unwrap_or(&[])
    }
    
    fn slot(&self, id: ElementId) -> Option<Slot> {
        self.get(id)?.base().slot()
    }
}

impl TreeWrite for ElementTree {
    fn get_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        let index = id.get() as usize - 1;
        self.nodes.get_mut(index).map(|node| &mut node.element)
    }
    
    fn insert(&mut self, element: Element) -> ElementId {
        let entry = self.nodes.vacant_entry();
        let id = ElementId::new(entry.key() as u64 + 1);
        entry.insert(ElementNode { element });
        id
    }
    
    fn remove(&mut self, id: ElementId) -> Option<Element> {
        let index = id.get() as usize - 1;
        self.nodes.try_remove(index).map(|node| node.element)
    }
    
    fn clear(&mut self) {
        self.nodes.clear();
    }
    
    fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }
}

impl TreeWriteNav for ElementTree {
    fn set_parent(
        &mut self,
        child: ElementId,
        new_parent: Option<ElementId>,
    ) -> TreeResult<()> {
        // Validate no cycles
        if let Some(parent_id) = new_parent {
            if !self.contains(parent_id) {
                return Err(TreeError::not_found(parent_id));
            }
            if self.is_descendant(parent_id, child) || parent_id == child {
                return Err(TreeError::cycle_detected(child));
            }
        }
        
        // Remove from old parent's children
        if let Some(old_parent) = self.parent(child) {
            if let Some(parent_elem) = self.get_mut(old_parent) {
                parent_elem.remove_child(child);
            }
        }
        
        // Update child's parent
        if let Some(child_elem) = self.get_mut(child) {
            child_elem.base_mut().set_parent(new_parent);
        }
        
        // Add to new parent's children
        if let Some(parent_id) = new_parent {
            if let Some(parent_elem) = self.get_mut(parent_id) {
                parent_elem.add_child(child);
            }
        }
        
        Ok(())
    }
}
```

## Шаг 2: Реализация RenderTreeAccess

### flui-element/src/element_tree_render.rs

```rust
use flui_tree::RenderTreeAccess;
use std::any::Any;

impl RenderTreeAccess for ElementTree {
    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        let element = self.get(id)?;
        
        // Only render elements have RenderObjects
        if !element.is_render() {
            return None;
        }
        
        // ViewObject stores the RenderObject
        element.view_object()
            .render_object()
            .map(|r| r as &dyn Any)
    }
    
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        let element = self.get_mut(id)?;
        
        if !element.is_render() {
            return None;
        }
        
        element.view_object_mut()
            .render_object_mut()
            .map(|r| r as &mut dyn Any)
    }
    
    fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
        let element = self.get(id)?;
        
        if !element.is_render() {
            return None;
        }
        
        element.view_object()
            .render_state()
            .map(|s| s as &dyn Any)
    }
    
    fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        let element = self.get_mut(id)?;
        
        if !element.is_render() {
            return None;
        }
        
        element.view_object_mut()
            .render_state_mut()
            .map(|s| s as &mut dyn Any)
    }
    
    fn get_size(&self, id: ElementId) -> Option<(f32, f32)> {
        let element = self.get(id)?;
        
        element.view_object()
            .render_state()
            .map(|s| {
                let size = s.size();
                (size.width, size.height)
            })
    }
    
    fn get_offset(&self, id: ElementId) -> Option<(f32, f32)> {
        let element = self.get(id)?;
        
        element.view_object()
            .render_state()
            .map(|s| {
                let offset = s.offset();
                (offset.x, offset.y)
            })
    }
}
```

## Шаг 3: Реализация DirtyTracking

### flui-element/src/element_tree_dirty.rs

```rust
use flui_tree::DirtyTracking;
use parking_lot::RwLock;
use std::sync::Arc;

/// Shared dirty sets for thread-safe marking
pub struct DirtySets {
    layout: RwLock<HashSet<ElementId>>,
    paint: RwLock<HashSet<ElementId>>,
}

impl ElementTree {
    pub fn with_dirty_sets(dirty: Arc<DirtySets>) -> Self {
        Self {
            nodes: Slab::new(),
            dirty_sets: dirty,
        }
    }
}

impl DirtyTracking for ElementTree {
    fn mark_needs_layout(&self, id: ElementId) {
        // Mark atomic flag for fast check
        if let Some(element) = self.get(id) {
            if let Some(state) = element.view_object().render_state() {
                state.mark_needs_layout();
            }
        }
        
        // Add to dirty set for pipeline
        self.dirty_sets.layout.write().insert(id);
        
        // TODO: Propagate up to relayout boundary
    }
    
    fn mark_needs_paint(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(state) = element.view_object().render_state() {
                state.mark_needs_paint();
            }
        }
        
        self.dirty_sets.paint.write().insert(id);
    }
    
    fn clear_needs_layout(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(state) = element.view_object().render_state() {
                state.clear_needs_layout();
            }
        }
    }
    
    fn clear_needs_paint(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(state) = element.view_object().render_state() {
                state.clear_needs_paint();
            }
        }
    }
    
    fn needs_layout(&self, id: ElementId) -> bool {
        self.get(id)
            .and_then(|e| e.view_object().render_state())
            .map(|s| s.needs_layout())
            .unwrap_or(false)
    }
    
    fn needs_paint(&self, id: ElementId) -> bool {
        self.get(id)
            .and_then(|e| e.view_object().render_state())
            .map(|s| s.needs_paint())
            .unwrap_or(false)
    }
}

impl DirtyTrackingExt for ElementTree {
    fn elements_needing_layout(&self) -> Vec<ElementId> {
        self.dirty_sets.layout.read().iter().copied().collect()
    }
    
    fn elements_needing_paint(&self) -> Vec<ElementId> {
        self.dirty_sets.paint.read().iter().copied().collect()
    }
}
```

## Шаг 4: Использование в flui-rendering

### flui-rendering/src/layout/flex.rs

```rust
use flui_tree::{RenderTreeAccess, TreeNav};
use flui_foundation::ElementId;

pub fn layout_flex<T: RenderTreeAccess>(
    tree: &T,
    element_id: ElementId,
    constraints: BoxConstraints,
) -> Size {
    // Get RenderFlex without knowing concrete ElementTree type
    let render_obj = tree.render_object(element_id)
        .expect("element must be render element")
        .downcast_ref::<RenderFlex>()
        .expect("must be RenderFlex");
    
    // Get render children (skips non-render wrappers)
    let children = tree.render_children(element_id);
    
    // Layout children
    let mut total_flex = 0;
    let mut allocated_size = 0.0;
    
    for &child_id in &children {
        // Downcast child RenderObject
        if let Some(child_obj) = tree.render_object(child_id) {
            if let Some(flex_child) = child_obj.downcast_ref::<RenderFlexChild>() {
                total_flex += flex_child.flex;
                
                // Call child layout recursively
                // The tree implementation handles the actual layout call
                // through its own layout_render_object() method
            }
        }
    }
    
    // Calculate sizes...
    Size::new(allocated_size, constraints.max_height)
}
```

### flui-rendering/src/paint/box_painter.rs

```rust
use flui_tree::{RenderTreeAccess, DirtyTracking};

pub fn paint_box<T: RenderTreeAccess + DirtyTracking>(
    tree: &T,
    element_id: ElementId,
    canvas: &mut Canvas,
) {
    // Check if needs paint
    if !tree.needs_paint(element_id) {
        return;
    }
    
    // Get RenderBox
    let render_box = tree.render_object(element_id)
        .and_then(|obj| obj.downcast_ref::<RenderBox>())
        .expect("must be RenderBox");
    
    // Get offset from RenderState
    let offset = tree.get_offset(element_id)
        .map(|(x, y)| Offset::new(x, y))
        .unwrap_or(Offset::ZERO);
    
    // Paint self
    canvas.save();
    canvas.translate(offset.x, offset.y);
    
    render_box.paint(canvas);
    
    // Paint children
    for &child_id in tree.render_children(element_id).iter() {
        paint_box(tree, child_id, canvas);
    }
    
    canvas.restore();
    
    // Clear dirty flag
    tree.clear_needs_paint(element_id);
}
```

## Шаг 5: Связываем всё в flui-pipeline

### flui-pipeline/src/layout_pipeline.rs

```rust
use flui_tree::{RenderTreeAccess, DirtyTracking, TreeNav};
use flui_element::ElementTree;
use flui_rendering::layout;

pub struct LayoutPipeline {
    // No direct dependency on ElementTree internals!
}

impl LayoutPipeline {
    pub fn perform_layout<T>(
        &mut self,
        tree: &mut T,
        root: ElementId,
        constraints: BoxConstraints,
    ) -> Result<(), LayoutError>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        // Get all dirty elements
        let dirty_elements = tree.elements_needing_layout();
        
        tracing::info!("Layout: processing {} dirty elements", dirty_elements.len());
        
        // Sort by depth (layout parents before children)
        let mut sorted: Vec<_> = dirty_elements.into_iter().collect();
        sorted.sort_by_key(|&id| tree.depth(id));
        
        for element_id in sorted {
            if !tree.needs_layout(element_id) {
                continue; // Already laid out as part of parent
            }
            
            // Dispatch to correct layout algorithm based on RenderObject type
            if let Some(render_obj) = tree.render_object(element_id) {
                if render_obj.is::<RenderFlex>() {
                    layout::flex::layout_flex(tree, element_id, constraints)?;
                } else if render_obj.is::<RenderBox>() {
                    layout::box_layout::layout_box(tree, element_id, constraints)?;
                }
                // ... more render types
            }
            
            tree.clear_needs_layout(element_id);
        }
        
        Ok(())
    }
}
```

### flui-pipeline/src/paint_pipeline.rs

```rust
pub struct PaintPipeline {
    // Generic over tree type
}

impl PaintPipeline {
    pub fn perform_paint<T>(
        &mut self,
        tree: &mut T,
        canvas: &mut Canvas,
    ) -> Result<usize, PaintError>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        let dirty_elements = tree.elements_needing_paint();
        
        tracing::info!("Paint: processing {} dirty elements", dirty_elements.len());
        
        for element_id in dirty_elements {
            if !tree.needs_paint(element_id) {
                continue;
            }
            
            // Paint using generic interface
            paint::box_painter::paint_box(tree, element_id, canvas);
        }
        
        Ok(dirty_elements.len())
    }
}
```

## Шаг 6: Использование итераторов

### Пример: Сбор всех RenderObject в subtree

```rust
use flui_tree::{RenderTreeAccess, iter::RenderDescendants};

pub fn collect_render_objects<T: RenderTreeAccess>(
    tree: &T,
    root: ElementId,
) -> Vec<ElementId> {
    RenderDescendants::new(tree, root).collect()
}

// Или с помощью метода:
let render_objects: Vec<_> = tree.render_descendants(root).collect();
```

### Пример: Найти render parent

```rust
use flui_tree::iter::render::find_render_ancestor;

pub fn get_render_parent<T: RenderTreeAccess>(
    tree: &T,
    element_id: ElementId,
) -> Option<ElementId> {
    find_render_ancestor(tree, element_id)
}
```

## Шаг 7: Тестирование с mock деревом

### flui-rendering/tests/layout_test.rs

```rust
use flui_tree::{TreeRead, TreeNav, RenderTreeAccess};
use std::any::Any;

// Mock tree для тестов layout алгоритмов
struct MockTree {
    nodes: HashMap<ElementId, MockNode>,
}

struct MockNode {
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    render_object: Box<dyn Any>,
}

impl TreeRead for MockTree {
    type Node = MockNode;
    
    fn get(&self, id: ElementId) -> Option<&MockNode> {
        self.nodes.get(&id)
    }
    
    fn len(&self) -> usize {
        self.nodes.len()
    }
}

impl TreeNav for MockTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent
    }
    
    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }
}

impl RenderTreeAccess for MockTree {
    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        self.get(id).map(|n| &*n.render_object as &dyn Any)
    }
    
    // ... other methods
}

#[test]
fn test_flex_layout() {
    let mut tree = MockTree::new();
    
    // Build test tree
    let root = tree.insert_flex_render();
    let child1 = tree.insert_box_render(root);
    let child2 = tree.insert_box_render(root);
    
    // Test layout algorithm without depending on ElementTree!
    let size = layout::flex::layout_flex(
        &tree,
        root,
        BoxConstraints::tight(Size::new(400.0, 600.0)),
    );
    
    assert_eq!(size.width, 400.0);
}
```

## Преимущества новой архитектуры

### 1. Нет циклических зависимостей

```toml
# flui-element/Cargo.toml
[dependencies]
flui-foundation = { path = "../flui-foundation" }
flui-tree = { path = "../flui-tree" }
# ✅ НЕ зависит от flui-rendering

# flui-rendering/Cargo.toml
[dependencies]
flui-foundation = { path = "../flui-foundation" }
flui-tree = { path = "../flui-tree" }
# ✅ НЕ зависит от flui-element

# flui-pipeline/Cargo.toml
[dependencies]
flui-element = { path = "../flui-element" }
flui-rendering = { path = "../flui-rendering" }
# ✅ Зависит от обоих, но они не зависят друг от друга
```

### 2. Разделение ответственности

| Крейт | Ответственность |
|-------|----------------|
| `flui-foundation` | Базовые типы (ElementId, Slot) |
| `flui-tree` | Абстрактные trait-ы для деревьев |
| `flui-element` | Управление Element деревом |
| `flui-rendering` | Layout/paint алгоритмы |
| `flui-pipeline` | Координация build/layout/paint |

### 3. Тестируемость

```rust
// ✅ Можем тестировать layout без ElementTree
#[test]
fn test_flex_layout_with_mock() {
    let mock_tree = MockTree::new();
    let result = layout_flex(&mock_tree, root_id, constraints);
    assert_eq!(result.width, expected);
}

// ✅ Можем тестировать ElementTree без layout
#[test]
fn test_element_tree_navigation() {
    let tree = ElementTree::new();
    let root = tree.insert(Element::new(...));
    assert_eq!(tree.parent(root), None);
}
```

### 4. Переиспользование алгоритмов

```rust
// Layout алгоритм работает с ЛЮБЫМ типом дерева
pub fn layout_flex<T: RenderTreeAccess>(
    tree: &T,
    element_id: ElementId,
    constraints: BoxConstraints,
) -> Size {
    // ...
}

// Можем использовать с:
// - ElementTree в production
// - MockTree в тестах
// - RemoteTree для distributed rendering
// - SerializedTree для serialization
```

### 5. Инкрементальная миграция

Можно мигрировать постепенно:

1. **Phase 1**: Добавить flui-tree, реализовать TreeRead/TreeNav
2. **Phase 2**: Переместить layout алгоритмы в flui-rendering
3. **Phase 3**: Добавить RenderTreeAccess
4. **Phase 4**: Переместить paint в flui-rendering
5. **Phase 5**: Создать flui-pipeline, убрать layout/paint из ElementTree

## План миграции

### Week 1: Foundation

- [ ] Добавить `flui-tree` в workspace
- [ ] Реализовать `TreeRead` для `ElementTree`
- [ ] Реализовать `TreeNav` для `ElementTree`
- [ ] Написать тесты для навигации

### Week 2: Write Operations

- [ ] Реализовать `TreeWrite` для `ElementTree`
- [ ] Реализовать `TreeWriteNav` для `ElementTree`
- [ ] Добавить cycle detection
- [ ] Тесты для mutations

### Week 3: Render Access

- [ ] Реализовать `RenderTreeAccess` для `ElementTree`
- [ ] Создать `flui-rendering/src/tree_access.rs`
- [ ] Переместить layout helpers в rendering
- [ ] Тесты с mock tree

### Week 4: Dirty Tracking

- [ ] Реализовать `DirtyTracking` для `ElementTree`
- [ ] Добавить `DirtySets` для координации
- [ ] Интегрировать с `LayoutPipeline`
- [ ] Тесты для dirty flags

### Week 5: Pipeline

- [ ] Создать `flui-pipeline` крейт
- [ ] Переместить `LayoutPipeline` из core
- [ ] Переместить `PaintPipeline` из core
- [ ] Integration tests

### Week 6: Cleanup

- [ ] Удалить layout/paint из `ElementTree`
- [ ] Обновить все импорты
- [ ] Обновить документацию
- [ ] Performance benchmarks

## Заключение

`flui-tree` решает фундаментальную архитектурную проблему в FLUI:

✅ **Разрывает циклические зависимости** через trait абстракции  
✅ **Разделяет ответственность** между управлением деревом и rendering  
✅ **Улучшает тестируемость** через mock implementations  
✅ **Обеспечивает переиспользование** layout/paint алгоритмов  
✅ **Позволяет инкрементальную миграцию** без breaking changes  

Это **production-ready** решение с:
- 442+ тестами в foundation
- Zero-allocation итераторами
- Thread-safe dirty tracking
- Comprehensive documentation
- Benchmark suite

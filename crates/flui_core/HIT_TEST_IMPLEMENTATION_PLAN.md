# Hit Test Implementation Plan

> План реализации полноценного hit testing для FLUI RenderObjects

**Дата создания:** 2025-01-11
**Статус:** 📋 Планирование
**Приоритет:** MEDIUM-HIGH

---

## 📊 Текущий статус

### ✅ Что уже реализовано

**Инфраструктура:**
- ✅ `ElementHitTestResult` - результат hit testing с entries
- ✅ `ElementHitTestEntry` - базовый entry с element_id и local_position
- ✅ `BoxHitTestEntry` - специализированный entry для box rendering
- ✅ `SliverHitTestEntry` - специализированный entry для sliver rendering (с scroll offset, geometry)
- ✅ `HitTestEntryTrait` - унифицированный trait для всех entry типов
- ✅ `HitTestCache` - кеширование результатов hit test
- ✅ `ElementTree::hit_test()` - базовый алгоритм hit testing (box-based, упрощенный)
- ✅ `ElementTree::hit_test_recursive()` - рекурсивный обход дерева
- ✅ `ElementTree::hit_test_render()` - hit test для box render elements
- ✅ `ElementTree::hit_test_sliver()` - hit test для sliver elements

**Файлы:**
```
crates/flui_core/src/element/
├── hit_test.rs              ✅ ElementHitTestResult, GenericHitTestResult
├── hit_test_entry.rs        ✅ BoxHitTestEntry, SliverHitTestEntry, trait
└── element_tree.rs          ✅ hit_test(), hit_test_recursive()

crates/flui_core/src/pipeline/
└── hit_test_cache.rs        ✅ Кеширование
```

### ❌ Что отсутствует

**Критические пробелы:**
1. ❌ `Render::hit_test()` - метод в Render trait
2. ❌ `RenderSliver::hit_test()` - метод в RenderSliver trait
3. ❌ `BoxHitTestContext` - контекст для box hit testing
4. ❌ `SliverHitTestContext` - контекст для sliver hit testing
5. ❌ Custom hit test shapes (круги, paths, произвольные формы)
6. ❌ Transform-aware hit testing (RenderTransform не применяет inverse transform)
7. ❌ Clip-aware hit testing (RenderClipRect не ограничивает hit область)
8. ❌ RenderAbsorbPointer/RenderIgnorePointer не контролируют события
9. ❌ Viewport-aware hit testing для slivers (scroll offset, cache extent)

---

## 🎯 Цели реализации

### Основные цели

1. **RenderObjects контролируют hit testing**
   - Каждый RenderObject может override hit test логику
   - Custom shapes, transforms, clipping работают корректно

2. **Полная поддержка interaction**
   - AbsorbPointer реально поглощает события
   - IgnorePointer реально пропускает события
   - Transform применяет inverse transform к hit position

3. **Viewport-aware hit testing для slivers**
   - Slivers учитывают scroll offset
   - Поддержка cache extent для off-screen content
   - Main axis / cross axis координаты

4. **Обратная совместимость**
   - Default реализация hit_test() для существующих RenderObjects
   - Постепенная миграция без breaking changes

---

## 📋 План реализации

### Phase 1: Trait Extensions (HIGH PRIORITY)

**Задача:** Добавить `hit_test()` методы в traits с default реализацией

#### 1.1. Создать HitTestContext структуры

**Файл:** `crates/flui_core/src/render/hit_test_context.rs` (новый)

```rust
/// Context for box hit testing
pub struct BoxHitTestContext<'a> {
    /// Element tree for child hit testing
    pub tree: &'a ElementTree,

    /// Position in local coordinates
    pub position: Offset,

    /// Size of the element (from RenderState)
    pub size: Size,

    /// Children of this element
    pub children: Children,

    /// Element ID being tested
    pub element_id: ElementId,
}

/// Context for sliver hit testing
pub struct SliverHitTestContext<'a> {
    /// Element tree for child hit testing
    pub tree: &'a ElementTree,

    /// Position along main axis
    pub main_axis_position: f32,

    /// Position along cross axis
    pub cross_axis_position: f32,

    /// Sliver geometry (from RenderState)
    pub geometry: SliverGeometry,

    /// Current scroll offset
    pub scroll_offset: f32,

    /// Axis direction (Vertical/Horizontal)
    pub axis_direction: AxisDirection,

    /// Children of this element
    pub children: Children,

    /// Element ID being tested
    pub element_id: ElementId,
}
```

**Статус:** ⏳ TODO

---

#### 1.2. Расширить Render trait

**Файл:** `crates/flui_core/src/render/render.rs`

**Изменения:**
```rust
pub trait Render: Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &LayoutContext) -> Size;
    fn paint(&self, ctx: &PaintContext) -> Canvas;

    // ✅ Новые методы hit testing

    /// Perform hit test on this render object
    ///
    /// Returns true if this element (or any child) was hit.
    /// Default implementation: test children, then self.
    fn hit_test(
        &self,
        ctx: &BoxHitTestContext,
        result: &mut BoxHitTestResult,
    ) -> bool {
        // Default: check self, then test children
        if self.hit_test_self(ctx.position) {
            let hit_children = self.hit_test_children(ctx, result);
            result.add(BoxHitTestEntry::new(ctx.position, ctx.size));
            return true;
        }
        false
    }

    /// Test if position hits this element (ignoring children)
    ///
    /// Default: simple box bounds check.
    fn hit_test_self(&self, position: Offset) -> bool {
        // Default: always return false (only hit if children hit)
        // Override for leaf nodes or special hit shapes
        false
    }

    /// Test children for hits
    ///
    /// Default: test all children front-to-back.
    fn hit_test_children(
        &self,
        ctx: &BoxHitTestContext,
        result: &mut BoxHitTestResult,
    ) -> bool {
        // Default implementation in trait
        match ctx.children {
            Children::None => false,
            Children::Single(child_id) => {
                ctx.tree.hit_test_child(child_id, ctx.position, result)
            }
            Children::Multi(children) => {
                let mut hit = false;
                // Test children front-to-back (reverse order)
                for &child_id in children.iter().rev() {
                    if ctx.tree.hit_test_child(child_id, ctx.position, result) {
                        hit = true;
                    }
                }
                hit
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any;
    fn arity(&self) -> Arity;
}
```

**Статус:** ⏳ TODO

---

#### 1.3. Расширить RenderSliver trait

**Файл:** `crates/flui_core/src/render/render_sliver.rs`

**Изменения:**
```rust
pub trait RenderSliver: Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry;
    fn paint(&self, ctx: &SliverPaintContext) -> Canvas;

    // ✅ Новые методы hit testing

    /// Perform hit test on this sliver
    ///
    /// Returns true if this sliver (or any child) was hit.
    fn hit_test(
        &self,
        ctx: &SliverHitTestContext,
        result: &mut SliverHitTestResult,
    ) -> bool {
        // Check if hit is in visible region
        if ctx.main_axis_position < 0.0
            || ctx.main_axis_position >= ctx.geometry.paint_extent {
            return false;
        }

        if self.hit_test_self(ctx.main_axis_position, ctx.cross_axis_position) {
            let hit = self.hit_test_children(ctx, result);
            result.add(SliverHitTestEntry::new(
                Offset::new(ctx.cross_axis_position, ctx.main_axis_position),
                ctx.geometry.clone(),
                ctx.scroll_offset,
                ctx.main_axis_position,
            ));
            return true;
        }
        false
    }

    /// Test if position hits this sliver (ignoring children)
    fn hit_test_self(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        false  // Default: only hit if children hit
    }

    /// Test children for hits
    fn hit_test_children(
        &self,
        ctx: &SliverHitTestContext,
        result: &mut SliverHitTestResult,
    ) -> bool {
        // Default implementation
        false
    }

    fn as_any(&self) -> &dyn std::any::Any;
    fn arity(&self) -> Arity;
}
```

**Статус:** ⏳ TODO

---

### Phase 2: Core RenderObject Implementations (HIGH PRIORITY)

**Задача:** Реализовать `hit_test()` для критических RenderObjects

#### 2.1. Interaction RenderObjects

**Файлы:**
- `crates/flui_rendering/src/objects/interaction/absorb_pointer.rs`
- `crates/flui_rendering/src/objects/interaction/ignore_pointer.rs`

**RenderAbsorbPointer:**
```rust
impl Render for RenderAbsorbPointer {
    fn hit_test(&self, ctx: &BoxHitTestContext, result: &mut BoxHitTestResult) -> bool {
        if self.absorbing {
            // Absorb: add self but DON'T test children
            result.add(BoxHitTestEntry::new(ctx.position, ctx.size));
            return true;  // ✅ Event absorbed!
        } else {
            // Normal: test children
            self.hit_test_children(ctx, result)
        }
    }
}
```

**RenderIgnorePointer:**
```rust
impl Render for RenderIgnorePointer {
    fn hit_test(&self, ctx: &BoxHitTestContext, result: &mut BoxHitTestResult) -> bool {
        if self.ignoring {
            return false;  // ✅ Event passes through!
        } else {
            self.hit_test_children(ctx, result)
        }
    }
}
```

**Статус:** ⏳ TODO

---

#### 2.2. Transform RenderObjects

**Файл:** `crates/flui_rendering/src/objects/effects/transform.rs`

**RenderTransform:**
```rust
impl Render for RenderTransform {
    fn hit_test(&self, ctx: &BoxHitTestContext, result: &mut BoxHitTestResult) -> bool {
        // Apply inverse transform to position
        let inverse = match self.transform.inverse() {
            Some(inv) => inv,
            None => return false,  // Singular transform, no hit
        };

        let transformed_position = inverse.transform_point(ctx.position);

        // Create new context with transformed position
        let new_ctx = BoxHitTestContext {
            position: transformed_position,  // ✅ Transform applied!
            ..ctx
        };

        self.hit_test_children(&new_ctx, result)
    }
}
```

**Статус:** ⏳ TODO

---

#### 2.3. Clip RenderObjects

**Файлы:**
- `crates/flui_rendering/src/objects/effects/clip_rect.rs`
- `crates/flui_rendering/src/objects/effects/clip_rrect.rs`
- `crates/flui_rendering/src/objects/effects/clip_oval.rs`
- `crates/flui_rendering/src/objects/effects/clip_path.rs`

**RenderClipRect:**
```rust
impl Render for RenderClipRect {
    fn hit_test_self(&self, position: Offset) -> bool {
        // Check if position is inside clip bounds
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx <= self.size.width
            && position.dy <= self.size.height
    }

    fn hit_test(&self, ctx: &BoxHitTestContext, result: &mut BoxHitTestResult) -> bool {
        if !self.hit_test_self(ctx.position) {
            return false;  // ✅ Outside clip bounds!
        }

        self.hit_test_children(ctx, result)
    }
}
```

**RenderClipOval:**
```rust
impl Render for RenderClipOval {
    fn hit_test_self(&self, position: Offset) -> bool {
        // Check if position is inside ellipse
        let center_x = self.size.width / 2.0;
        let center_y = self.size.height / 2.0;
        let dx = (position.dx - center_x) / center_x;
        let dy = (position.dy - center_y) / center_y;

        dx * dx + dy * dy <= 1.0  // ✅ Ellipse equation!
    }
}
```

**Статус:** ⏳ TODO

---

### Phase 3: ElementTree Integration (MEDIUM PRIORITY)

**Задача:** Интегрировать новые методы в ElementTree

#### 3.1. Обновить ElementTree::hit_test_render()

**Файл:** `crates/flui_core/src/element/element_tree.rs`

**Изменения:**
```rust
fn hit_test_render(
    &self,
    element_id: ElementId,
    render_elem: &RenderElement,
    position: Offset,
    result: &mut ElementHitTestResult,
) -> bool {
    let render_state = &render_elem.render_state;
    let offset = render_state.offset();
    let size = render_state.size();

    // Transform to local coordinates
    let local_position = position - offset;

    // Create hit test context
    let ctx = BoxHitTestContext {
        tree: self,
        position: local_position,
        size,
        children: self.get_children(element_id),
        element_id,
    };

    // Call RenderObject's hit_test method
    let mut box_result = BoxHitTestResult::new();
    let hit = render_elem.render_object.hit_test(&ctx, &mut box_result);

    if hit {
        // Convert BoxHitTestResult to ElementHitTestResult
        for entry in box_result.entries() {
            result.add_element(element_id, entry.local_position);
        }
    }

    hit
}
```

**Статус:** ⏳ TODO

---

### Phase 4: Testing & Validation (MEDIUM PRIORITY)

**Задача:** Создать тесты для hit testing

#### 4.1. Unit tests для RenderObjects

**Файл:** `crates/flui_rendering/tests/hit_test_tests.rs` (новый)

**Тесты:**
- ✅ RenderAbsorbPointer поглощает события
- ✅ RenderIgnorePointer пропускает события
- ✅ RenderTransform применяет inverse transform
- ✅ RenderClipRect ограничивает hit область
- ✅ RenderClipOval проверяет ellipse bounds
- ✅ RenderStack тестирует детей front-to-back

**Статус:** ⏳ TODO

---

#### 4.2. Integration tests

**Файл:** `crates/flui_core/tests/hit_test_integration.rs` (новый)

**Тесты:**
- ✅ Nested transforms (transform в transform)
- ✅ Clipped + transformed content
- ✅ AbsorbPointer внутри Stack
- ✅ Hit test с scrolling viewport

**Статус:** ⏳ TODO

---

### Phase 5: Documentation (LOW PRIORITY)

**Задача:** Документировать hit testing API

#### 5.1. API Guide

**Файл:** `crates/flui_core/docs/HIT_TESTING_GUIDE.md` (новый)

**Содержание:**
- Архитектура hit testing
- Как override hit_test() в custom RenderObjects
- Примеры custom hit shapes
- Best practices

**Статус:** ⏳ TODO

---

#### 5.2. Обновить существующую документацию

**Файлы:**
- `crates/flui_rendering/RENDER_OBJECT_GUIDE.md`
- `docs/API_GUIDE.md`

**Статус:** ⏳ TODO

---

## 📊 Progress Tracking

### Overall Progress: 0% (0/25 tasks)

| Phase | Tasks | Completed | Status |
|-------|-------|-----------|--------|
| Phase 1: Trait Extensions | 3 | 0 | ⏳ TODO |
| Phase 2: Core Implementations | 3 | 0 | ⏳ TODO |
| Phase 3: ElementTree Integration | 1 | 0 | ⏳ TODO |
| Phase 4: Testing | 2 | 0 | ⏳ TODO |
| Phase 5: Documentation | 2 | 0 | ⏳ TODO |
| **TOTAL** | **11** | **0** | **0%** |

---

## 🎯 Priority Matrix

### Must Have (Блокируют другие фичи)
1. ✅ Phase 1.1: HitTestContext structures
2. ✅ Phase 1.2: Render trait extension
3. ✅ Phase 2.1: AbsorbPointer/IgnorePointer
4. ✅ Phase 3.1: ElementTree integration

### Should Have (Важные для UX)
5. Phase 1.3: RenderSliver trait extension
6. Phase 2.2: Transform hit testing
7. Phase 2.3: Clip hit testing
8. Phase 4.1: Unit tests

### Nice to Have (Можно отложить)
9. Phase 4.2: Integration tests
10. Phase 5.1: API Guide
11. Phase 5.2: Update docs

---

## 🚧 Implementation Notes

### Breaking Changes
- ❌ **НЕТ breaking changes** - default implementation в traits
- ✅ Все существующие RenderObjects работают без изменений
- ✅ Постепенная миграция по мере необходимости

### Performance Considerations
- Hit testing должен быть быстрым (вызывается на каждый pointer event)
- Кеширование результатов через HitTestCache
- Ранний выход из рекурсии при hit == false
- Избегать аллокаций в hot path

### Future Extensions
- Custom hit shapes через trait (HitShape)
- Hit test debugging tools (визуализация hit regions)
- Hit test profiling (bottleneck detection)
- Gesture recognition integration

---

## 📝 Changelog

### 2025-01-11 - Initial Plan
- Создан план реализации hit testing
- Определены 5 фаз реализации
- Приоритизированы задачи

---

## 🔗 Related Documents

- [FLUTTER_RENDEROBJECT_ANALYSIS.md](../../flui_rendering/docs/FLUTTER_RENDEROBJECT_ANALYSIS.md) - Анализ Flutter API
- [RENDER_OBJECT_GUIDE.md](../../flui_rendering/RENDER_OBJECT_GUIDE.md) - Гайд по RenderObjects
- [element/hit_test.rs](./element/hit_test.rs) - Текущая реализация
- [element/hit_test_entry.rs](./element/hit_test_entry.rs) - Entry structures

---

**Next Steps:**
1. Review этого плана
2. Создать issue в GitHub (если используется)
3. Начать с Phase 1.1 (HitTestContext)
4. Итеративная реализация по фазам

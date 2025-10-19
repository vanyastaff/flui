# Flui Framework - Week 5-6 Roadmap: flui_widgets

> План развития на следующие 2 недели (20 октября - 3 ноября 2025)
> **Фокус:** Реализация базовых виджетов на основе готовых RenderObjects

## 🎯 Current Status (2025-10-19)

**✅ Завершено Week 3-4:**
- ✅ **13/13 RenderObjects** полностью готовы (flui_rendering complete!)
- ✅ **814 тестов** проходят во всём workspace
- ✅ **~23,550 строк кода** написано
- ✅ **0 clippy warnings**

**🚀 Готовы к старту Week 5-6:**
- 🎯 Создать **flui_widgets** crate
- 🎯 Реализовать базовые виджеты
- 🎯 Интеграция Widget → Element → RenderObject
- 🎯 Первые работающие примеры

---

## 📋 Week 5: Basic Widgets (20-27 октября)

### Priority 1: Setup flui_widgets Crate (~2 часа)

**Задачи:**
- Создать `crates/flui_widgets/` структуру
- Настроить Cargo.toml с зависимостями
- Создать lib.rs с базовой структурой
- Настроить re-exports

**Зависимости:**
```toml
[dependencies]
flui_core = { path = "../flui_core" }
flui_rendering = { path = "../flui_rendering" }
flui_types = { path = "../flui_types" }
```

---

### Priority 2: Container Widget (~300 строк, 12 тестов)

**Время:** 2 дня

**Описание:** Базовый контейнер - композиция всех layout виджетов

**Структура:**
```rust
pub struct Container {
    key: Option<Key>,
    // Layout properties
    width: Option<f32>,
    height: Option<f32>,
    padding: Option<EdgeInsets>,
    margin: Option<EdgeInsets>,
    alignment: Option<Alignment>,

    // Decoration
    color: Option<Color>,
    decoration: Option<BoxDecoration>,

    // Constraints
    constraints: Option<BoxConstraints>,

    // Child
    child: Option<Box<dyn Widget>>,
}
```

**Реализация:**
- Использует RenderConstrainedBox, RenderPadding, RenderDecoratedBox, RenderPositionedBox
- Композиция через вложенные виджеты
- Builder pattern для удобного API

**Тесты:**
- Container with width/height
- Container with padding
- Container with decoration
- Container with alignment
- Container composition

---

### Priority 3: Row & Column Widgets (~150 строк каждый, 8 тестов)

**Время:** 1.5 дня

**Описание:** Layout widgets для горизонтального и вертикального размещения

**Row:**
```rust
pub struct Row {
    key: Option<Key>,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    children: Vec<Box<dyn Widget>>,
}

impl RenderObjectWidget for Row {
    type RenderObjectType = RenderFlex;

    fn create_render_object(&self, context: &BuildContext) -> Self::RenderObjectType {
        RenderFlex::new(
            Axis::Horizontal,
            self.main_axis_alignment,
            self.cross_axis_alignment,
            self.main_axis_size,
        )
    }
}
```

**Column:** аналогично, но с Axis::Vertical

**Тесты:**
- Row with multiple children
- Column with multiple children
- MainAxisAlignment variants
- CrossAxisAlignment variants
- MainAxisSize::Min vs Max

---

### Priority 4: SizedBox, Padding, Center Widgets (~100 строк каждый, 6 тестов)

**Время:** 1.5 дня

**Описание:** Простые single-child layout виджеты

**SizedBox:**
```rust
pub struct SizedBox {
    key: Option<Key>,
    width: Option<f32>,
    height: Option<f32>,
    child: Option<Box<dyn Widget>>,
}

impl RenderObjectWidget for SizedBox {
    type RenderObjectType = RenderConstrainedBox;

    fn create_render_object(&self, context: &BuildContext) -> Self::RenderObjectType {
        RenderConstrainedBox::new(BoxConstraints::tightFor(
            self.width,
            self.height,
        ))
    }
}
```

**Padding:**
```rust
pub struct Padding {
    key: Option<Key>,
    padding: EdgeInsets,
    child: Option<Box<dyn Widget>>,
}
```

**Center:**
```rust
pub struct Center {
    key: Option<Key>,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: Option<Box<dyn Widget>>,
}
```

---

### Priority 5: Align Widget (~120 строк, 8 тестов)

**Время:** 1 день

**Описание:** Выравнивание child внутри доступного пространства

```rust
pub struct Align {
    key: Option<Key>,
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: Option<Box<dyn Widget>>,
}

impl RenderObjectWidget for Align {
    type RenderObjectType = RenderPositionedBox;

    fn create_render_object(&self, context: &BuildContext) -> Self::RenderObjectType {
        RenderPositionedBox::new(
            self.alignment,
            self.width_factor,
            self.height_factor,
        )
    }
}
```

---

### Summary Week 5:
- ✅ flui_widgets crate setup
- ✅ **6 базовых виджетов** (Container, Row, Column, SizedBox, Padding, Center, Align)
- ✅ **~920 строк кода**
- ✅ **40 тестов**
- ✅ Widget → RenderObject integration работает

---

## 📋 Week 6: Flex & Stack Widgets (28 октября - 3 ноября)

### Priority 6: Expanded & Flexible Widgets (~150 строк, 8 тестов)

**Время:** 1 день

**Описание:** Flex children с автоматическим распределением пространства

**Expanded:**
```rust
pub struct Expanded {
    key: Option<Key>,
    flex: i32,
    child: Box<dyn Widget>,
}

impl ParentDataWidget for Expanded {
    fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
        if let Some(flex_parent) = render_object.downcast_mut::<RenderFlex>() {
            flex_parent.set_flex(self.flex);
        }
    }
}
```

**Flexible:**
```rust
pub struct Flexible {
    key: Option<Key>,
    flex: i32,
    fit: FlexFit,
    child: Box<dyn Widget>,
}
```

---

### Priority 7: Stack & Positioned Widgets (~200 строк, 10 тестов)

**Время:** 1.5 дня

**Описание:** Позиционирование детей друг над другом

**Stack:**
```rust
pub struct Stack {
    key: Option<Key>,
    alignment: Alignment,
    fit: StackFit,
    children: Vec<Box<dyn Widget>>,
}

impl MultiChildRenderObjectWidget for Stack {
    type RenderObjectType = RenderStack;

    fn create_render_object(&self, context: &BuildContext) -> Self::RenderObjectType {
        RenderStack::new(self.alignment, self.fit)
    }
}
```

**Positioned:**
```rust
pub struct Positioned {
    key: Option<Key>,
    left: Option<f32>,
    top: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    child: Box<dyn Widget>,
}
```

---

### Priority 8: AspectRatio & DecoratedBox Widgets (~120 строк, 6 тестов)

**Время:** 1 день

**Описание:** Специализированные layout и decoration виджеты

**AspectRatio:**
```rust
pub struct AspectRatio {
    key: Option<Key>,
    aspect_ratio: f32,
    child: Option<Box<dyn Widget>>,
}
```

**DecoratedBox:**
```rust
pub struct DecoratedBox {
    key: Option<Key>,
    decoration: BoxDecoration,
    position: DecorationPosition,
    child: Option<Box<dyn Widget>>,
}
```

---

### Priority 9: Opacity, Transform, ClipRRect Widgets (~100 строк каждый, 6 тестов)

**Время:** 1.5 дня

**Описание:** Visual effects виджеты

**Opacity:**
```rust
pub struct Opacity {
    key: Option<Key>,
    opacity: f32,
    child: Option<Box<dyn Widget>>,
}
```

**Transform:**
```rust
pub struct Transform {
    key: Option<Key>,
    transform: Matrix4,
    alignment: Alignment,
    child: Option<Box<dyn Widget>>,
}
```

**ClipRRect:**
```rust
pub struct ClipRRect {
    key: Option<Key>,
    border_radius: BorderRadius,
    clip_behavior: Clip,
    child: Option<Box<dyn Widget>>,
}
```

---

### Summary Week 6:
- ✅ **8 дополнительных виджетов** (Expanded, Flexible, Stack, Positioned, AspectRatio, DecoratedBox, Opacity, Transform, ClipRRect)
- ✅ **~670 строк кода**
- ✅ **36 тестов**
- ✅ ParentDataWidget support

---

## 🎯 Goals After 2 Weeks

### Виджеты реализованы (14 total):

**Layout widgets:**
1. Container - композиция всех layout свойств
2. Row - горизонтальный flex layout
3. Column - вертикальный flex layout
4. SizedBox - фиксированный размер
5. Padding - отступы
6. Center - центрирование
7. Align - выравнивание
8. Expanded - flex child с автоматическим размером
9. Flexible - flex child с настраиваемым fit
10. Stack - layered positioning
11. Positioned - абсолютное позиционирование
12. AspectRatio - соотношение сторон

**Visual effects widgets:**
13. DecoratedBox - декорирование
14. Opacity - прозрачность
15. Transform - трансформации
16. ClipRRect - закругленное обрезание

### Statistics After Week 5-6:
- **16 базовых виджетов** реализовано
- **~1590 строк кода** в flui_widgets
- **76 тестов** в flui_widgets
- **890+ тестов** total в workspace
- **Widget → Element → RenderObject** pipeline работает

---

## 📊 Success Metrics

### Week 5 Goals:
- [ ] flui_widgets crate создан и настроен
- [ ] 6 базовых виджетов (Container, Row, Column, SizedBox, Padding, Center, Align)
- [ ] 40 тестов
- [ ] 0 clippy warnings
- [ ] Все тесты проходят

### Week 6 Goals:
- [ ] 8 дополнительных виджетов (Expanded, Flexible, Stack, Positioned, etc.)
- [ ] 36 тестов
- [ ] ParentDataWidget trait реализован
- [ ] MultiChildRenderObjectWidget support
- [ ] Документация для всех виджетов

### Overall 2-Week Goals:
- [ ] **16 виджетов** total
- [ ] **76 тестов** в flui_widgets
- [ ] **890+ тестов** в workspace
- [ ] **100%** базовых layout виджетов готовы
- [ ] **100%** visual effects виджетов готовы
- [ ] Ready to start FluiApp integration

---

## 🚀 Long-Term Vision (Week 7-8)

### Week 7: FluiApp & Platform Integration
- ElementTree management
- BuildContext implementation
- Widget lifecycle (mount, unmount, update)
- setState mechanism
- Integration с eframe

### Week 8: Examples & Demo
- Hello World example
- Counter example (StatefulWidget)
- Layout showcase
- Styling showcase
- First working demo app!

---

## ⚠️ Technical Challenges

### High Priority:
1. **Widget → Element → RenderObject lifecycle**
   - Правильная реализация create_element()
   - Element updates и rebuilds
   - RenderObject updates

2. **ParentDataWidget integration**
   - Применение parent data к RenderObjects
   - Flexible/Expanded integration с RenderFlex

3. **BuildContext implementation**
   - Доступ к Element tree
   - InheritedWidget lookups
   - Theme/MediaQuery support

### Medium Priority:
4. **MultiChildRenderObjectWidget**
   - Управление списком детей
   - Efficient updates

5. **Key system**
   - Widget identification
   - Element reuse

---

## 📅 Detailed Daily Breakdown

### Week 5 Schedule (20-27 октября):

**Day 1 (Oct 20): Setup & Container Part 1**
- Morning: Create flui_widgets crate (~1 hour)
- Afternoon: Container implementation start (~3 hours)

**Day 2 (Oct 21): Container Part 2**
- Morning: Container tests (~2 hours)
- Afternoon: Container documentation (~2 hours)

**Day 3 (Oct 22): Row & Column Part 1**
- Morning: Row implementation (~2 hours)
- Afternoon: Column implementation (~2 hours)

**Day 4 (Oct 23): Row & Column Part 2**
- Morning: Row/Column tests (~2 hours)
- Afternoon: Documentation (~2 hours)

**Day 5 (Oct 24): SizedBox, Padding, Center**
- Morning: SizedBox & Padding (~2 hours)
- Afternoon: Center & tests (~2 hours)

**Day 6 (Oct 25): Align Widget**
- Morning: Align implementation (~2 hours)
- Afternoon: Align tests & docs (~2 hours)

**Day 7 (Oct 26-27): Week Review**
- Review all widgets
- Integration testing
- Week 5 retrospective

### Week 6 Schedule (28 октября - 3 ноября):

**Day 8 (Oct 28): Expanded & Flexible**
- Morning: ParentDataWidget trait (~2 hours)
- Afternoon: Expanded & Flexible implementation (~2 hours)

**Day 9 (Oct 29): Stack Widget**
- Morning: Stack implementation (~2 hours)
- Afternoon: Stack tests (~2 hours)

**Day 10 (Oct 30): Positioned Widget**
- Morning: Positioned implementation (~2 hours)
- Afternoon: Positioned tests (~2 hours)

**Day 11 (Oct 31): AspectRatio & DecoratedBox**
- Morning: AspectRatio (~1.5 hours)
- Afternoon: DecoratedBox (~1.5 hours)

**Day 12 (Nov 1): Visual Effects Part 1**
- Morning: Opacity (~1.5 hours)
- Afternoon: Transform (~1.5 hours)

**Day 13 (Nov 2): Visual Effects Part 2**
- Morning: ClipRRect (~2 hours)
- Afternoon: All tests & documentation (~2 hours)

**Day 14 (Nov 3): Final Review & Planning**
- Morning: Week 6 retrospective
- Afternoon: Plan Week 7-8 (FluiApp)
- Evening: Prepare for platform integration

---

## 🎓 Learning Goals

### Technical Skills:
- **Widget patterns:** Composition, inheritance, mixins
- **Rust patterns:** Builder pattern, trait objects, downcasting
- **Testing:** Widget testing strategies
- **API design:** Fluent APIs, builder APIs

### Deliverables:
- [ ] Document: "Flui Widget Architecture"
- [ ] Tutorial: "Creating Custom Widgets"
- [ ] Examples: "Common Layout Patterns"

---

## 🔄 Iteration Strategy

### After Each Widget:
1. **Design** API и структуру
2. **Implement** create_render_object
3. **Test** все комбинации параметров
4. **Document** с примерами
5. **Review** API ergonomics
6. **Integrate** в flui_widgets

### Red Flags:
- ⚠️ Виджет API неудобен → переделать
- ⚠️ RenderObject не подходит → расширить
- ⚠️ Тесты сложные → упростить API
- ⚠️ Много boilerplate → создать макрос

---

## 🎯 Definition of Done

### For Each Widget:
- ✅ Implementation complete
- ✅ RenderObject integration working
- ✅ Minimum 6 tests per widget
- ✅ Documentation with examples
- ✅ No clippy warnings
- ✅ Exported from lib.rs

### For Each Week:
- ✅ All planned widgets complete
- ✅ All tests passing
- ✅ Documentation complete
- ✅ Retrospective notes written
- ✅ Next week planned

### For 2-Week Milestone:
- ✅ 16 widgets implemented
- ✅ 76 tests in flui_widgets
- ✅ 890+ tests total
- ✅ Ready for FluiApp integration
- ✅ Examples prepared

---

## 🎊 Ready to Start!

**Previous Achievement:** ✅ flui_rendering complete (13 RenderObjects)
**Current Focus:** 🎯 flui_widgets implementation
**Next Milestone:** 🚀 FluiApp & first working demo

**Let's build amazing widgets!** 🎨

---

**Last Updated:** 2025-10-19
**Version:** 0.1.0-alpha
**Phase:** Week 5-6 Planning - flui_widgets
**Next Review:** 2025-11-03 (After Week 6)
**Owner:** Flui Core Team
**Status:** 🚀 Ready to implement widgets!

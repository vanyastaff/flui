# Flui Framework - Next Roadmap (Week 3-4)

> План развития на ближайшие 2 недели (19 января - 2 февраля 2025)

## 🎯 Current Status (2025-01-18)

**Completed Today:**
- ✅ RenderDecoratedBox (320 строк, 10 тестов)
- ✅ RenderAspectRatio (390 строк, 17 тестов)
- ✅ BoxDecorationPainter (180 строк, 6 тестов)

**Total Progress:**
- **701 тест** (525 flui_types + 49 flui_core + 99 flui_rendering + 27 flui_animation + 1 flui_foundation)
- **6 RenderObjects** готовы (RenderFlex, RenderPadding, RenderStack, RenderConstrainedBox, RenderDecoratedBox, RenderAspectRatio)
- **~19600 строк кода**

---

## 📋 Week 3: Simple Layout RenderObjects (19-26 января)

### Priority 1: RenderLimitedBox (~150 строк, 8 тестов)

**Время:** 1 день

**Описание:** Ограничивает размер child при unbounded constraints

**Алгоритм:**
```rust
fn layout(&mut self, constraints: BoxConstraints) -> Size {
    let child_constraints = BoxConstraints::new(
        constraints.min_width,
        if constraints.max_width.is_infinite() {
            self.max_width
        } else {
            constraints.max_width
        },
        constraints.min_height,
        if constraints.max_height.is_infinite() {
            self.max_height
        } else {
            constraints.max_height
        },
    );

    let child_size = child.layout(child_constraints);
    constraints.constrain(child_size)
}
```

**Примеры использования:**
- Ограничение размера текста в unbounded контексте
- Ограничение размера изображений
- Предотвращение бесконечных размеров

**Тесты:**
- Unbounded width → limited to maxWidth
- Unbounded height → limited to maxHeight
- Bounded constraints → pass through
- No child → smallest size

---

### Priority 2: RenderIndexedStack (~200 строк, 10 тестов)

**Время:** 1.5 дня

**Описание:** Stack, который показывает только один child по индексу

**Алгоритм:**
```rust
struct RenderIndexedStack {
    index: Option<usize>,
    alignment: Alignment,
    sizing: StackFit,
    children: Vec<Box<dyn RenderObject>>,
}

fn layout(&mut self, constraints: BoxConstraints) -> Size {
    // Layout ALL children (для правильного size calculation)
    // Но paint только child с индексом `index`

    let mut size = Size::zero();
    for (i, child) in self.children.iter_mut().enumerate() {
        let child_size = child.layout(loose_constraints);
        if Some(i) == self.index || self.index.is_none() {
            size = size.max(child_size); // Учитываем размер видимого
        }
    }

    constraints.constrain(size)
}

fn paint(&self, painter: &egui::Painter, offset: Offset) {
    // Paint только видимого child
    if let Some(index) = self.index {
        if let Some(child) = self.children.get(index) {
            child.paint(painter, offset);
        }
    }
}
```

**Примеры использования:**
- Tab navigation (показывать только активный tab)
- Wizard steps (показывать текущий шаг)
- Page view (показывать текущую страницу)

**Тесты:**
- Index 0 → shows first child
- Index out of bounds → shows nothing
- index = None → shows nothing
- Alignment with visible child
- StackFit::Loose vs Expand

---

### Priority 3: RenderPositionedBox (Align) (~180 строк, 10 тестов)

**Время:** 1.5 дня

**Описание:** Выравнивает child внутри доступного пространства

**Алгоритм:**
```rust
struct RenderPositionedBox {
    alignment: Alignment,
    width_factor: Option<f32>,  // Size = child.width * width_factor
    height_factor: Option<f32>,
    child: Option<Box<dyn RenderObject>>,
}

fn layout(&mut self, constraints: BoxConstraints) -> Size {
    let child_constraints = constraints.loosen();
    let child_size = child.layout(child_constraints);

    let width = if let Some(factor) = self.width_factor {
        (child_size.width * factor).max(constraints.min_width)
    } else {
        constraints.constrain_width(child_size.width)
    };

    let height = if let Some(factor) = self.height_factor {
        (child_size.height * factor).max(constraints.min_height)
    } else {
        constraints.constrain_height(child_size.height)
    };

    Size::new(width, height)
}

fn paint(&self, painter: &egui::Painter, offset: Offset) {
    // Calculate child offset based on alignment
    let child_offset = self.alignment.along_size(
        Size::new(self.size.width - child.size.width,
                  self.size.height - child.size.height)
    );

    child.paint(painter, offset + child_offset);
}
```

**Примеры использования:**
- Center widget (alignment = Alignment::CENTER)
- Align widget (любое выравнивание)
- Sized container (width_factor / height_factor)

**Тесты:**
- Alignment::CENTER
- Alignment::TOP_LEFT
- Alignment::BOTTOM_RIGHT
- width_factor = 2.0 → parent twice child width
- height_factor = 0.5 → parent half child height

---

### Priority 4: RenderFractionallySizedBox (~200 строк, 10 тестов)

**Время:** 1.5 дня

**Описание:** Размер child как процент от parent

**Алгоритм:**
```rust
struct RenderFractionallySizedBox {
    width_factor: Option<f32>,   // 0.0 to 1.0 (or > 1.0)
    height_factor: Option<f32>,
    alignment: Alignment,
    child: Option<Box<dyn RenderObject>>,
}

fn layout(&mut self, constraints: BoxConstraints) -> Size {
    let child_constraints = BoxConstraints::new(
        if let Some(factor) = self.width_factor {
            constraints.max_width * factor
        } else {
            0.0
        },
        if let Some(factor) = self.width_factor {
            constraints.max_width * factor
        } else {
            constraints.max_width
        },
        // Same for height
    );

    let child_size = child.layout(child_constraints);
    constraints.constrain(child_size)
}
```

**Примеры использования:**
- 50% width: `FractionallySizedBox(widthFactor: 0.5)`
- 75% height: `FractionallySizedBox(heightFactor: 0.75)`
- Responsive layouts

**Тесты:**
- widthFactor = 0.5 → child is 50% parent width
- heightFactor = 0.75 → child is 75% parent height
- widthFactor = None → child uses full width
- Alignment with smaller child

---

### Summary Week 3:
- **4 RenderObjects** (RenderLimitedBox, RenderIndexedStack, RenderPositionedBox, RenderFractionallySizedBox)
- **~730 строк кода**
- **38 тестов**
- **Итого после Week 3:** 10 RenderObjects, 137 тестов в flui_rendering

---

## 📋 Week 4: Complex Layout & Visual Effects (27 января - 2 февраля)

### Priority 5: RenderOpacity (~150 строк, 8 тестов)

**Время:** 1 день

**Описание:** Применяет opacity к child

**Алгоритм:**
```rust
struct RenderOpacity {
    opacity: f32,  // 0.0 to 1.0
    child: Option<Box<dyn RenderObject>>,
}

fn paint(&self, painter: &egui::Painter, offset: Offset) {
    // Set opacity on painter
    let old_opacity = painter.opacity();
    painter.set_opacity(self.opacity);

    child.paint(painter, offset);

    painter.set_opacity(old_opacity);
}
```

**Тесты:**
- Opacity 0.0 → invisible
- Opacity 1.0 → fully visible
- Opacity 0.5 → semi-transparent

---

### Priority 6: RenderTransform (~250 строк, 12 тестов)

**Время:** 2 дня

**Описание:** Применяет 2D трансформации (translate, rotate, scale)

**Алгоритм:**
```rust
struct RenderTransform {
    transform: Transform,
    alignment: Alignment,
    child: Option<Box<dyn RenderObject>>,
}

enum Transform {
    Translate(Offset),
    Rotate(f32),  // radians
    Scale(f32, f32),
    Matrix(Matrix3),
}

fn paint(&self, painter: &egui::Painter, offset: Offset) {
    // Apply transform
    let transform_offset = match &self.transform {
        Transform::Translate(t) => offset + *t,
        Transform::Rotate(angle) => {
            // Rotate around alignment point
            let pivot = self.alignment.along_size(self.size);
            // ... rotation math
        },
        // etc
    };

    child.paint(painter, transform_offset);
}
```

**Примеры использования:**
- Анимированные трансформации
- Rotate widget
- Scale widget

**Тесты:**
- Translate by (10, 20)
- Rotate 90 degrees
- Scale 2x
- Combined transforms

---

### Priority 7: RenderClipRRect (~200 строк, 10 тестов)

**Время:** 1.5 дня

**Описание:** Обрезает child по rounded rectangle

**Алгоритм:**
```rust
struct RenderClipRRect {
    border_radius: BorderRadius,
    clip_behavior: Clip,
    child: Option<Box<dyn RenderObject>>,
}

enum Clip {
    None,
    HardEdge,
    AntiAlias,
    AntiAliasWithSaveLayer,
}

fn paint(&self, painter: &egui::Painter, offset: Offset) {
    // Set clip rect with border radius
    let rect = egui::Rect::from_min_size(
        offset.to_pos2(),
        self.size.to_vec2(),
    );

    let rounding = self.border_radius.to_egui_rounding();

    painter.clip_rect_rounded(rect, rounding, |painter| {
        child.paint(painter, offset);
    });
}
```

**Примеры использования:**
- Rounded image containers
- Rounded card corners
- Clipping overflow content

**Тесты:**
- Circular clipping (all corners same)
- Different corner radii
- Clip::None vs HardEdge
- Child larger than clip area

---

### Summary Week 4:
- **3 RenderObjects** (RenderOpacity, RenderTransform, RenderClipRRect)
- **~600 строк кода**
- **30 тестов**
- **Итого после Week 4:** 13 RenderObjects, 167 тестов в flui_rendering

---

## 🎯 Goals After 2 Weeks

### RenderObjects Completed:
1. ✅ RenderFlex (Row/Column)
2. ✅ RenderPadding
3. ✅ RenderStack (Positioned)
4. ✅ RenderConstrainedBox (SizedBox)
5. ✅ RenderDecoratedBox
6. ✅ RenderAspectRatio
7. ⏳ RenderLimitedBox
8. ⏳ RenderIndexedStack
9. ⏳ RenderPositionedBox (Align/Center)
10. ⏳ RenderFractionallySizedBox
11. ⏳ RenderOpacity
12. ⏳ RenderTransform
13. ⏳ RenderClipRRect

### Statistics After 2 Weeks:
- **13 RenderObjects** (было 6)
- **~167 тестов** в flui_rendering (было 99)
- **~5000 строк кода** в flui_rendering (было ~3150)
- **~770 тестов** total (было 701)

### Next Phase After Week 4:
- **flui_widgets** - начать реализацию виджетов
- **Widget implementations** - Container, Row, Column, SizedBox, Padding, etc.
- **Integration tests** - создать примеры использования

---

## 📊 Success Metrics

### Week 3 Goals:
- [ ] 4 новых RenderObjects (RenderLimitedBox, RenderIndexedStack, RenderPositionedBox, RenderFractionallySizedBox)
- [ ] +38 тестов
- [ ] 0 clippy warnings
- [ ] Все тесты проходят

### Week 4 Goals:
- [ ] 3 новых RenderObjects (RenderOpacity, RenderTransform, RenderClipRRect)
- [ ] +30 тестов
- [ ] 0 clippy warnings
- [ ] Начать документацию для виджетов

### Overall 2-Week Goals:
- [ ] **13 RenderObjects** total (текущий прогресс: 6/13)
- [ ] **167 тестов** в flui_rendering (текущий прогресс: 99/167)
- [ ] **100%** основных layout RenderObjects готовы
- [ ] **60%** visual effects RenderObjects готовы
- [ ] Ready to start flui_widgets implementation

---

## 🚀 Long-Term Vision (Week 5-8)

### Week 5-6: flui_widgets - Basic Widgets
- Container, Row, Column, Padding, SizedBox, Center, Align
- Expanded, Flexible, Spacer
- DecoratedBox, AspectRatio, LimitedBox

### Week 7-8: flui_widgets - Interactive Widgets
- GestureDetector, InkWell
- Button, IconButton
- Text (basic), RichText

### Week 9-10: Examples & Integration
- Hello World example
- Counter example (StatefulWidget)
- Layout showcase
- Styling showcase

---

**Last Updated:** 2025-01-18
**Version:** 0.1.0-alpha
**Phase:** Week 3-4 Planning
**Next Review:** 2025-02-02 (After Week 4)

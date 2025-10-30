# 🗺️ Flutter Widgets Implementation Roadmap

> Поэтапный план реализации виджетов Flutter для Flui в правильном порядке

## 📊 Текущий статус

### ✅ Реализовано
- **flui_types** (13677 строк, 524 теста) - Все базовые типы
- **flui_core** (25000+ строк, 442 теста) - Widget → Element → RenderObject архитектура

### ❌ Требует реализации
- **flui_rendering** - RenderObject implementations
- **flui_widgets** - Widget implementations
- **flui_material** - Material Design components

---

## 🎯 Фазы реализации

# Phase 0: Foundation (RenderObject System)

> **Цель:** Создать базовую систему рендеринга, на которой будут строиться все виджеты

## 0.1 Core RenderObject Infrastructure (КРИТИЧНО!)

### Приоритет: P0 (Блокирует всё)

**Что нужно:**

```rust
// flui_rendering/src/lib.rs
pub mod render_object;
pub mod layer;

pub mod paint;
pub mod constraints;
```

### 1. RenderBox базовый trait ⏰ 1 неделя
```rust
pub trait RenderBox: RenderObject {
    fn compute_intrinsic_width(&self, height: f64) -> f64;
    fn compute_intrinsic_height(&self, width: f64) -> f64;
    fn compute_min_intrinsic_width(&self, height: f64) -> f64;
    fn compute_max_intrinsic_width(&self, height: f64) -> f64;
    fn hit_test(&self, position: Offset) -> bool;
}
```

### 2. Layer System ⏰ 3 дня
```rust
pub trait Layer {
    fn composite(&self, context: &CompositeContext);
}

pub struct ContainerLayer {
    children: Vec<Box<dyn Layer>>,
}

pub struct PictureLayer {
    picture: Picture,
}

pub struct TransformLayer {
    transform: Matrix4,
    child: Box<dyn Layer>,
}

pub struct OpacityLayer {
    opacity: f64,
    child: Box<dyn Layer>,
}
```

### 3. PaintContext ⏰ 2 дня
```rust
pub struct PaintContext {
    canvas: Canvas,
    offset: Offset,
}

impl PaintContext {
    pub fn push_offset(&mut self, offset: Offset);
    pub fn pop_offset(&mut self);
    pub fn push_clip_rect(&mut self, rect: Rect);
    pub fn push_opacity(&mut self, opacity: f64);
}
```

**Итого Phase 0.1:** ~10 дней

---

# Phase 1: Leaf RenderObjects (Примитивы)

> **Цель:** Виджеты без детей - основа для всех остальных

## Priority: P1 (Высший приоритет)

### 1.1 RenderColoredBox ⏰ 1 день
```rust
pub struct RenderColoredBox {
    color: Color,
    size: Size,
}

impl RenderObject for RenderColoredBox {
    type Arity = LeafArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Занимает всё доступное пространство
        cx.constraints().biggest()
    }
    
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Рисует прямоугольник с цветом
    }
}
```

**Виджет:**
```rust
pub struct ColoredBox {
    color: Color,
    child: Option<BoxedWidget>,
}
```

### 1.2 RenderSizedBox ⏰ 1 день
```rust
pub struct RenderSizedBox {
    width: Option<f64>,
    height: Option<f64>,
}
```

**Виджет:**
```rust
pub struct SizedBox {
    width: Option<f64>,
    height: Option<f64>,
    child: Option<BoxedWidget>,
}
```

### 1.3 RenderParagraph (Text) ⏰ 5 дней
```rust
pub struct RenderParagraph {
    text: String,
    style: TextStyle,
    text_painter: TextPainter,
}
```

**Виджет:**
```rust
pub struct Text {
    data: String,
    style: Option<TextStyle>,
}
```

**Зависимости:**
- Text shaping (harfbuzz)
- Font rendering (fontdue или ab_glyph)
- Text layout

### 1.4 RenderImage ⏰ 3 дня
```rust
pub struct RenderImage {
    image: Image,
    width: Option<f64>,
    height: Option<f64>,
    fit: BoxFit,
}
```

**Виджет:**
```rust
pub struct Image {
    image: ImageProvider,
    width: Option<f64>,
    height: Option<f64>,
    fit: BoxFit,
}
```

**Итого Phase 1:** ~10 дней

---

# Phase 2: Single-Child Layout RenderObjects

> **Цель:** Контейнеры с одним ребёнком - основа композиции

## Priority: P1

### 2.1 RenderPadding ⏰ 1 день
```rust
pub struct RenderPadding {
    padding: EdgeInsets,
}

impl RenderObject for RenderPadding {
    type Arity = SingleArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();
        
        // Уменьшаем constraints на padding
        let child_constraints = cx.constraints()
            .deflate(self.padding);
        
        // Layout child
        let child_size = cx.layout_child(child, child_constraints);
        
        // Добавляем padding к размеру
        Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical()
        )
    }
    
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let child = cx.child();
        
        // Paint child со смещением
        let mut layer = ContainerLayer::new();
        layer.add_child_at_offset(
            cx.capture_child_layer(child),
            Offset::new(self.padding.left, self.padding.top)
        );
        Box::new(layer)
    }
}
```

**Виджет:**
```rust
pub struct Padding {
    padding: EdgeInsets,
    child: BoxedWidget,
}
```

### 2.2 RenderAlign / RenderCenter ⏰ 2 дня
```rust
pub struct RenderAlign {
    alignment: Alignment,
}

pub struct RenderCenter;  // Alias for Align(center)
```

### 2.3 RenderConstrainedBox ⏰ 1 день
```rust
pub struct RenderConstrainedBox {
    additional_constraints: BoxConstraints,
}
```

### 2.4 RenderAspectRatio ⏰ 1 день
```rust
pub struct RenderAspectRatio {
    aspect_ratio: f64,
}
```

### 2.5 RenderFittedBox ⏰ 2 дня
```rust
pub struct RenderFittedBox {
    fit: BoxFit,
    alignment: Alignment,
}
```

### 2.6 RenderDecoratedBox ⏰ 3 дня
```rust
pub struct RenderDecoratedBox {
    decoration: BoxDecoration,
}
```

**Зависимости:**
- Border rendering
- Shadow rendering
- Gradient rendering

### 2.7 RenderOpacity ⏰ 1 день
```rust
pub struct RenderOpacity {
    opacity: f64,
}
```

### 2.8 RenderTransform ⏰ 2 дня
```rust
pub struct RenderTransform {
    transform: Matrix4,
}
```

### 2.9 RenderClipRect / RenderClipRRect ⏰ 2 дня
```rust
pub struct RenderClipRect;

pub struct RenderClipRRect {
    border_radius: BorderRadius,
}
```

**Итого Phase 2:** ~15 дней

---

# Phase 3: Multi-Child Layout RenderObjects

> **Цель:** Flex layouts (Row, Column) - самые используемые виджеты

## Priority: P1

### 3.1 RenderFlex (Row/Column base) ⏰ 7 дней
```rust
pub struct RenderFlex {
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
}

impl RenderObject for RenderFlex {
    type Arity = MultiArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let children = cx.children();
        
        // 1. Layout flexible children
        // 2. Distribute space
        // 3. Layout inflexible children
        // 4. Position children
        // 5. Compute total size
    }
    
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Paint children at computed offsets
    }
}
```

**Виджеты:**
```rust
pub struct Row {
    children: Vec<BoxedWidget>,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
}

pub struct Column {
    children: Vec<BoxedWidget>,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
}

pub struct Flexible {
    flex: i32,
    fit: FlexFit,
    child: BoxedWidget,
}

pub struct Expanded {
    flex: i32,
    child: BoxedWidget,
}
```

**Сложность:**
- Flex algorithm (Flutter specification)
- Baseline alignment
- Text direction handling

### 3.2 RenderStack ⏰ 5 дней
```rust
pub struct RenderStack {
    alignment: Alignment,
    fit: StackFit,
}
```

**Виджеты:**
```rust
pub struct Stack {
    children: Vec<BoxedWidget>,
    alignment: Alignment,
}

pub struct Positioned {
    left: Option<f64>,
    top: Option<f64>,
    right: Option<f64>,
    bottom: Option<f64>,
    child: BoxedWidget,
}
```

### 3.3 RenderWrap ⏰ 5 дней
```rust
pub struct RenderWrap {
    direction: Axis,
    alignment: WrapAlignment,
    spacing: f64,
    run_spacing: f64,
}
```

**Итого Phase 3:** ~17 дней

---

# Phase 4: Composite Widgets (Stateless)

> **Цель:** Высокоуровневые виджеты из комбинаций RenderObjects

## Priority: P2

### 4.1 Container ⏰ 2 дня
```rust
pub struct Container {
    padding: Option<EdgeInsets>,
    margin: Option<EdgeInsets>,
    color: Option<Color>,
    decoration: Option<BoxDecoration>,
    width: Option<f64>,
    height: Option<f64>,
    constraints: Option<BoxConstraints>,
    alignment: Option<Alignment>,
    child: Option<BoxedWidget>,
}

impl StatelessWidget for Container {
    fn build(&self, context: &BuildContext) -> BoxedWidget {
        let mut child = self.child.clone();
        
        // Применяем слои изнутри наружу
        
        // 1. Alignment
        if let Some(alignment) = self.alignment {
            child = Some(Box::new(Align {
                alignment,
                child: child.unwrap(),
            }));
        }
        
        // 2. Padding
        if let Some(padding) = self.padding {
            child = Some(Box::new(Padding {
                padding,
                child: child.unwrap(),
            }));
        }
        
        // 3. Decoration
        if let Some(decoration) = self.decoration {
            child = Some(Box::new(DecoratedBox {
                decoration,
                child: child.unwrap(),
            }));
        } else if let Some(color) = self.color {
            child = Some(Box::new(ColoredBox {
                color,
                child: Some(child.unwrap()),
            }));
        }
        
        // 4. Constraints
        if let Some(constraints) = self.constraints {
            child = Some(Box::new(ConstrainedBox {
                constraints,
                child: child.unwrap(),
            }));
        }
        
        // 5. Margin
        if let Some(margin) = self.margin {
            child = Some(Box::new(Padding {
                padding: margin,
                child: child.unwrap(),
            }));
        }
        
        child.unwrap()
    }
}
```

### 4.2 Card ⏰ 1 день
```rust
pub struct Card {
    child: BoxedWidget,
    color: Option<Color>,
    elevation: f64,
}

impl StatelessWidget for Card {
    fn build(&self, context: &BuildContext) -> BoxedWidget {
        Box::new(Container {
            decoration: Some(BoxDecoration {
                color: self.color,
                border_radius: Some(BorderRadius::circular(4.0)),
                box_shadow: compute_elevation_shadow(self.elevation),
            }),
            child: Some(self.child.clone()),
            ..Default::default()
        })
    }
}
```

**Итого Phase 4:** ~3 дня

---

# Phase 5: Interaction & Gesture Detection

> **Цель:** Сделать UI интерактивным

## Priority: P2

### 5.1 RenderPointerListener ⏰ 3 дня
```rust
pub struct RenderPointerListener {
    on_pointer_down: Option<Box<dyn Fn(PointerEvent)>>,
    on_pointer_up: Option<Box<dyn Fn(PointerEvent)>>,
    on_pointer_move: Option<Box<dyn Fn(PointerEvent)>>,
}
```

### 5.2 GestureDetector ⏰ 5 дней
```rust
pub struct GestureDetector {
    on_tap: Option<Box<dyn Fn()>>,
    on_double_tap: Option<Box<dyn Fn()>>,
    on_long_press: Option<Box<dyn Fn()>>,
    on_pan_update: Option<Box<dyn Fn(DragUpdateDetails)>>,
    child: BoxedWidget,
}
```

**Зависимости:**
- Gesture arena
- Gesture recognizers (Tap, Pan, Scale, etc.)

### 5.3 InkWell (Material Ripple) ⏰ 3 дня
```rust
pub struct InkWell {
    on_tap: Option<Box<dyn Fn()>>,
    border_radius: Option<BorderRadius>,
    splash_color: Color,
    child: BoxedWidget,
}
```

**Итого Phase 5:** ~11 дней

---

# Phase 6: Scrolling Widgets

> **Цель:** Scrollable контент

## Priority: P2

### 6.1 SingleChildScrollView ⏰ 5 дней
```rust
pub struct RenderSingleChildScrollView {
    axis: Axis,
    scroll_offset: f64,
}
```

### 6.2 ListView.builder ⏰ 7 дней
```rust
pub struct RenderSliverList {
    delegate: SliverChildDelegate,
}
```

### 6.3 GridView ⏰ 5 дней
```rust
pub struct RenderSliverGrid {
    grid_delegate: SliverGridDelegate,
}
```

**Итого Phase 6:** ~17 дней

---

# Phase 7: Material Design Basics

> **Цель:** Основные Material компоненты

## Priority: P2

### 7.1 Material ⏰ 2 дня
```rust
pub struct Material {
    type_: MaterialType,
    elevation: f64,
    color: Color,
    child: BoxedWidget,
}
```

### 7.2 Scaffold ⏰ 5 дней
```rust
pub struct Scaffold {
    app_bar: Option<BoxedWidget>,
    body: BoxedWidget,
    floating_action_button: Option<BoxedWidget>,
    bottom_navigation_bar: Option<BoxedWidget>,
}
```

### 7.3 AppBar ⏰ 3 дня
```rust
pub struct AppBar {
    title: BoxedWidget,
    actions: Vec<BoxedWidget>,
    elevation: f64,
}
```

### 7.4 TextButton / ElevatedButton / OutlinedButton ⏰ 4 дня
```rust
pub struct TextButton {
    on_pressed: Option<Box<dyn Fn()>>,
    child: BoxedWidget,
    style: ButtonStyle,
}
```

### 7.5 FloatingActionButton ⏰ 2 дня
```rust
pub struct FloatingActionButton {
    on_pressed: Box<dyn Fn()>,
    child: BoxedWidget,
    background_color: Color,
}
```

**Итого Phase 7:** ~16 дней

---

# Phase 8: Input Widgets

> **Цель:** Формы и ввод данных

## Priority: P2

### 8.1 TextField ⏰ 10 дней
```rust
pub struct TextField {
    controller: TextEditingController,
    decoration: InputDecoration,
    style: TextStyle,
}
```

**Сложность:**
- Text editing
- Cursor management
- Selection handling
- IME integration

### 8.2 Checkbox ⏰ 2 дня
```rust
pub struct Checkbox {
    value: bool,
    on_changed: Box<dyn Fn(bool)>,
}
```

### 8.3 Radio ⏰ 2 дня
```rust
pub struct Radio<T> {
    value: T,
    group_value: T,
    on_changed: Box<dyn Fn(T)>,
}
```

### 8.4 Switch ⏰ 2 дня
```rust
pub struct Switch {
    value: bool,
    on_changed: Box<dyn Fn(bool)>,
}
```

### 8.5 Slider ⏰ 3 дня
```rust
pub struct Slider {
    value: f64,
    min: f64,
    max: f64,
    on_changed: Box<dyn Fn(f64)>,
}
```

**Итого Phase 8:** ~19 дней

---

# Phase 9: Navigation & Routing

> **Цель:** Multi-page приложения

## Priority: P2

### 9.1 Navigator ⏰ 7 дней
```rust
pub struct Navigator {
    pages: Vec<Page>,
    on_pop_page: Box<dyn Fn(&Route) -> bool>,
}
```

### 9.2 MaterialPageRoute ⏰ 3 дня
```rust
pub struct MaterialPageRoute {
    builder: Box<dyn Fn(&BuildContext) -> BoxedWidget>,
}
```

### 9.3 Hero ⏰ 5 дней
```rust
pub struct Hero {
    tag: Object,
    child: BoxedWidget,
}
```

**Итого Phase 9:** ~15 дней

---

# Phase 10: Advanced Widgets

> **Цель:** Продвинутые виджеты

## Priority: P3

### 10.1 CustomPaint ⏰ 3 дня
```rust
pub struct CustomPaint {
    painter: Box<dyn CustomPainter>,
    child: Option<BoxedWidget>,
}
```

### 10.2 AnimatedBuilder ⏰ 2 дня
```rust
pub struct AnimatedBuilder {
    animation: Animation<f64>,
    builder: Box<dyn Fn(&BuildContext, Widget) -> BoxedWidget>,
}
```

### 10.3 FutureBuilder / StreamBuilder ⏰ 3 дня
```rust
pub struct FutureBuilder<T> {
    future: Future<T>,
    builder: Box<dyn Fn(&BuildContext, AsyncSnapshot<T>) -> BoxedWidget>,
}
```

**Итого Phase 10:** ~8 дней

---

## 📊 Суммарная оценка по фазам

| Фаза | Описание | Приоритет | Время | Статус |
|------|----------|-----------|-------|--------|
| **Phase 0** | RenderObject Foundation | P0 | 10 дней | ❌ |
| **Phase 1** | Leaf RenderObjects | P1 | 10 дней | ❌ |
| **Phase 2** | Single-Child Layouts | P1 | 15 дней | ❌ |
| **Phase 3** | Multi-Child Layouts | P1 | 17 дней | ❌ |
| **Phase 4** | Composite Widgets | P2 | 3 дня | ❌ |
| **Phase 5** | Interaction | P2 | 11 дней | ❌ |
| **Phase 6** | Scrolling | P2 | 17 дней | ❌ |
| **Phase 7** | Material Basics | P2 | 16 дней | ❌ |
| **Phase 8** | Input Widgets | P2 | 19 дней | ❌ |
| **Phase 9** | Navigation | P2 | 15 дней | ❌ |
| **Phase 10** | Advanced | P3 | 8 дней | ❌ |
| **ИТОГО** | | | **141 день** (~7 месяцев) | |

---

## 🎯 Критический путь (MVP)

Для минимального работающего приложения нужны:

### Milestone 1: "Hello World" (26 дней)
- Phase 0: Foundation (10 дней)
- Phase 1: Leaf RenderObjects (10 дней)
- Phase 2.1-2.3: Padding, Align, Constraints (4 дня)
- Phase 3.1: Flex (Row/Column) (7 дней) - начать параллельно с Phase 2
- Phase 4.1: Container (2 дня)

**Результат:** Можно создать simple UI с Text, Container, Row, Column

### Milestone 2: "Interactive App" (+20 дней)
- Phase 5: Interaction (11 дней)
- Phase 7.1-7.4: Material + Buttons (11 дней)

**Результат:** Кнопки работают, Material Design

### Milestone 3: "Real App" (+27 дней)
- Phase 6.1: SingleChildScrollView (5 дней)
- Phase 8.1-8.2: TextField, Checkbox (12 дней)
- Phase 9.1-9.2: Navigator (10 дней)

**Результат:** Multi-page app с формами

**ИТОГО до MVP:** ~73 дня (~3.5 месяца)

---

## 🚀 Рекомендуемый порядок работы

### Неделя 1-2: Foundation
1. RenderBox trait
2. Layer system
3. PaintContext
4. **Цель:** RenderObject infrastructure готов

### Неделя 3-4: Primitives
1. RenderColoredBox
2. RenderSizedBox
3. RenderParagraph (Text)
4. **Цель:** Можно показать "Hello World"

### Неделя 5-7: Single-Child Layouts
1. RenderPadding
2. RenderAlign / RenderCenter
3. RenderConstrainedBox
4. RenderDecoratedBox
5. **Цель:** Container работает

### Неделя 8-10: Multi-Child Layouts
1. RenderFlex (Row/Column)
2. Flexible/Expanded
3. RenderStack/Positioned
4. **Цель:** Complex layouts работают

### Неделя 11-12: Composite & Interaction
1. Container widget
2. GestureDetector
3. InkWell
4. **Цель:** UI интерактивный

### Неделя 13-15: Material Basics
1. Material widget
2. Scaffold
3. AppBar
4. Buttons (Text, Elevated, Outlined, FAB)
5. **Цель:** Material Design app

### Неделя 16-18: Input & Forms
1. TextField
2. Checkbox/Radio/Switch
3. Slider
4. **Цель:** Формы работают

### Неделя 19-20: Navigation
1. Navigator
2. MaterialPageRoute
3. Hero transitions
4. **Цель:** Multi-page app

---

## 💡 Советы по реализации

### 1. Начинайте с тестов
```rust
#[test]
fn test_padding_layout() {
    let mut render = RenderPadding {
        padding: EdgeInsets::all(10.0),
    };
    
    // Test layout logic
}
```

### 2. Используйте Flutter как референс
- Читайте Flutter source code
- Копируйте алгоритмы layout
- Тестируйте против Flutter поведения

### 3. Incremental development
- Одна фича за раз
- Тесты после каждой фичи
- Коммит после зелёных тестов

### 4. Performance с самого начала
- Profile после каждого milestone
- Layout cache критичен
- Избегайте лишних allocations

### 5. Документация
- Документируйте каждый RenderObject
- Примеры использования
- Диаграммы layout algorithm

---

## 📚 Ресурсы

### Flutter Source Code
- [framework/lib/src/rendering/](https://github.com/flutter/flutter/tree/master/packages/flutter/lib/src/rendering)
- [framework/lib/src/widgets/](https://github.com/flutter/flutter/tree/master/packages/flutter/lib/src/widgets)

### Документация
- [Flutter Layout Algorithm](https://docs.flutter.dev/ui/layout)
- [RenderObject Deep Dive](https://flutter.dev/docs/resources/architectural-overview#rendering-and-layout)

### Полезные статьи
- "Understanding Flutter's Layout" (Medium)
- "How Flutter Renders Widgets" (Flutter.dev)

---

## ✅ Чеклист готовности к следующей фазе

### Before Phase 1:
- [ ] RenderBox trait работает
- [ ] Layer system реализован
- [ ] PaintContext функционален
- [ ] Есть integration tests

### Before Phase 2:
- [ ] Text рендерится
- [ ] Image рендерится
- [ ] Простые RenderObjects работают

### Before Phase 3:
- [ ] Single-child layout работает
- [ ] Padding/Align/Constraints протестированы
- [ ] Container готов

### Before Phase 4:
- [ ] Flex layout работает
- [ ] Row/Column рендерятся
- [ ] Stack/Positioned работает

### Before Phase 5:
- [ ] Composite widgets работают
- [ ] Container полностью функционален

---

**🎉 Удачи в реализации!** Следуйте roadmap, делайте небольшие коммиты, и через 3-4 месяца у вас будет работающий Flutter-like фреймворк на Rust!

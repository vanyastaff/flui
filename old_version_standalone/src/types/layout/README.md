# Layout Types

Типы для управления layout, spacing, alignment и constraints в UI системе.

## Обзор

Модуль `layout` содержит типы для построения гибких и отзывчивых пользовательских интерфейсов. Все типы интегрированы с [core types](../core/README.md) и используют идиоматичный Rust с `impl Into<T>`.

## Категории типов

### 📦 Spacing (Отступы)

#### [`EdgeInsets`](edge_insets.rs)
Универсальные отступы для всех четырех сторон.

```rust
use nebula_ui::types::layout::EdgeInsets;
use nebula_ui::types::core::{Rect, Size, Point};

// Создание
let insets = EdgeInsets::all(10.0);
let insets = EdgeInsets::symmetric(20.0, 10.0);  // horizontal, vertical
let insets = EdgeInsets::new(5.0, 10.0, 15.0, 20.0);  // L, T, R, B

// Константы
EdgeInsets::ZERO;

// Применение к Rect
let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
let inner = insets.deflate_rect(rect);  // уменьшить
let outer = insets.inflate_rect(rect);  // увеличить

// Применение к Size
let size = Size::new(100.0, 100.0);
let content_size = insets.shrink_size(size);
let total_size = insets.expand_size(size);

// Totals
let h_total = insets.horizontal_total();  // left + right
let v_total = insets.vertical_total();    // top + bottom
let total = insets.total_size();          // Size

// Extension trait
use nebula_ui::types::layout::EdgeInsetsExt;
let shrunk = rect.shrink_by(insets);
let expanded = size.expand_by(insets);
```

**Использование**: Универсальный тип для любых отступов

#### [`Padding`](padding.rs)
Внутренние отступы (internal spacing).

```rust
use nebula_ui::types::layout::Padding;

// Создание
let padding = Padding::all(8.0);
let padding = Padding::symmetric(12.0, 8.0);
let padding = Padding::horizontal(12.0);
let padding = Padding::vertical(8.0);

// Предопределенные константы
Padding::ZERO;
Padding::SMALL;        // 4px
Padding::MEDIUM;       // 8px
Padding::LARGE;        // 16px
Padding::EXTRA_LARGE;  // 24px

// Применение
let content_rect = padding.shrink_rect(container_rect);
let content_size = padding.shrink_size(container_size);

// Interpolation
let animated = Padding::lerp(start, end, t);
```

**Семантика**: Внутренние отступы элемента (как CSS padding)
**Отличие от Margin**: Padding - внутри элемента, Margin - снаружи

#### [`Margin`](margin.rs)
Внешние отступы (external spacing).

```rust
use nebula_ui::types::layout::Margin;

// Создание
let margin = Margin::all(10.0);
let margin = Margin::symmetric(20.0, 10.0);
let margin = Margin::horizontal(20.0);
let margin = Margin::vertical(10.0);

// Константы
Margin::ZERO;

// Применение
let outer_rect = margin.expand_rect(element_rect);
let total_size = margin.expand_size(element_size);

// Конвертация с egui
let egui_margin = margin.to_egui_margin();
let our_margin = Margin::from_egui_margin(egui_margin);
```

**Семантика**: Внешние отступы вокруг элемента (как CSS margin)

#### [`Spacing`](spacing.rs)
Стандартизированная шкала spacing.

```rust
use nebula_ui::types::layout::Spacing;

// Предопределенная шкала
Spacing::XXS;    // 2px
Spacing::XS;     // 4px
Spacing::S;      // 8px
Spacing::M;      // 12px
Spacing::L;      // 16px
Spacing::XL;     // 24px
Spacing::XXL;    // 32px
Spacing::XXXL;   // 48px

// Создание custom spacing
let custom = Spacing::from_pixels(15.0);

// Операции
let pixels = spacing.to_pixels();
let larger = spacing.larger();
let smaller = spacing.smaller();
let scaled = spacing.scale(1.5);

// Display
format!("{}", Spacing::M);  // "12px"
```

**Использование**: Cons

истентные отступы в UI системе

### 📐 Alignment (Выравнивание)

#### [`Alignment`](alignment.rs)
2D выравнивание элемента в контейнере.

```rust
use nebula_ui::types::layout::Alignment;

// Предопределенные константы
Alignment::TOP_LEFT;
Alignment::TOP_CENTER;
Alignment::TOP_RIGHT;
Alignment::CENTER_LEFT;
Alignment::CENTER;
Alignment::CENTER_RIGHT;
Alignment::BOTTOM_LEFT;
Alignment::BOTTOM_CENTER;
Alignment::BOTTOM_RIGHT;

// Создание custom
let alignment = Alignment::new(-0.5, 0.5);  // x, y от -1.0 до 1.0

// Вычисление offset
let offset = alignment.along_size(container_size);

// Применение к Rect
let positioned = alignment.inscribe(child_size, container_rect);
```

**Координаты**:
- `-1.0` = left/top
- `0.0` = center
- `1.0` = right/bottom

#### [`MainAxisAlignment`](alignment.rs)
Выравнивание вдоль главной оси (для Flex layout).

```rust
use nebula_ui::types::layout::MainAxisAlignment;

MainAxisAlignment::Start;           // В начале
MainAxisAlignment::End;             // В конце
MainAxisAlignment::Center;          // По центру
MainAxisAlignment::SpaceBetween;    // Пространство между элементами
MainAxisAlignment::SpaceAround;     // Пространство вокруг элементов
MainAxisAlignment::SpaceEvenly;     // Равномерное пространство

// Spacing calculation
let spacing = alignment.spacing(
    container_size,
    total_children_size,
    child_count,
);
```

#### [`CrossAxisAlignment`](alignment.rs)
Выравнивание вдоль поперечной оси.

```rust
use nebula_ui::types::layout::CrossAxisAlignment;

CrossAxisAlignment::Start;    // В начале
CrossAxisAlignment::End;      // В конце
CrossAxisAlignment::Center;   // По центру
CrossAxisAlignment::Stretch;  // Растянуть
CrossAxisAlignment::Baseline; // По базовой линии текста
```

### 📏 Flex Layout

#### [`FlexDirection`](flex.rs)
Направление flex контейнера.

```rust
use nebula_ui::types::layout::FlexDirection;

FlexDirection::Row;            // Горизонтально →
FlexDirection::RowReverse;     // Горизонтально ←
FlexDirection::Column;         // Вертикально ↓
FlexDirection::ColumnReverse;  // Вертикально ↑

// Утилиты
let axis = direction.to_axis();
let is_reversed = direction.is_reversed();
let opposite = direction.opposite();
```

#### [`FlexFit`](flex.rs)
Как flex item заполняет доступное пространство.

```rust
use nebula_ui::types::layout::FlexFit;

FlexFit::Tight;   // Заполнить все пространство
FlexFit::Loose;   // Использовать минимум необходимого
```

#### [`FlexWrap`](flex.rs)
Поведение переноса flex items.

```rust
use nebula_ui::types::layout::FlexWrap;

FlexWrap::NoWrap;       // Не переносить
FlexWrap::Wrap;         // Переносить
FlexWrap::WrapReverse;  // Переносить в обратном порядке
```

### 🎯 Constraints & Sizing

#### [`BoxConstraints`](layout.rs)
Ограничения размера для layout.

```rust
use nebula_ui::types::layout::BoxConstraints;
use nebula_ui::types::core::Size;

// Создание
let constraints = BoxConstraints::new(
    min_width: 100.0,
    max_width: 300.0,
    min_height: 50.0,
    max_height: 200.0,
);

// Утилиты
let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
let tight_for = BoxConstraints::tight_for(width: Some(100.0), height: None);
let loose = BoxConstraints::loose(Size::new(300.0, 200.0));
let expand = BoxConstraints::expand();

// Проверки
let is_tight = constraints.is_tight();
let has_bounded_width = constraints.has_bounded_width();
let has_infinite_width = constraints.has_infinite_width();

// Применение
let constrained = constraints.constrain(size);
let width = constraints.constrain_width(width);
let height = constraints.constrain_height(height);

// Операции
let tightened = constraints.tighten(width: Some(150.0), height: None);
let loosened = constraints.loosen();
let enforced = constraints.enforce(other_constraints);
```

#### [`BoxFit`](layout.rs)
Как вписать объект в пространство.

```rust
use nebula_ui::types::layout::BoxFit;

BoxFit::Fill;        // Заполнить, игнорируя aspect ratio
BoxFit::Contain;     // Вписать полностью, сохраняя aspect ratio
BoxFit::Cover;       // Покрыть полностью, может обрезаться
BoxFit::FitWidth;    // По ширине
BoxFit::FitHeight;   // По высоте
BoxFit::None;        // Оригинальный размер
BoxFit::ScaleDown;   // Как Contain, но не увеличивать

// Применение
let fitted_size = box_fit.apply_box_fit(
    child_size,
    container_size,
);
```

### 📊 Aspect Ratio

#### [`AspectRatio`](aspect_ratio.rs)
Соотношение сторон (width / height).

```rust
use nebula_ui::types::layout::AspectRatio;
use nebula_ui::types::core::Size;

// Создание
let ratio = AspectRatio::new(16.0, 9.0);
let ratio = AspectRatio::from_ratio(1.777);
let ratio = AspectRatio::from_size(Size::new(1920.0, 1080.0));

// Общие соотношения
AspectRatio::SQUARE;      // 1:1
AspectRatio::WIDESCREEN;  // 16:9
AspectRatio::ULTRAWIDE;   // 21:9
AspectRatio::PORTRAIT;    // 9:16
AspectRatio::GOLDEN;      // φ:1

// Вычисления
let height = ratio.height_for_width(width);
let width = ratio.width_for_height(height);
let is_landscape = ratio.is_landscape();
let is_portrait = ratio.is_portrait();

// Операции
let inverted = ratio.inverse();  // height:width
let closest = AspectRatio::closest_standard(1.5);
```

### 🧭 Axis & Direction

#### [`Axis`](axis.rs)
Главная ось layout.

```rust
use nebula_ui::types::layout::Axis;
use nebula_ui::types::core::{Size, Offset};

Axis::Horizontal;
Axis::Vertical;

// Извлечение компонент
let main = axis.main(size);     // width или height
let cross = axis.cross(size);   // height или width

// Создание из компонент
let size = axis.pack(main: 100.0, cross: 50.0);  // Size
let offset = axis.pack_offset(main: 10.0, cross: 5.0);  // Offset

// Swap
let other = axis.flip();
```

#### [`AxisDirection`](axis.rs)
Направление вдоль оси.

```rust
use nebula_ui::types::layout::AxisDirection;

AxisDirection::Up;
AxisDirection::Down;
AxisDirection::Left;
AxisDirection::Right;

// Утилиты
let axis = direction.axis();
let is_reversed = direction.is_reversed();
let opposite = direction.opposite();
```

#### [`VerticalDirection`](axis.rs)
Вертикальное направление.

```rust
use nebula_ui::types::layout::VerticalDirection;

VerticalDirection::Up;    // Снизу вверх
VerticalDirection::Down;  // Сверху вниз
```

## Примеры использования

### Пример 1: Flex Layout

```rust
use nebula_ui::types::layout::*;
use nebula_ui::types::core::{Rect, Size};

fn layout_flex_children(
    container: Rect,
    children_sizes: &[Size],
    direction: FlexDirection,
    main_alignment: MainAxisAlignment,
    cross_alignment: CrossAxisAlignment,
) -> Vec<Rect> {
    let axis = direction.to_axis();
    let container_size = container.size();

    // Вычислить total size детей вдоль главной оси
    let total_main: f32 = children_sizes.iter()
        .map(|s| axis.main(*s))
        .sum();

    // Вычислить spacing
    let available = axis.main(container_size) - total_main;
    let spacing = main_alignment.spacing(
        axis.main(container_size),
        total_main,
        children_sizes.len(),
    );

    // Разместить детей
    let mut current_main = spacing.before;
    let mut result = Vec::new();

    for &child_size in children_sizes {
        // Позиция вдоль главной оси
        let main_pos = current_main;
        current_main += axis.main(child_size) + spacing.between;

        // Позиция вдоль поперечной оси
        let cross_pos = match cross_alignment {
            CrossAxisAlignment::Start => 0.0,
            CrossAxisAlignment::Center => {
                (axis.cross(container_size) - axis.cross(child_size)) * 0.5
            }
            CrossAxisAlignment::End => {
                axis.cross(container_size) - axis.cross(child_size)
            }
            CrossAxisAlignment::Stretch => 0.0,
            _ => 0.0,
        };

        // Создать rect
        let offset = axis.pack_offset(main_pos, cross_pos);
        let child_rect = Rect::from_min_size(
            container.min + offset,
            child_size,
        );
        result.push(child_rect);
    }

    result
}
```

### Пример 2: Центрирование элемента

```rust
use nebula_ui::types::layout::Alignment;
use nebula_ui::types::core::{Rect, Size};

fn center_element(
    element_size: Size,
    container: Rect,
) -> Rect {
    Alignment::CENTER.inscribe(element_size, container)
}

fn align_element(
    element_size: Size,
    container: Rect,
    alignment: Alignment,
) -> Rect {
    alignment.inscribe(element_size, container)
}
```

### Пример 3: Responsive sizing с constraints

```rust
use nebula_ui::types::layout::{BoxConstraints, BoxFit, AspectRatio};
use nebula_ui::types::core::Size;

fn responsive_image_size(
    image_size: Size,
    container_size: Size,
    maintain_aspect: bool,
) -> Size {
    let constraints = BoxConstraints::loose(container_size);

    if maintain_aspect {
        // Вписать с сохранением пропорций
        BoxFit::Contain.apply_box_fit(image_size, container_size)
    } else {
        // Просто ограничить размер
        constraints.constrain(image_size)
    }
}

fn fit_to_aspect_ratio(
    width: f32,
    ratio: AspectRatio,
    max_height: f32,
) -> Size {
    let height = ratio.height_for_width(width).min(max_height);
    let final_width = if height == max_height {
        ratio.width_for_height(height)
    } else {
        width
    };
    Size::new(final_width, height)
}
```

### Пример 4: Padding и Margin

```rust
use nebula_ui::types::layout::{Padding, Margin};
use nebula_ui::types::core::Rect;

fn apply_spacing(
    content: Rect,
    padding: Padding,
    margin: Margin,
) -> (Rect, Rect) {
    // Padding уменьшает внутреннее пространство
    let content_area = padding.shrink_rect(content);

    // Margin увеличивает внешнее пространство
    let total_area = margin.expand_rect(content);

    (content_area, total_area)
}

fn calculate_total_size(
    content_size: Size,
    padding: Padding,
    margin: Margin,
) -> Size {
    // Сначала добавляем padding
    let with_padding = padding.expand_size(content_size);

    // Потом добавляем margin
    let total = margin.expand_size(with_padding);

    total
}
```

### Пример 5: Standard spacing scale

```rust
use nebula_ui::types::layout::{Spacing, EdgeInsets};

fn create_card_spacing() -> EdgeInsets {
    EdgeInsets::new(
        Spacing::L.to_pixels(),   // left
        Spacing::M.to_pixels(),   // top
        Spacing::L.to_pixels(),   // right
        Spacing::M.to_pixels(),   // bottom
    )
}

fn vertical_stack_spacing() -> f32 {
    Spacing::S.to_pixels()
}

fn section_spacing() -> f32 {
    Spacing::XL.to_pixels()
}
```

## Интеграция с Core Types

Все layout типы полностью интегрированы с [core types](../core/README.md):

```rust
use nebula_ui::types::core::{Point, Size, Rect, Offset};
use nebula_ui::types::layout::{Padding, Margin, EdgeInsets, Alignment};

// Все методы принимают impl Into<T>
let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
let padding = Padding::all(10.0);

// Можно передавать как core типы
let inner1 = padding.shrink_rect(rect);

// Так и совместимые типы напрямую
let inner2 = padding.shrink_rect(egui::Rect::from_min_size(...));

// Size и Point работают везде
let size = Size::new(100.0, 50.0);
let expanded = padding.expand_size(size);
let expanded2 = padding.expand_size((100.0, 50.0));  // из tuple!
```

## Design Patterns

### 1. Composition Over Configuration

```rust
// ❌ Плохо - много параметров
fn layout(
    width: f32,
    height: f32,
    padding_left: f32,
    padding_right: f32,
    padding_top: f32,
    padding_bottom: f32,
    margin_left: f32,
    // ... еще 20 параметров
) { }

// ✅ Хорошо - композиция типов
fn layout(
    size: Size,
    padding: Padding,
    margin: Margin,
    alignment: Alignment,
) { }
```

### 2. Type Safety для семантики

```rust
// ❌ Легко перепутать
fn apply_spacing(inner: f32, outer: f32) { }
apply_spacing(margin, padding);  // Oops!

// ✅ Типы защищают
fn apply_spacing(padding: Padding, margin: Margin) { }
apply_spacing(margin, padding);  // ❌ Ошибка компиляции!
```

### 3. Предопределенные константы

```rust
// ✅ Используйте стандартные значения
Padding::MEDIUM;
Spacing::L;
AspectRatio::WIDESCREEN;
Alignment::CENTER;

// Создавайте custom только когда необходимо
let custom_padding = Padding::new(7.0, 13.0, 11.0, 17.0);
```

## Best Practices

### Spacing

1. **Используйте Spacing scale** для консистентности:
   ```rust
   // ✅ Хорошо
   let gap = Spacing::M.to_pixels();

   // ❌ Плохо - magic numbers
   let gap = 12.5;
   ```

2. **Padding vs Margin**:
   - `Padding` - для внутренних отступов элемента
   - `Margin` - для внешних отступов между элементами
   - `EdgeInsets` - когда семантика не важна

3. **Симметричность**:
   ```rust
   // Для симметричных отступов
   let padding = Padding::symmetric(horizontal, vertical);

   // Для полного контроля
   let padding = Padding::new(left, right, top, bottom);
   ```

### Alignment

1. **Используйте константы** для распространенных случаев:
   ```rust
   Alignment::CENTER;
   Alignment::TOP_LEFT;
   Alignment::BOTTOM_RIGHT;
   ```

2. **Flex alignment** для списков:
   ```rust
   MainAxisAlignment::SpaceBetween;  // для навбаров
   MainAxisAlignment::Center;         // для кнопок
   CrossAxisAlignment::Stretch;       // для полной высоты
   ```

### Constraints

1. **Tight constraints** для фиксированных размеров:
   ```rust
   BoxConstraints::tight(Size::new(200.0, 100.0))
   ```

2. **Loose constraints** для гибких размеров:
   ```rust
   BoxConstraints::loose(max_size)
   ```

3. **Expand** для заполнения доступного пространства:
   ```rust
   BoxConstraints::expand()
   ```

## Тестирование

```bash
# Все layout тесты
cargo test --lib --package nebula-ui layout

# Конкретный тип
cargo test --lib --package nebula-ui layout::padding

# С выводом
cargo test --lib --package nebula-ui layout -- --nocapture
```

**Текущее покрытие**: 66 тестов

## См. также

- [Core types](../core/README.md) - фундаментальные геометрические типы
- [Styling types](../styling/README.md) - визуальное оформление
- [Typography types](../typography/README.md) - текст и шрифты

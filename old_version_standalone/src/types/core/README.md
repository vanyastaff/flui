# Core Types

Фундаментальные примитивные типы для построения UI систем.

## Обзор

Модуль `core` содержит базовые геометрические и визуальные типы, которые служат основой для всей UI системы nebula-ui. Эти типы спроектированы с акцентом на:

- **Type Safety** - каждый тип имеет четкое семантическое значение
- **Zero-Cost Abstractions** - использование `impl Into<T>` не добавляет runtime overhead
- **Идиоматичный Rust** - полная интеграция с From/Into traits
- **egui Integration** - бесшовная конвертация с egui типами

## Категории типов

### 🎯 2D Базовые типы

#### [`Point`](point.rs)
Абсолютная позиция в 2D пространстве.

```rust
use nebula_ui::types::core::Point;

// Создание
let p1 = Point::new(10.0, 20.0);
let p2 = Point::from((10.0, 20.0));  // из tuple
let p3 = Point::from([10.0, 20.0]);  // из array

// Вычисления
let distance = p1.distance_to(p2);
let midpoint = p1.midpoint(p2);
let p3 = Point::lerp(p1, p2, 0.5);

// Работает с impl Into<Point>
p1.distance_to((15.0, 25.0));  // напрямую из tuple!
```

**Семантика**: Абсолютные координаты (где находится объект)

#### [`Offset`](offset.rs)
Относительное смещение/перемещение.

```rust
use nebula_ui::types::core::{Point, Offset};

let offset = Offset::new(5.0, 10.0);
let point = Point::new(10.0, 20.0);

// Point + Offset = Point (перемещение)
let new_point = point + offset;

// Point - Point = Offset (разница)
let diff = new_point - point;
```

**Семантика**: Относительное перемещение (на сколько сдвинуть)

#### [`Size`](size.rs)
Размеры объектов (ширина × высота).

```rust
use nebula_ui::types::core::Size;

let size = Size::new(100.0, 50.0);
let area = size.width * size.height;
let aspect_ratio = size.aspect_ratio();

// Константы
Size::ZERO;
Size::INFINITY;
```

**Семантика**: Размеры объектов

#### [`Scale`](scale.rs)
Коэффициенты масштабирования.

```rust
use nebula_ui::types::core::Scale;

let scale = Scale::uniform(2.0);  // 2x масштаб
let scale = Scale::new(2.0, 0.5); // неравномерный

// Применение
let scaled_size = scale.apply(size);
```

**Семантика**: Масштабирование (во сколько раз)

#### [`Rotation`](rotation.rs)
Type-safe углы поворота.

```rust
use nebula_ui::types::core::Rotation;

let angle = Rotation::degrees(45.0);
let angle = Rotation::radians(std::f32::consts::PI / 4.0);

// Константы
Rotation::ZERO;
Rotation::RIGHT;      // 90°
Rotation::STRAIGHT;   // 180°
Rotation::LEFT;       // 270°
Rotation::FULL;       // 360°

// Конвертация
let radians = angle.as_radians();
let degrees = angle.as_degrees();
```

**Семантика**: Углы вращения

### 📐 Векторы

#### [`Vector2`](vector.rs)
2D вектор для направления и величины.

```rust
use nebula_ui::types::core::Vector2;

let v1 = Vector2::new(3.0, 4.0);
let length = v1.length();  // 5.0
let normalized = v1.normalize();

// Векторная математика
let dot = v1.dot(v2);
let cross = v1.cross(v2);
let reflected = v1.reflect(normal);

// Константы
Vector2::ZERO;
Vector2::RIGHT;  // (1, 0)
Vector2::UP;     // (0, 1)
Vector2::ONE;    // (1, 1)
```

**Использование**: Физика, скорости, силы, направления

#### [`Vector3`](vector.rs)
3D вектор для трехмерных вычислений.

```rust
use nebula_ui::types::core::Vector3;

let v = Vector3::new(1.0, 2.0, 3.0);
let cross_product = v1.cross(v2);  // 3D cross product

// Проекция на XY
let v2d = v.xy();  // Vector2
```

### 🔷 Геометрические фигуры

#### [`Rect`](rect.rs)
Прямоугольник (min/max углы).

```rust
use nebula_ui::types::core::{Rect, Point, Size};

// Различные способы создания
let rect = Rect::from_min_max((0.0, 0.0), (100.0, 50.0));
let rect = Rect::from_min_size((0.0, 0.0), (100.0, 50.0));
let rect = Rect::from_center_size((50.0, 25.0), (100.0, 50.0));
let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);

// Запросы
let contains = rect.contains((25.0, 25.0));
let intersects = rect.intersects(&other);
let union = rect.union(&other);
let intersection = rect.intersection(&other);

// Трансформации
let expanded = rect.expand(10.0);
let translated = rect.translate((5.0, 5.0));
```

**Использование**: Bounding boxes, области UI, layout bounds

#### [`Circle`](circle.rs)
Круг (центр + радиус).

```rust
use nebula_ui::types::core::{Circle, Point, Rotation};

let circle = Circle::new((50.0, 50.0), 25.0);
let circle = Circle::from_diameter((50.0, 50.0), 50.0);

// Геометрия
let area = circle.area();
let circumference = circle.circumference();
let contains = circle.contains((60.0, 60.0));

// Точки на окружности
let point = circle.point_at_angle(Rotation::degrees(45.0));
```

#### [`Arc`](circle.rs)
Дуга (часть круга).

```rust
use nebula_ui::types::core::{Arc, Circle, Rotation};

let arc = Arc::from_center_radius(
    (50.0, 50.0),
    25.0,
    Rotation::degrees(0.0),    // start
    Rotation::degrees(90.0),   // sweep
);

let length = arc.arc_length();
let area = arc.sector_area();
let midpoint = arc.midpoint();

// Интерполяция
let point = arc.point_at(0.5);  // середина дуги
```

#### [`Bounds`](bounds.rs)
Bounding box с центром и extents (half-size).

```rust
use nebula_ui::types::core::{Bounds, Point, Vector2, Size};

// center-based representation
let bounds = Bounds::from_center_size((50.0, 50.0), (100.0, 60.0));

// Лучше для физики и collision detection
let intersects = bounds.intersects(&other);
let contains = bounds.contains((60.0, 55.0));

// Closest point (для расчета расстояний)
let closest = bounds.closest_point((200.0, 200.0));
let distance = bounds.distance_to_point((200.0, 200.0));

// Операции
let merged = bounds.merge(&other);
let intersection = bounds.intersection(&other);
```

**Разница с Rect**:
- `Rect` использует min/max углы - удобно для UI layout
- `Bounds` использует center/extents - удобно для физики

### 🎨 Векторные пути

#### [`Path`](path.rs)
Векторный путь из сегментов.

```rust
use nebula_ui::types::core::{Path, Point};

// Builder pattern
let path = Path::new()
    .move_to((0.0, 0.0))
    .line_to((100.0, 0.0))
    .cubic_to(
        (120.0, 0.0),   // control1
        (130.0, 20.0),  // control2
        (130.0, 40.0),  // end
    )
    .close();

// Готовые формы
let rect_path = Path::rect(rect);
let circle_path = Path::circle((50.0, 50.0), 25.0);
```

#### [`CubicBezier`](path.rs)
Кубическая кривая Безье (4 точки).

```rust
use nebula_ui::types::core::CubicBezier;

let curve = CubicBezier {
    start: Point::new(0.0, 0.0),
    control1: Point::new(33.0, 100.0),
    control2: Point::new(66.0, 100.0),
    end: Point::new(100.0, 0.0),
};

// Вычисление точек на кривой
let point = curve.at(0.5);
let tangent = curve.tangent_at(0.5);

// Разделение кривой
let (first_half, second_half) = curve.split_at(0.5);
```

#### [`QuadraticBezier`](path.rs)
Квадратичная кривая Безье (3 точки).

```rust
use nebula_ui::types::core::QuadraticBezier;

let curve = QuadraticBezier {
    start: Point::new(0.0, 0.0),
    control: Point::new(50.0, 100.0),
    end: Point::new(100.0, 0.0),
};

// Конвертация в кубическую
let cubic = curve.to_cubic();
```

### 📏 Диапазоны

#### [`Range1D`](range.rs)
Одномерный интервал значений.

```rust
use nebula_ui::types::core::Range1D;

let range = Range1D::new(0.0, 100.0);

// Проверки
let contains = range.contains(50.0);
let overlaps = range.overlaps(&other);

// Интерполяция
let value = range.lerp(0.5);  // 50.0
let t = range.inverse_lerp(75.0);  // 0.75

// Mapping между диапазонами
let range2 = Range1D::new(0.0, 1.0);
let normalized = range.map_to(50.0, &range2);  // 0.5

// Операции
let clamped = range.clamp(150.0);  // 100.0
let expanded = range.expand(10.0);
```

#### [`Range2D`](range.rs)
Двумерный интервал (два Range1D).

```rust
use nebula_ui::types::core::{Range2D, Point};

let range = Range2D::from_values(0.0, 100.0, 0.0, 50.0);
let range = Range2D::from_rect(rect);

// 2D операции
let contains = range.contains((50.0, 25.0));
let point = range.lerp(0.5, 0.5);  // центр
let (tx, ty) = range.inverse_lerp((75.0, 37.5));
```

### 🎭 Layout типы

#### [`Position`](position.rs)
CSS-like позиционирование с опциональными краями.

```rust
use nebula_ui::types::core::Position;

let pos = Position::new()
    .left(10.0)
    .top(20.0)
    .right(10.0)
    .bottom(20.0);

// Вычисление rect из позиции
let container = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
let positioned = pos.resolve(container);
```

**Семантика**: Абсолютное позиционирование (как CSS position: absolute)

#### [`Transform`](transform.rs)
2D трансформация (translation + rotation + scale).

```rust
use nebula_ui::types::core::{Transform, Offset, Rotation, Scale};

let transform = Transform::identity()
    .translate((10.0, 20.0))
    .rotate(Rotation::degrees(45.0))
    .scale(2.0);

// Применение к точке
let transformed = transform.transform_point(point);

// Композиция трансформаций
let combined = transform1.then(transform2);
```

### 🎨 Визуальные типы

#### [`Color`](color.rs)
RGBA цвет с rich API.

```rust
use nebula_ui::types::core::Color;

// Создание
let color = Color::rgb(255, 128, 64);
let color = Color::rgba(255, 128, 64, 200);
let color = Color::from_hex("#FF8040");

// Константы
Color::RED;
Color::GREEN;
Color::BLUE;
Color::WHITE;
Color::BLACK;
Color::TRANSPARENT;

// HSL/HSV
let color = Color::from_hsl(0.5, 0.8, 0.6);
let (h, s, l) = color.to_hsl();

// Модификация
let lighter = color.lighten(0.2);
let darker = color.darken(0.2);
let saturated = color.saturate(0.3);
let with_opacity = color.with_opacity(0.5);

// Интерполяция
let mixed = Color::lerp(color1, color2, 0.5);
```

#### [`Opacity`](opacity.rs)
Значение прозрачности (0.0 - 1.0).

```rust
use nebula_ui::types::core::Opacity;

let opacity = Opacity::new(0.75);
let opacity = Opacity::from_percent(75.0);
let opacity = Opacity::from_u8(192);

// Константы
Opacity::TRANSPARENT;  // 0.0
Opacity::OPAQUE;       // 1.0

// Операции
let composed = opacity1.compose(opacity2);  // multiply
let inverted = opacity.inverse();
```

### ⏱️ Время

#### [`Duration`](duration.rs)
Type-safe длительность времени.

```rust
use nebula_ui::types::core::Duration;

let duration = Duration::seconds(2.5);
let duration = Duration::milliseconds(500);

// Константы для анимаций
Duration::SHORT;       // 200ms
Duration::MEDIUM;      // 400ms
Duration::LONG;        // 600ms
Duration::EXTRA_LONG;  // 1000ms

// Арифметика
let total = duration1 + duration2;
let scaled = duration * 2.0;

// Конвертация
let secs = duration.as_seconds();
let millis = duration.as_milliseconds();
```

## Принципы дизайна

### 1. Type Safety через семантику

Каждый тип имеет четкое семантическое значение:

```rust
// ❌ Неправильно - все f32, легко перепутать
fn move_widget(x: f32, y: f32, width: f32, height: f32) { }

// ✅ Правильно - типы защищают от ошибок
fn move_widget(position: Point, size: Size) { }

// Компилятор не даст перепутать:
move_widget(size, position);  // ❌ Ошибка компиляции!
```

### 2. Идиоматичный Rust с `impl Into<T>`

Все методы используют `impl Into<T>` для удобства:

```rust
// Все эти варианты работают:
point.distance_to(other_point);
point.distance_to((10.0, 20.0));
point.distance_to([10.0, 20.0]);
point.distance_to(egui_pos);
```

### 3. Zero-Cost Abstractions

Благодаря `impl Into<T>` и инлайнингу, нет runtime overhead:

```rust
// Компилируется в один и тот же код:
point.distance_to(Point::new(10.0, 20.0));
point.distance_to((10.0, 20.0));
```

### 4. Полная интеграция с egui

Бесшовная конвертация туда и обратно:

```rust
use egui::{Pos2, Vec2, Rect as EguiRect};

// From egui
let point: Point = pos2.into();
let offset: Offset = vec2.into();

// To egui
let pos2: Pos2 = point.into();
let vec2: Vec2 = offset.into();
```

## Примеры использования

### Пример 1: Вычисление collision

```rust
use nebula_ui::types::core::{Bounds, Vector2, Point};

fn check_collision(player: &Bounds, enemies: &[Bounds]) -> Option<usize> {
    for (i, enemy) in enemies.iter().enumerate() {
        if player.intersects(enemy) {
            return Some(i);
        }
    }
    None
}

fn get_push_back_vector(entity: &Bounds, obstacle: &Bounds) -> Vector2 {
    let closest = obstacle.closest_point(entity.center);
    let push_direction = Vector2::new(
        entity.center.x - closest.x,
        entity.center.y - closest.y,
    );
    push_direction.normalize()
}
```

### Пример 2: Анимация по Bezier кривой

```rust
use nebula_ui::types::core::{CubicBezier, Point, Duration};

struct Animation {
    curve: CubicBezier,
    duration: Duration,
    elapsed: Duration,
}

impl Animation {
    fn current_position(&self) -> Point {
        let t = (self.elapsed.as_seconds() / self.duration.as_seconds())
            .clamp(0.0, 1.0);
        self.curve.at(t)
    }
}
```

### Пример 3: Layout вычисления

```rust
use nebula_ui::types::core::{Rect, Size, Point, Offset};

fn layout_children(
    container: Rect,
    child_sizes: &[Size],
    spacing: f32,
) -> Vec<Rect> {
    let mut result = Vec::new();
    let mut current_pos = container.min;

    for &size in child_sizes {
        let child_rect = Rect::from_min_size(current_pos, size);
        result.push(child_rect);
        current_pos = current_pos + Offset::new(size.width + spacing, 0.0);
    }

    result
}
```

### Пример 4: Работа с цветами

```rust
use nebula_ui::types::core::{Color, Duration};

struct FadeAnimation {
    from_color: Color,
    to_color: Color,
    duration: Duration,
    elapsed: Duration,
}

impl FadeAnimation {
    fn current_color(&self) -> Color {
        let t = (self.elapsed.as_seconds() / self.duration.as_seconds())
            .clamp(0.0, 1.0);
        Color::lerp(self.from_color, self.to_color, t)
    }
}
```

## Тестирование

Все типы имеют comprehensive тесты:

```bash
# Запустить все тесты
cargo test --lib --package nebula-ui

# Запустить тесты конкретного типа
cargo test --lib --package nebula-ui point::tests
cargo test --lib --package nebula-ui vector::tests
```

**Текущее покрытие**: 426 тестов

## Performance заметки

### Когда использовать что:

**Rect vs Bounds:**
- `Rect` - для UI layout (min/max углы естественны для рисования)
- `Bounds` - для физики/collision (center/extents быстрее для intersection tests)

**Point vs Offset vs Vector2:**
- `Point` - абсолютные координаты
- `Offset` - UI смещения/перемещения
- `Vector2` - физика, направления, силы

**CubicBezier vs QuadraticBezier:**
- `CubicBezier` - более гибкий, 4 точки контроля
- `QuadraticBezier` - проще, 3 точки, конвертируется в кубический

## См. также

- [Layout types](../layout/README.md) - flex, alignment, constraints
- [Styling types](../styling/README.md) - borders, shadows, gradients
- [Typography types](../typography/README.md) - fonts, text styles

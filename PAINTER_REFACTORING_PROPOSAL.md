# Painter Refactoring Proposal

## Текущие проблемы

### 1. Смешанные уровни абстракции

```rust
// ❌ Плохо: Одинаковый уровень для простых и сложных примитивов
pub trait Painter {
    fn rect(&mut self, rect: Rect, paint: &Paint);        // Простой
    fn rrect(&mut self, rrect: RRect, paint: &Paint);     // Средний
    fn path(&mut self, path: &Path, paint: &Paint);       // Сложный
    fn polygon(&mut self, points: &[Point], paint: &Paint); // Default impl через line()
}
```

**Проблема:** Непонятно, что является базовым примитивом, а что — удобной обёрткой.

### 2. Default implementations скрывают возможности

```rust
fn ellipse(&mut self, ...) {
    // Default: усреднение радиусов → круг (неправильно!)
    let avg_radius = (radius_x + radius_y) * 0.5;
    self.circle(center, avg_radius, paint);
}

fn polygon(&mut self, points: &[Point], paint: &Paint) {
    // Default: рисует линиями (только stroke, нет fill!)
    for i in 0..points.len() {
        self.line(points[i], points[(i + 1) % points.len()], paint);
    }
}
```

**Проблема:**
- Неполная реализация (polygon не поддерживает fill)
- Бэкенды с нативной поддержкой не знают, что можно переопределить
- Пользователи не знают, работает fill или нет

### 3. Отсутствие низкоуровневых примитивов

```rust
// ❌ Нет:
fn mesh(&mut self, vertices: &[Vertex], indices: &[u32]);
fn quad(&mut self, corners: [Point; 4]);
fn triangle(&mut self, p1: Point, p2: Point, p3: Point);
```

**Проблема:** Нельзя эффективно рендерить произвольную геометрию без обхода в высокоуровневые примитивы.

### 4. Transform API неэффективен

```rust
// ❌ Каждый transform требует отдельного вызова
painter.save();
painter.translate(offset);
painter.rotate(angle);
painter.scale(sx, sy);
// ... draw ...
painter.restore();
```

**Проблема:**
- Много вызовов для одной операции
- Нет пакетных трансформаций
- `transform_matrix` имеет default impl через decomposition (неточно)

### 5. Paint слишком большой

```rust
pub struct Paint {
    pub color: Color,
    pub style: PaintingStyle,
    pub stroke_width: f32,
    pub stroke_cap: StrokeCap,
    pub stroke_join: StrokeJoin,
    pub stroke_miter_limit: f32,
    pub anti_alias: bool,
    pub blend_mode: BlendMode,
    pub letter_spacing: f32,  // ❌ Только для текста!
    pub word_spacing: f32,    // ❌ Только для текста!
}
```

**Проблема:** Смешивает общие стили и специфичные для текста.

---

## Предложение: Трёхуровневая архитектура

### Level 1: Core Primitives (обязательные)

Минимальный набор, который ДОЛЖЕН реализовать каждый бэкенд:

```rust
/// Core primitive layer - minimal required implementation
pub trait PainterCore {
    // ========== Базовые примитивы ==========

    /// Mesh primitive - основа для всего
    /// Все остальные примитивы можно построить через mesh
    fn mesh(&mut self, vertices: &[Vertex], indices: &[u32], paint: &Paint);

    /// Quad primitive (4 вершины)
    /// Эффективнее, чем mesh для простых прямоугольников
    fn quad(&mut self, corners: [Point; 4], paint: &Paint) {
        // Default через mesh
        let vertices = corners.iter()
            .map(|&p| Vertex { pos: p, color: paint.color, uv: Point::ZERO })
            .collect::<Vec<_>>();
        let indices = vec![0, 1, 2, 0, 2, 3];
        self.mesh(&vertices, &indices, paint);
    }

    /// Triangle primitive
    fn triangle(&mut self, p1: Point, p2: Point, p3: Point, paint: &Paint) {
        // Default через mesh
        let vertices = vec![
            Vertex { pos: p1, color: paint.color, uv: Point::ZERO },
            Vertex { pos: p2, color: paint.color, uv: Point::ZERO },
            Vertex { pos: p3, color: paint.color, uv: Point::ZERO },
        ];
        let indices = vec![0, 1, 2];
        self.mesh(&vertices, &indices, paint);
    }

    // ========== Transform Stack ==========

    /// Save/restore state
    fn save(&mut self);
    fn restore(&mut self);

    /// Apply full matrix in one call (эффективнее)
    fn apply_matrix4(&mut self, matrix: Mat4);

    // ========== Clipping ==========

    /// Clip to arbitrary mesh
    fn clip_mesh(&mut self, vertices: &[Vertex], indices: &[u32]);
}

/// Vertex structure for mesh rendering
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub pos: Point,
    pub color: Color,
    pub uv: Point,  // Texture coordinates (for images)
}
```

### Level 2: Shape Primitives (рекомендуется переопределить)

Высокоуровневые фигуры с эффективными default implementations:

```rust
/// Shape primitives - commonly used shapes
/// Backends should override for optimal performance
pub trait PainterShapes: PainterCore {
    /// Rectangle (aligned)
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        let corners = [
            rect.top_left(),
            rect.top_right(),
            rect.bottom_right(),
            rect.bottom_left(),
        ];
        self.quad(corners, paint);
    }

    /// Rounded rectangle
    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        // Default: Tesselate to triangles
        let (vertices, indices) = tesselate_rrect(&rrect, paint.color);
        self.mesh(&vertices, &indices, paint);
    }

    /// Circle
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        // Default: Tesselate to triangles
        let segments = calculate_circle_segments(radius);
        let (vertices, indices) = tesselate_circle(center, radius, segments, paint.color);
        self.mesh(&vertices, &indices, paint);
    }

    /// Ellipse (proper implementation!)
    fn ellipse(&mut self, center: Point, radius_x: f32, radius_y: f32, paint: &Paint) {
        let segments = calculate_ellipse_segments(radius_x, radius_y);
        let (vertices, indices) = tesselate_ellipse(center, radius_x, radius_y, segments, paint.color);
        self.mesh(&vertices, &indices, paint);
    }

    /// Polygon (filled)
    fn polygon(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 3 { return; }

        // Triangulate polygon (ear clipping algorithm)
        let (vertices, indices) = triangulate_polygon(points, paint.color);
        self.mesh(&vertices, &indices, paint);
    }

    /// Polyline (stroked)
    fn polyline(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 2 { return; }

        // Generate stroke geometry
        let (vertices, indices) = tesselate_stroke(points, paint.stroke_width, paint.color);
        self.mesh(&vertices, &indices, paint);
    }

    /// Line
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.polyline(&[p1, p2], paint);
    }

    /// Arc
    fn arc(&mut self, center: Point, radius: f32, start_angle: f32, end_angle: f32, paint: &Paint) {
        let segments = calculate_arc_segments(radius, end_angle - start_angle);
        let (vertices, indices) = tesselate_arc(center, radius, start_angle, end_angle, segments, paint.color);
        self.mesh(&vertices, &indices, paint);
    }
}
```

### Level 3: High-Level API (удобство)

Удобные методы для пользователей:

```rust
/// High-level convenience methods
pub trait Painter: PainterShapes {
    // ========== Text Rendering ==========

    /// Simple text rendering
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint);

    /// Styled text rendering
    fn text_styled(&mut self, text: &str, position: Point, style: &TextStyle);

    // ========== Image Rendering ==========

    /// Draw image
    fn image(&mut self, image: &Image, src_rect: Rect, dst_rect: Rect, paint: &Paint);

    // ========== Path Rendering ==========

    /// Draw arbitrary path
    fn path(&mut self, path: &Path, paint: &Paint) {
        // Convert path to triangles
        let (vertices, indices) = tesselate_path(path, paint);
        self.mesh(&vertices, &indices, paint);
    }

    // ========== Transform Helpers ==========

    /// Translate (convenience)
    fn translate(&mut self, offset: Offset) {
        let matrix = Mat4::from_translation(offset.to_vec3());
        self.apply_matrix4(matrix);
    }

    /// Rotate (convenience)
    fn rotate(&mut self, angle: f32) {
        let matrix = Mat4::from_rotation_z(angle);
        self.apply_matrix4(matrix);
    }

    /// Scale (convenience)
    fn scale(&mut self, sx: f32, sy: f32) {
        let matrix = Mat4::from_scale(Vec3::new(sx, sy, 1.0));
        self.apply_matrix4(matrix);
    }

    /// Skew (convenience)
    fn skew(&mut self, skew_x: f32, skew_y: f32) {
        let matrix = Mat4::from_skew(skew_x, skew_y);
        self.apply_matrix4(matrix);
    }

    // ========== Clipping Helpers ==========

    /// Clip to rect (convenience)
    fn clip_rect(&mut self, rect: Rect) {
        let (vertices, indices) = rect_to_mesh(rect);
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to rounded rect
    fn clip_rrect(&mut self, rrect: RRect) {
        let (vertices, indices) = tesselate_rrect(&rrect, Color::WHITE);
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to oval
    fn clip_oval(&mut self, rect: Rect) {
        let center = rect.center();
        let radius_x = rect.width() / 2.0;
        let radius_y = rect.height() / 2.0;
        let (vertices, indices) = tesselate_ellipse(center, radius_x, radius_y, 32, Color::WHITE);
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to path
    fn clip_path(&mut self, path: &Path) {
        let (vertices, indices) = tesselate_path(path, &Paint::fill(Color::WHITE));
        self.clip_mesh(&vertices, &indices);
    }
}
```

### Paint Refactoring

Разделить на общий и специализированные стили:

```rust
/// Core paint properties (used by all primitives)
#[derive(Debug, Clone)]
pub struct Paint {
    pub color: Color,
    pub style: PaintingStyle,
    pub anti_alias: bool,
    pub blend_mode: BlendMode,
}

/// Stroke-specific properties
#[derive(Debug, Clone)]
pub struct StrokePaint {
    pub base: Paint,
    pub width: f32,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    pub miter_limit: f32,
}

/// Text-specific properties
#[derive(Debug, Clone)]
pub struct TextPaint {
    pub base: Paint,
    pub letter_spacing: f32,
    pub word_spacing: f32,
}

impl Paint {
    /// Create fill paint
    pub fn fill(color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Fill,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create stroke paint
    pub fn stroke(width: f32, color: Color) -> StrokePaint {
        StrokePaint {
            base: Self::fill(color),
            width,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
            miter_limit: 4.0,
        }
    }
}
```

---

## Преимущества новой архитектуры

### 1. Чёткое разделение уровней

```rust
// Level 1: Бэкенд ОБЯЗАН реализовать
impl PainterCore for EguiPainter {
    fn mesh(&mut self, vertices, indices, paint) {
        // Нативная реализация через egui::Mesh
        let mesh = egui::epaint::Mesh { vertices, indices, .. };
        self.add_shape(egui::Shape::Mesh(mesh));
    }

    fn apply_matrix4(&mut self, matrix: Mat4) {
        self.transform_stack.push(matrix);
    }
}

// Level 2: Бэкенд МОЖЕТ переопределить для оптимизации
impl PainterShapes for EguiPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        // egui имеет нативный Rect → используем его
        self.add_shape(egui::Shape::Rect(..));
    }

    // circle, rrect - используем default через tesselation
}

// Level 3: Автоматически доступен
// fn translate, rotate, clip_rect - уже работают!
```

### 2. Легко добавить новый бэкенд

Минимум для нового бэкенда:

```rust
struct MyBackendPainter {
    // ... backend state
}

impl PainterCore for MyBackendPainter {
    fn mesh(&mut self, vertices, indices, paint) {
        // TODO: Render triangles to your backend
    }

    fn save(&mut self) { self.state_stack.push(self.current_state.clone()); }
    fn restore(&mut self) { self.current_state = self.state_stack.pop().unwrap(); }

    fn apply_matrix4(&mut self, matrix: Mat4) {
        self.current_state.transform = matrix;
    }

    fn clip_mesh(&mut self, vertices, indices) {
        // TODO: Set clip region
    }
}

// ВСЁ! rect, circle, path, text - уже работают через default implementations
```

### 3. Оптимизация по необходимости

```rust
// Начали с простого
impl PainterCore for MyBackend {
    fn mesh(...) { /* basic implementation */ }
}
// → Всё работает через mesh tesselation

// Профилировали, нашли bottleneck
impl PainterShapes for MyBackend {
    fn circle(...) { /* optimized native circle */ }
}
// → Только circle оптимизирован, остальное через default

// Нужна максимальная скорость
impl PainterShapes for MyBackend {
    fn rect(...) { /* native */ }
    fn rrect(...) { /* native */ }
    fn circle(...) { /* native */ }
    // ...
}
```

### 4. Тестируемость

```rust
#[cfg(test)]
mod tests {
    struct TestPainter {
        meshes: Vec<(Vec<Vertex>, Vec<u32>)>,
    }

    impl PainterCore for TestPainter {
        fn mesh(&mut self, v, i, _) {
            self.meshes.push((v.to_vec(), i.to_vec()));
        }
        // ... minimal impl
    }

    #[test]
    fn test_rect_generates_correct_mesh() {
        let mut painter = TestPainter::new();
        painter.rect(Rect::from_ltwh(0, 0, 10, 10), &Paint::fill(Color::RED));

        assert_eq!(painter.meshes.len(), 1);
        assert_eq!(painter.meshes[0].0.len(), 4); // 4 vertices
        assert_eq!(painter.meshes[0].1, vec![0, 1, 2, 0, 2, 3]); // 2 triangles
    }
}
```

---

## Migration Path

### Phase 1: Add new traits (non-breaking)

```rust
// crates/flui_engine/src/painter/core.rs
pub trait PainterCore { ... }
pub trait PainterShapes: PainterCore { ... }

// Implement for existing backends
impl PainterCore for EguiPainter { ... }
impl PainterShapes for EguiPainter { ... }

// Old Painter trait still exists
pub trait Painter { ... }
impl Painter for EguiPainter { ... }
```

### Phase 2: Migrate RenderObjects

```rust
// Change gradually
impl LeafRender for RenderRect {
    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Old
        // painter.rect(self.rect, &self.paint);

        // New
        painter.rect(self.rect, &self.paint);
        // ^ Still works! Same API
    }
}
```

### Phase 3: Deprecate old Painter

```rust
#[deprecated(note = "Use PainterCore + PainterShapes instead")]
pub trait Painter: PainterShapes {
    // Keep for backwards compat
}
```

### Phase 4: Remove old Painter

```rust
// Remove deprecated trait
// All code now uses new architecture
```

---

## Tesselation Utilities

Create shared tesselation library:

```rust
// crates/flui_engine/src/painter/tesselation.rs

pub fn tesselate_rect(rect: Rect, color: Color) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_rrect(rrect: &RRect, color: Color) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_circle(center: Point, radius: f32, segments: usize, color: Color) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_ellipse(...) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_arc(...) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn triangulate_polygon(points: &[Point], color: Color) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_stroke(points: &[Point], width: f32, color: Color) -> (Vec<Vertex>, Vec<u32>) { ... }
pub fn tesselate_path(path: &Path, paint: &Paint) -> (Vec<Vertex>, Vec<u32>) { ... }

// Segment calculation (adaptive quality)
pub fn calculate_circle_segments(radius: f32) -> usize {
    // More segments for larger circles
    (radius.sqrt() * 4.0).max(8.0).min(64.0) as usize
}

pub fn calculate_ellipse_segments(radius_x: f32, radius_y: f32) -> usize {
    let max_radius = radius_x.max(radius_y);
    calculate_circle_segments(max_radius)
}
```

---

## Summary

**Что меняется:**
1. ✅ Чёткая иерархия: Core → Shapes → High-Level
2. ✅ Mesh как базовый примитив (все через треугольники)
3. ✅ Default implementations через tesselation
4. ✅ Бэкенды могут оптимизировать любой уровень
5. ✅ Легко добавлять новые бэкенды (минимум ~50 строк)
6. ✅ Paint разделён на специализированные типы

**Что остаётся:**
1. ✅ API для пользователей почти не меняется
2. ✅ Существующий код работает
3. ✅ Производительность egui не ухудшается (можно улучшить)

**Next Steps:**
1. Создать `crates/flui_engine/src/painter/core.rs` с новыми traits
2. Создать `crates/flui_engine/src/painter/tesselation.rs` с утилитами
3. Реализовать PainterCore/PainterShapes для EguiPainter
4. Добавить тесты
5. Мигрировать RenderObjects постепенно
6. Deprecate старый Painter trait

# RenderObject Implementation Guide

Этот документ описывает best practices для реализации RenderObject в FLUI.

## Документация RenderObject

Каждый RenderObject должен иметь:

### 1. Module-level Documentation

```rust
//! RenderObjectName - краткое описание
//!
//! Полное описание функциональности и назначения.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderObjectName` | `RenderObjectName` from package:flutter/... |
//! | `property1` | `property1` property |
//! | `property2` | `property2` property |
//!
//! # Layout Protocol (для Box objects)
//!
//! 1. **Step 1**: Описание первого шага
//! 2. **Step 2**: Описание второго шага
//! 3. **Sizing**: Как определяется размер
//!
//! # Performance
//!
//! - **Layout**: Сложность layout операции
//! - **Paint**: Сложность paint операции
//! - **Memory**: Использование памяти
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderObjectName;
//!
//! let obj = RenderObjectName::new();
//! ```
```

### 2. Struct Documentation

```rust
/// RenderObject для [назначение].
///
/// # Arity
///
/// [Leaf|Single|Optional|Variable] - [описание детей]
///
/// # Protocol
///
/// [Box|Sliver] protocol - Uses [Constraints type] and returns [Geometry type].
///
/// # Use Cases
///
/// - **Case 1**: Описание
/// - **Case 2**: Описание
///
/// # Flutter Behavior
///
/// Описание соответствия Flutter поведению.
#[derive(Debug)]
pub struct RenderObjectName {
    // fields
}
```

## Правильное использование API

### Layout API

**✅ ПРАВИЛЬНО:**
```rust
impl RenderBox<Single> for RenderPadding {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Для Single используем convenience метод
        let child_size = ctx.layout_single_child_with(|c| {
            c.deflate(self.padding)
        })?;

        Ok(child_size.inflate(self.padding))
    }
}
```

**✅ ПРАВИЛЬНО (Variable):**
```rust
impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        // ctx.children() возвращает Iterator<Item = ElementId>
        for child_id in ctx.children() {
            let size = ctx.layout_child(child_id, constraints)?;
            // используем child_id напрямую, без *
        }
        Ok(size)
    }
}
```

**❌ НЕПРАВИЛЬНО:**
```rust
// НЕ используем children.iter() напрямую
for child_id in ctx.children.iter() {  // ❌
    let size = ctx.layout_child(*child_id, constraints)?; // ❌ лишний *
}
```

**✅ ПРАВИЛЬНО (Optional):**
```rust
impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        // children.get() возвращает Option<&ElementId>
        if let Some(child_id) = ctx.children.get() {
            let size = ctx.layout_child(*child_id, constraints)?;
            // здесь * нужен потому что get() возвращает &ElementId
        }
        Ok(size)
    }
}
```

### Paint API

**✅ ПРАВИЛЬНО:**
```rust
impl RenderBox<Single> for RenderOpacity {
    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Paint никогда не возвращает Result
        ctx.paint_single_child(ctx.offset);
        // Нет let _ =, нет ?, просто вызываем
    }
}
```

**✅ ПРАВИЛЬНО (Variable):**
```rust
impl RenderBox<Variable> for RenderStack {
    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let child_ids: Vec<_> = ctx.children().collect();

        for (i, child_id) in child_ids.into_iter().enumerate() {
            let offset = self.child_offsets[i];
            ctx.paint_child(child_id, ctx.offset + offset);
            // child_id уже ElementId, без *
        }
    }
}
```

**❌ НЕПРАВИЛЬНО:**
```rust
fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
    // ❌ Не нужен let _ =
    let _ = ctx.paint_child(child_id, offset);

    // ❌ Не используем ctx.children.iter()
    for child_id in ctx.children.iter() {
        ctx.paint_child(*child_id, offset); // ❌ лишний *
    }
}
```

### Canvas API

**✅ ПРАВИЛЬНО:**
```rust
fn paint(&self, ctx: &mut BoxPaintCtx<'_, Leaf>) {
    let canvas = ctx.canvas_mut();
    let bounds = ctx.local_bounds();

    // Создаем Paint
    let paint = Paint::new()
        .with_color(self.color)
        .with_style(PaintStyle::Fill);

    // Рисуем
    canvas.draw_rect(bounds, &paint);
}
```

**Часто используемые Canvas методы:**
- `draw_rect(rect, paint)` - прямоугольник
- `draw_rrect(rrect, paint)` - скругленный прямоугольник
- `draw_circle(center, radius, paint)` - круг
- `draw_path(path, paint)` - произвольный путь
- `draw_image(image, offset, paint)` - изображение
- `draw_paragraph(paragraph, offset)` - текст

## Arity Types

### Leaf (0 детей)
```rust
impl RenderBox<Leaf> for RenderEmpty {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Leaf>) -> RenderResult<Size> {
        // ctx.children is NoChildren - нет методов доступа к детям
        Ok(Size::ZERO)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Leaf>) {
        // Ничего не рисуем
    }
}
```

### Single (1 ребенок)
```rust
impl RenderBox<Single> for RenderPadding {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Convenience методы
        let child_id = ctx.single_child();
        let size = ctx.layout_single_child()?;
        let size = ctx.layout_single_child_with(|c| c.deflate(...))?;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        ctx.paint_single_child(offset);
    }
}
```

### Optional (0-1 ребенок)
```rust
impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        if let Some(child_id) = ctx.children.get() {
            ctx.layout_child(*child_id, constraints)?; // * нужен
        }
        Ok(size)
    }
}
```

### Variable (N детей)
```rust
impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        for child_id in ctx.children() {
            ctx.layout_child(child_id, constraints)?; // без *
        }
        Ok(size)
    }
}
```

## Протоколы

### Box Protocol
```rust
impl RenderBox<A> for MyObject {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, A>) -> RenderResult<Size> {
        // ctx.constraints: BoxConstraints
        // Возвращаем Size
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, A>) {
        // ctx.geometry: Size
        // ctx.canvas_mut(): &mut Canvas
    }
}
```

### Sliver Protocol
```rust
impl RenderSliver<A> for MySliverObject {
    fn layout(&mut self, ctx: SliverLayoutCtx<'_, A>) -> RenderResult<SliverGeometry> {
        // ctx.constraints: SliverConstraints
        // Возвращаем SliverGeometry
    }
}
```

## Flutter Compliance Checklist

Для каждого RenderObject проверьте:

- [ ] Arity соответствует количеству детей
- [ ] Protocol соответствует типу constraints
- [ ] Layout logic соответствует Flutter (constraints down, sizes up)
- [ ] Используется ctx.children() вместо children.iter()
- [ ] Используется ctx.paint_child() без let _ =
- [ ] Используется ctx.canvas_mut() для рисования
- [ ] Документация включает Flutter Equivalence таблицу
- [ ] Документация описывает Layout Protocol
- [ ] Документация включает Performance заметки
- [ ] Документация включает примеры использования

## Примеры хорошо написанных RenderObjects

Используйте эти как reference:

1. **RenderPadding** - простой Single child
2. **RenderFlex** - сложный Variable children с alignment
3. **RenderStack** - Variable с позиционированием
4. **RenderOpacity** - effects с Canvas
5. **RenderTransform** - сложная трансформация координат

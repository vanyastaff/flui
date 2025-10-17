# Size Hint - Quick Start Guide

## Что такое size_hint()?

`size_hint()` - метод который возвращает **ожидаемый размер виджета ДО его рендеринга**.

```rust
fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2>
```

## Зачем это нужно?

### 1. **Оптимизация Column/Row layouts**
```rust
// Column может предвычислить общую высоту!
let total_height: Option<f32> = children
    .iter()
    .try_fold(0.0, |acc, child| {
        child.size_hint(ui).map(|hint| acc + hint.y)
    });

if let Some(height) = total_height {
    ui.allocate_space(egui::vec2(width, height));  // ✅ Резервируем место заранее
}
```

### 2. **ScrollArea знает размер контента**
```rust
if let Some(content_size) = child.size_hint(ui) {
    // ✅ Знаем нужен ли scrollbar до рендеринга!
    let needs_scrollbar = content_size.y > ui.available_height();
}
```

### 3. **Grid layout распределяет ячейки**
```rust
// Собираем hints для всех ячеек
for (row, col, cell) in &cells {
    if let Some(size) = cell.size_hint(ui) {
        row_heights[row] = row_heights[row].max(size.y);
        col_widths[col] = col_widths[col].max(size.x);
    }
}
// Теперь рендерим с правильными размерами!
```

### 4. **Виртуализация списков**
```rust
// Рендерим ТОЛЬКО видимые элементы
let item_height = 50.0;  // Из size_hint!
let first_visible = (viewport.min.y / item_height).floor() as usize;
let last_visible = (viewport.max.y / item_height).ceil() as usize;

for i in first_visible..last_visible {
    render_item(i);  // Только 10-20 элементов вместо 10,000!
}
```

---

## Как реализовать для своих виджетов

### ✅ Простой случай: Фиксированный размер

```rust
struct MyWidget {
    width: f32,
    height: f32,
}

impl Widget for MyWidget {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        Some(egui::vec2(self.width, self.height))  // ✅ Всегда знаем размер!
    }
}
```

### ✅ Container с учётом padding/margin

```rust
impl Widget for Container {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Если есть фиксированный размер
        if let (Some(w), Some(h)) = (self.width, self.height) {
            return Some(egui::vec2(
                w + self.padding.horizontal_total() + self.margin.horizontal_total(),
                h + self.padding.vertical_total() + self.margin.vertical_total(),
            ));
        }

        // Если есть минимальные ограничения
        if let (Some(min_w), Some(min_h)) = (self.min_width, self.min_height) {
            return Some(egui::vec2(
                min_w + self.padding.horizontal_total() + self.margin.horizontal_total(),
                min_h + self.padding.vertical_total() + self.margin.vertical_total(),
            ));
        }

        None  // Размер зависит от child
    }
}
```

### ✅ Text виджет - измеряет текст

```rust
impl Widget for Text {
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        // egui может измерить текст БЕЗ рендеринга!
        let font_id = egui::FontId::proportional(self.font_size);
        let galley = ui.fonts(|fonts| {
            fonts.layout_no_wrap(
                self.text.clone(),
                font_id,
                egui::Color32::WHITE,
            )
        });

        Some(galley.size())  // ✅ Точный размер текста
    }
}
```

### ✅ Column - сумма детей

```rust
impl Widget for Column {
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        let mut total_height = 0.0;
        let mut max_width = 0.0;

        for child in &self.children {
            let hint = child.size_hint(ui)?;  // Если хоть один None - весь None
            total_height += hint.y;
            max_width = max_width.max(hint.x);
        }

        // Добавляем spacing
        total_height += self.spacing * (self.children.len().saturating_sub(1)) as f32;

        Some(egui::vec2(max_width, total_height))
    }
}
```

### ❌ Когда возвращать None

```rust
impl Widget for Container {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Размер зависит от child или available space
        if self.width.is_none() && self.height.is_none() {
            return None;  // ❌ Не знаем размер
        }

        // ...
    }
}
```

---

## Практические примеры

### Example 1: Адаптивный layout

```rust
fn adaptive_layout(widget: impl Widget, ui: &mut egui::Ui) {
    if let Some(size) = widget.size_hint(ui) {
        if size.x > ui.available_width() {
            // ✅ Слишком широкий - добавляем scroll
            egui::ScrollArea::horizontal().show(ui, |ui| {
                widget.ui(ui);
            });
        } else {
            // ✅ Помещается
            widget.ui(ui);
        }
    } else {
        widget.ui(ui);  // Не знаем размер - рендерим обычно
    }
}
```

### Example 2: Резервирование места для анимации

```rust
impl Widget for AnimatedWidget {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // ✅ ВСЕГДА возвращаем ФИНАЛЬНЫЙ размер
        // Чтобы layout не прыгал во время анимации
        Some(self.final_size)
    }

    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let current_size = self.final_size * self.progress;  // Анимируется

        // Выделяем место под финальный размер
        let (rect, response) = ui.allocate_exact_size(
            self.final_size,  // ← size_hint возвращает это же
            egui::Sense::hover()
        );

        // Рендерим с текущим размером (меньше чем выделено)
        self.paint_at_size(ui, current_size);

        response
    }
}
```

### Example 3: Grid auto-sizing

```rust
struct Grid {
    rows: Vec<Vec<Box<dyn Widget>>>,
}

impl Widget for Grid {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Собираем hints для ВСЕХ ячеек
        let mut row_heights = vec![0.0; self.rows.len()];
        let mut col_widths = vec![0.0; self.rows[0].len()];

        for (row_idx, row) in self.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                if let Some(size) = cell.size_hint(ui) {
                    row_heights[row_idx] = row_heights[row_idx].max(size.y);
                    col_widths[col_idx] = col_widths[col_idx].max(size.x);
                }
            }
        }

        // Теперь рендерим grid с ИДЕАЛЬНЫМИ размерами
        // Каждая ячейка получает ровно столько места, сколько нужно!
    }
}
```

---

## Правила использования

### ✅ DO:

1. **Возвращайте `None` если размер неизвестен**
   ```rust
   if self.width.is_none() { return None; }
   ```

2. **Возвращайте минимальный размер**
   ```rust
   Some(egui::vec2(self.min_width?, self.min_height?))
   ```

3. **Учитывайте padding, margin, borders**
   ```rust
   Some(egui::vec2(
       content.x + padding.horizontal_total(),
       content.y + padding.vertical_total()
   ))
   ```

4. **Используйте `ui.fonts()` для текста**
   ```rust
   let galley = ui.fonts(|f| f.layout_no_wrap(text, font, color));
   Some(galley.size())
   ```

### ❌ DON'T:

1. **Не возвращайте размер БОЛЬШЕ реального**
   ```rust
   // ❌ BAD: Вернули 500, а рендер занял 300
   Some(egui::vec2(500.0, 500.0))  // Layout будет сломан!
   ```

2. **Не делайте тяжёлых вычислений**
   ```rust
   // ❌ BAD: size_hint вызывается ЧАСТО!
   fn size_hint(&self, ui: &egui::Ui) -> Option<Vec2> {
       expensive_computation()  // ← Будет лагать!
   }
   ```

3. **Не рендерите в size_hint**
   ```rust
   // ❌ BAD: Только ИЗМЕРЯЕМ, не рендерим!
   fn size_hint(&self, ui: &mut egui::Ui) -> Option<Vec2> {
       self.ui(ui);  // ← НЕТ! Это сломает всё!
   }
   ```

4. **Не возвращайте `Some(0, 0)` вместо `None`**
   ```rust
   // ❌ BAD:
   Some(egui::vec2(0.0, 0.0))  // Выглядит как виджет без размера

   // ✅ GOOD:
   None  // Явно говорит "не знаю размер"
   ```

---

## Когда возвращать Some vs None

### ✅ Возвращайте `Some`:

- Фиксированные `width` и `height` заданы
- Есть минимальные ограничения (`min_width`, `min_height`)
- Можно измерить контент (текст, картинка)
- Все дети имеют hint (Column/Row может суммировать)
- Виджет ВСЕГДА одного размера (Spacer, Image)

### ❌ Возвращайте `None`:

- Размер зависит от `available_space`
- Размер зависит от неизвестного child
- Размер зависит от динамического контента
- Хоть один child без hint (Column/Row не может вычислить)
- Виджет адаптируется под родителя

---

## Тесты

```rust
#[test]
fn test_size_hint_fixed() {
    let widget = Container {
        width: Some(300.0),
        height: Some(200.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    assert_eq!(widget.size_hint(ui), Some(egui::vec2(300.0, 200.0)));
}

#[test]
fn test_size_hint_with_padding() {
    let widget = Container {
        width: Some(300.0),
        height: Some(200.0),
        padding: EdgeInsets::all(20.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    // 300 + 40 (padding) = 340, 200 + 40 = 240
    assert_eq!(widget.size_hint(ui), Some(egui::vec2(340.0, 240.0)));
}

#[test]
fn test_size_hint_none() {
    let widget = Container {
        padding: EdgeInsets::all(20.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    assert_eq!(widget.size_hint(ui), None);
}
```

---

## Полный пример

Смотри `examples/size_hint_demo.rs` для полного рабочего примера!

```bash
cargo run --example size_hint_demo
```

---

## Итого

`size_hint()` - **опциональная** оптимизация:
- ✅ Возвращайте `Some` если знаете размер
- ✅ Возвращайте `None` если не знаете
- ✅ Должен быть **быстрым** (вызывается часто)
- ✅ Должен быть **точным** (недооценка лучше переоценки)
- ✅ Полезен для **layout optimization** (Column, Grid, Scroll)

**Золотое правило**: Лучше вернуть `None`, чем неточный `Some`! ✨

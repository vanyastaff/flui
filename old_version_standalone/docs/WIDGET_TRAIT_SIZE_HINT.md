# Widget Trait with size_hint() - Implementation Guide

## Концепция size_hint()

`size_hint()` возвращает **предварительный желаемый размер** виджета ДО его рендеринга.

### Зачем это нужно:

1. **Layout optimization** - родительские виджеты могут заранее выделить место
2. **Scroll optimization** - scroll области знают размер контента
3. **Grid/Table layouts** - можно предвычислить размеры ячеек
4. **Animation planning** - можно заранее резервировать место для анимаций

### Важно:

- `size_hint()` это **подсказка**, не гарантия
- Может вернуть `None` если размер неизвестен
- Финальный размер определяется в `ui()` методе
- Принимает `&egui::Ui` для доступа к контексту (шрифты, spacing, etc.)

---

## Примеры реализации

### 1. Container с фиксированным размером

Самый простой случай - виджет знает точный размер:

```rust
impl Container {
    pub fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Если заданы фиксированные width/height - возвращаем их
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(egui::vec2(w, h)),
            _ => None,  // Размер зависит от child или available space
        }
    }
}
```

**Когда работает:**
```rust
// ✅ Вернёт Some(300.0, 200.0)
Container {
    width: Some(300.0),
    height: Some(200.0),
    ..Default::default()
}

// ❌ Вернёт None (неизвестен child размер)
Container {
    padding: EdgeInsets::all(20.0),
    ..Default::default()
}
```

---

### 2. Container с минимальными ограничениями

Если известны только min/max, можем вернуть минимум:

```rust
impl Container {
    pub fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Если есть фиксированный размер - вернуть его
        if let (Some(w), Some(h)) = (self.width, self.height) {
            return Some(egui::vec2(w, h));
        }

        // Если есть минимальные ограничения - вернуть их
        let min_w = self.min_width.or_else(|| {
            self.constraints.as_ref().map(|c| c.min_width)
        });

        let min_h = self.min_height.or_else(|| {
            self.constraints.as_ref().map(|c| c.min_height)
        });

        match (min_w, min_h) {
            (Some(w), Some(h)) => Some(egui::vec2(w, h)),
            _ => None,
        }
    }
}
```

**Когда работает:**
```rust
// ✅ Вернёт Some(100.0, 50.0) - минимальный размер
Container {
    min_width: Some(100.0),
    min_height: Some(50.0),
    ..Default::default()
}

// ✅ Вернёт Some(200.0, 100.0) - из BoxConstraints
Container {
    constraints: Some(BoxConstraints::new(200.0, 400.0, 100.0, 200.0)),
    ..Default::default()
}
```

---

### 3. Container с учётом padding и margin

Более точная версия учитывает padding/margin:

```rust
impl Container {
    pub fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Базовый размер контента
        let mut size = match (self.width, self.height) {
            (Some(w), Some(h)) => egui::vec2(w, h),
            (Some(w), None) => return None,  // Нужен child для height
            (None, Some(h)) => return None,  // Нужен child для width
            (None, None) => {
                // Попробуем взять минимальные размеры
                let min_w = self.min_width.or_else(|| {
                    self.constraints.as_ref().map(|c| c.min_width)
                })?;

                let min_h = self.min_height.or_else(|| {
                    self.constraints.as_ref().map(|c| c.min_height)
                })?;

                egui::vec2(min_w, min_h)
            }
        };

        // Добавляем padding (внутри контейнера)
        size.x += self.padding.left + self.padding.right;
        size.y += self.padding.top + self.padding.bottom;

        // Добавляем margin (вокруг контейнера)
        size.x += self.margin.left + self.margin.right;
        size.y += self.margin.top + self.margin.bottom;

        Some(size)
    }
}
```

**Когда работает:**
```rust
// ✅ Вернёт Some(340.0, 240.0)
//    300 + 20*2 (padding) + 0 (no margin) = 340
//    200 + 20*2 (padding) + 0 (no margin) = 240
Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: EdgeInsets::all(20.0),
    ..Default::default()
}

// ✅ Вернёт Some(370.0, 270.0)
//    300 + 20*2 (padding) + 15*2 (margin) = 370
Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: EdgeInsets::all(20.0),
    margin: EdgeInsets::all(15.0),
    ..Default::default()
}
```

---

### 4. Полная реализация (рекомендуемая для Container)

Использует существующий метод `calculate_size()`:

```rust
impl Container {
    /// Get size hint for layout optimization.
    ///
    /// Returns the expected size of this container if it can be determined
    /// without rendering. This helps parent widgets optimize layout.
    ///
    /// Returns `Some(size)` if the container has fixed dimensions or minimum
    /// constraints. Returns `None` if size depends on child or available space.
    pub fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        // Можем дать hint только если знаем хотя бы один из размеров
        let has_width = self.width.is_some() || self.min_width.is_some();
        let has_height = self.height.is_some() || self.min_height.is_some();

        if !has_width && !has_height {
            // Размер полностью зависит от child или available space
            return None;
        }

        // Используем очень большой available size для получения максимума
        // В реальности calculate_size() применит min/max constraints
        let large_size = egui::vec2(f32::INFINITY, f32::INFINITY);
        let content_size = self.calculate_size(large_size);

        // Если получили бесконечность - значит нет ограничений
        if content_size.x.is_infinite() || content_size.y.is_infinite() {
            return None;
        }

        // Добавляем padding и margin
        let total_size = egui::vec2(
            content_size.x + self.padding.horizontal() + self.margin.horizontal(),
            content_size.y + self.padding.vertical() + self.margin.vertical(),
        );

        Some(total_size)
    }
}
```

**Нужно добавить helper методы в EdgeInsets:**
```rust
impl EdgeInsets {
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}
```

---

## Примеры для других виджетов

### Text виджет (знает размер текста)

```rust
struct Text {
    text: String,
    font_size: f32,
}

impl Text {
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        // egui может измерить текст БЕЗ рендеринга!
        let font_id = egui::FontId::proportional(self.font_size);
        let galley = ui.fonts(|fonts| {
            fonts.layout_no_wrap(
                self.text.clone(),
                font_id,
                egui::Color32::WHITE,  // Цвет не важен для размера
            )
        });

        Some(galley.size())
    }
}
```

### Button (знает примерный размер)

```rust
struct Button {
    text: String,
}

impl Button {
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        // Кнопка = текст + padding + frame
        let text_size = ui.fonts(|fonts| {
            fonts.layout_no_wrap(
                self.text.clone(),
                egui::TextStyle::Button.resolve(ui.style()),
                egui::Color32::WHITE,
            ).size()
        });

        let button_padding = ui.spacing().button_padding;
        let frame_margin = ui.spacing().item_spacing;

        Some(egui::vec2(
            text_size.x + button_padding.x * 2.0 + frame_margin.x,
            text_size.y + button_padding.y * 2.0 + frame_margin.y,
        ))
    }
}
```

### Image виджет (всегда знает размер!)

```rust
struct Image {
    texture: egui::TextureId,
    size: egui::Vec2,
}

impl Image {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Картинка ВСЕГДА знает свой размер!
        Some(self.size)
    }
}
```

### Spacer виджет (фиксированный размер)

```rust
struct Spacer {
    width: f32,
    height: f32,
}

impl Spacer {
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        Some(egui::vec2(self.width, self.height))
    }
}
```

### Column/Row виджет (сумма детей)

```rust
struct Column {
    children: Vec<Box<dyn Widget>>,
    spacing: f32,
}

impl Column {
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        let mut total_height = 0.0;
        let mut max_width = 0.0;

        for child in &self.children {
            let hint = child.size_hint(ui)?;  // Если хоть один None - возвращаем None
            total_height += hint.y;
            max_width = max_width.max(hint.x);
        }

        // Добавляем spacing между элементами
        total_height += self.spacing * (self.children.len() - 1) as f32;

        Some(egui::vec2(max_width, total_height))
    }
}
```

---

## Использование в Widget трейте

```rust
pub trait Widget: Sized {
    /// Render the widget
    fn ui(self, ui: &mut egui::Ui) -> egui::Response;

    /// Get optional size hint for layout optimization
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        None  // По умолчанию размер неизвестен
    }
}
```

---

## Использование родительскими виджетами

### ScrollArea с предвычислением размера

```rust
impl ScrollArea {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Если child знает свой размер - можем предвычислить scroll
        if let Some(content_size) = self.child.size_hint(ui) {
            // Резервируем место для scroll bar если нужно
            let needs_scrollbar = content_size.y > ui.available_height();

            if needs_scrollbar {
                // Выделяем место под scrollbar
                let scrollbar_width = ui.spacing().scroll_bar_width;
                // ... настраиваем layout
            }
        }

        // Рендерим child
        self.child.ui(ui)
    }
}
```

### Grid layout с предвычислением ячеек

```rust
impl Grid {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Собираем size hints для всех ячеек
        let mut row_heights = vec![0.0; self.rows];
        let mut col_widths = vec![0.0; self.cols];

        for (row, col, cell) in &self.cells {
            if let Some(size) = cell.size_hint(ui) {
                row_heights[row] = row_heights[row].max(size.y);
                col_widths[col] = col_widths[col].max(size.x);
            }
        }

        // Теперь можем отрендерить grid с правильными размерами
        // ...
    }
}
```

---

## Правила использования size_hint()

### ✅ DO:
1. Возвращайте `None` если размер неизвестен
2. Возвращайте минимальный размер если есть constraints
3. Учитывайте padding, margin, borders
4. Используйте `ui.fonts()` для измерения текста
5. Кэшируйте результат если вычисления дорогие

### ❌ DON'T:
1. Не возвращайте размер больше чем будет в реальности
2. Не делайте тяжёлых вычислений (вызывается часто!)
3. Не рендерите виджет в size_hint (только измеряйте!)
4. Не полагайтесь на size_hint как на гарантию
5. Не возвращайте `Some(0, 0)` вместо `None`

---

## Тесты для size_hint()

```rust
#[test]
fn test_container_size_hint_fixed() {
    let container = Container {
        width: Some(300.0),
        height: Some(200.0),
        ..Default::default()
    };

    let ui = &mut test_ui();  // Mock UI
    assert_eq!(container.size_hint(ui), Some(egui::vec2(300.0, 200.0)));
}

#[test]
fn test_container_size_hint_with_padding() {
    let container = Container {
        width: Some(300.0),
        height: Some(200.0),
        padding: EdgeInsets::all(20.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    // 300 + 40 (padding) = 340, 200 + 40 = 240
    assert_eq!(container.size_hint(ui), Some(egui::vec2(340.0, 240.0)));
}

#[test]
fn test_container_size_hint_none() {
    let container = Container {
        padding: EdgeInsets::all(20.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    // Размер зависит от child - возвращаем None
    assert_eq!(container.size_hint(ui), None);
}

#[test]
fn test_container_size_hint_min_constraints() {
    let container = Container {
        min_width: Some(100.0),
        min_height: Some(50.0),
        ..Default::default()
    };

    let ui = &mut test_ui();
    assert_eq!(container.size_hint(ui), Some(egui::vec2(100.0, 50.0)));
}
```

---

## Оптимизация производительности

Если `size_hint()` вызывается часто, можно добавить кэширование:

```rust
pub struct CachedWidget<W: Widget> {
    widget: W,
    cached_hint: Option<egui::Vec2>,
}

impl<W: Widget> Widget for CachedWidget<W> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        self.widget.ui(ui)
    }

    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        if let Some(cached) = self.cached_hint {
            return Some(cached);
        }

        self.widget.size_hint(ui)
    }
}
```

---

## Выводы

`size_hint()` - мощный инструмент для оптимизации layout, но:

1. **Не обязательно** - можно вернуть `None`
2. **Должен быть быстрым** - вызывается часто
3. **Должен быть точным** - недооценка лучше переоценки
4. **Учитывает контекст** - через `&egui::Ui` доступ к шрифтам, spacing
5. **Полезен для scroll, grid, table** - они могут предвычислить layout

Для Container оптимальная реализация - использовать `calculate_size()` с проверкой на infinity.

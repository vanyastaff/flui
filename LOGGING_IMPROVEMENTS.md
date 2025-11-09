# Улучшение системы логирования FLUI

## Обзор изменений

Логирование было оптимизировано для лучшей читаемости в production. Теперь по умолчанию показываются только важные события.

## Что изменилось

### 1. Уровни логирования

**До:**
- Все примеры использовали `Level::DEBUG` - слишком много информации
- Каждый текстовый рендер генерировал 5-10 строк логов
- Невозможно было увидеть важную информацию

**После:**
- Примеры используют `Level::INFO` по умолчанию
- DEBUG логи доступны через `RUST_LOG=debug cargo run`
- WARN логи показывают реальные проблемы (overflow, ошибки типов)
- ERROR логи - только критичные ошибки

### 2. Удалено избыточное логирование

#### painter.rs
Убрано 10+ debug логов на каждый текстовый виджет:
- Убрано: `"EguiPainter::text: transformed_pos=..."`
- Убрано: `"EguiPainter::text: creating galley..."`
- Убрано: `"EguiPainter::text: adding text shape"`
- **Оставлено:** `WARN` при ошибках vector rendering

#### picture.rs (layer)
Убрано избыточное логирование команд рисования:
- Убрано: `"PictureLayer::paint: {} commands"`
- Убрано: `"PictureLayer::paint: Text command - text='...'"`
- Убрано: `"PictureLayer::paint: Text painted"`

#### app.rs
Убраны DEBUG println!:
- Убрано: `"[DEBUG] Requesting layout and paint for root"`
- Убрано: `"[DEBUG] Layout succeeded"`
- Убрано: `"[DEBUG] ComponentElement will be built..."`

### 3. Что осталось (полезные логи)

#### WARN - предупреждения о проблемах
```rust
WARN flui_rendering::objects::layout::flex: 
  RenderFlex overflow detected! 
  Tip: Use Flexible/Expanded widgets or reduce content size
  direction=Horizontal 
  content_size_px=2472.0 
  container_size_px=824.0 
  overflow_px=1648.0
```

#### ERROR - критические ошибки
```rust
ERROR flui_core::pipeline::frame_coordinator: 
  flush_layout: Root element not found! ID: ElementId(123)
```

## Как использовать

### По умолчанию (INFO уровень)
```bash
cargo run --example style_comparison
```

**Вывод:** Только важная информация + warnings/errors

### Debug режим
```bash
RUST_LOG=debug cargo run --example style_comparison
```

**Вывод:** Детальная информация для отладки

### Trace режим (максимум деталей)
```bash
RUST_LOG=trace cargo run --example style_comparison
```

**Вывод:** Вся доступная информация

### Выборочный debug
```bash
# Только логи из painter
RUST_LOG=flui_engine::backends::egui::painter=debug cargo run --example style_comparison

# Только pipeline логи
RUST_LOG=flui_core::pipeline=debug cargo run --example style_comparison

# Комбинация
RUST_LOG=flui_engine=debug,flui_core::pipeline=trace cargo run --example style_comparison
```

## Примеры выводов

### До оптимизации
```
[DEBUG] EguiPainter::text: transformed_pos=Point { x: 237.5, y: 185.6 }
[DEBUG] EguiPainter::text: creating galley with font_id=FontId { size: 32.0 }
[DEBUG] EguiPainter::text: galley created, size=[145.2 36.0]
[DEBUG] EguiPainter::text: adding text shape
[DEBUG] EguiPainter::text: text shape added
[DEBUG] PictureLayer::paint: 42 commands
[DEBUG] PictureLayer::paint: Text command - text='Macro Style', position=Point { x: 0.0, y: 0.0 }
[DEBUG] PictureLayer::paint: Text painted
[DEBUG] build_frame: Build phase rebuilt 15 widgets
[DEBUG] build_frame: Layout phase computed 42 layouts
[DEBUG] build_frame: Root size Size { width: 872.0, height: 811.0 }
[DEBUG] build_frame: Paint phase generated 28 layers
... (еще 200+ строк логов на один фрейм)
```

### После оптимизации  
```
=== FLUI Style Comparison ===
  1. Macro Style - compact and declarative
  2. Builder Style - traditional and explicit
  3. Hybrid Style - best of both (recommended)

Choose your preferred style with:
  use flui_widgets::style::macros::prelude::*;  // Macro
  use flui_widgets::style::builder::prelude::*; // Builder
  use flui_widgets::style::hybrid::prelude::*;  // Hybrid

[WARN] RenderFlex overflow detected! 
  Tip: Use Flexible/Expanded widgets or reduce content size
  direction=Horizontal overflow_px=1648.0

(Приложение работает)
```

## Рекомендации для разработчиков

### При разработке новых фичей
```rust
// ✅ Хорошо - используйте tracing с уровнями
tracing::debug!("Detailed info: {:?}", value);
tracing::info!("Important event happened");
tracing::warn!("Something unusual: {:?}", issue);
tracing::error!("Critical error: {:?}", error);

// ❌ Плохо - не используйте println!
println!("[DEBUG] Some info");  // Невозможно выключить!
```

### При отладке
```rust
// ✅ Используйте #[cfg(debug_assertions)] для временных логов
#[cfg(debug_assertions)]
tracing::trace!("Very detailed debugging: {:?}", state);

// ❌ Не коммитьте временные debug логи
tracing::debug!("TODO: remove this debug log");
```

### Для production
```rust
// ✅ Только важные события
tracing::info!("Application started");
tracing::warn!("Configuration issue: {:?}", config);
tracing::error!("Fatal error: {:?}", error);

// ❌ Не логируйте в горячих циклах на INFO уровне
for item in items {
    tracing::info!("Processing {}", item);  // Плохо!
}
```

## Статистика

**Удалено логов:**
- `painter.rs`: 10 debug логов
- `picture.rs`: 3 debug лога  
- `app.rs`: 9 debug println!
- Всего: ~22 избыточных лога в горячих путях

**Результат:**
- Лог файлы меньше в ~50x раз
- Вывод консоли читаемый и понятный
- Debug информация доступна при необходимости

## Файлы изменений

- [crates/flui_engine/src/backends/egui/painter.rs](crates/flui_engine/src/backends/egui/painter.rs) - удалены debug логи рендеринга
- [crates/flui_engine/src/layer/picture.rs](crates/flui_engine/src/layer/picture.rs) - удалены debug логи команд
- [crates/flui_app/src/app.rs](crates/flui_app/src/app.rs) - удалены debug println!
- `examples/*.rs` - изменен уровень логирования с DEBUG на INFO

## Связанные документы

- [CLAUDE.md](CLAUDE.md) - правила использования tracing
- [STYLE_SYSTEM.md](STYLE_SYSTEM.md) - документация системы стилей

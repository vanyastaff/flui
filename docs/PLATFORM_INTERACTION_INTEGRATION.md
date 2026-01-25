# Platform ↔ Interaction Integration Strategy

**Date:** 2026-01-24  
**Status:** 🔍 Analysis Complete - Ready for Integration

---

## Current Situation

### Two Event Systems Discovered

**1. `flui-platform/src/traits/input.rs` (GPUI-style)**
```rust
pub struct PointerEvent {
    pub pointer_id: u64,
    pub position: Point<Pixels>,      // ✅ Конкретный тип
    pub delta: Point<Pixels>,         // ⚠️ Должен быть PixelDelta
    pub phase: PointerPhase,
    pub kind: PointerKind,
    // ...
}

pub enum PlatformInput {
    Pointer(PointerEvent),
    ScrollWheel(ScrollWheelEvent),
    KeyDown(KeyDownEvent),
    KeyUp(KeyUpEvent),
    // ...
}
```

**2. `flui_interaction/src/events.rs` (W3C-style via ui-events)**
```rust
// Re-exports from ui-events crate
pub use ui_events::pointer::{
    PointerEvent,          // W3C compliant
    PointerButtonEvent,
    PointerScrollEvent,
    // ...
};

pub struct PointerEventData {  // Compatibility wrapper
    pub position: Offset<Pixels>,       // ✅ Конкретный тип
    pub local_position: Offset<Pixels>,
    pub device_kind: PointerType,
    // ...
}
```

### Key Differences

| Aspect | flui-platform | flui_interaction |
|--------|---------------|------------------|
| **Style** | GPUI-inspired | W3C ui-events |
| **Types** | Custom enums | Standard crate |
| **Naming** | `PointerEvent` | `PointerEvent` (conflict!) |
| **Position** | `Point<Pixels>` | `Offset<Pixels>` |
| **Delta** | `Point<Pixels>` ⚠️ | `Offset<PixelDelta>` ✅ |
| **Status** | Active (Phase 1) | Disabled (waiting) |

### ⚠️ Type Issues Found

**In `flui-platform`:**
```rust
pub struct PointerEvent {
    pub delta: Point<Pixels>,  // ❌ Должно быть Point<PixelDelta>
}

pub struct Velocity {
    pub x: f32,  // ❌ Должно быть из flui_types::gestures::Velocity
    pub y: f32,
}
```

---

## Integration Options

### Option A: Platform Events → W3C Events (Рекомендуется)

**Архитектура:**
```
OS Events (winit)
    ↓
flui-platform (конвертирует)
    ↓
ui-events types (W3C)
    ↓
flui_interaction (обрабатывает)
    ↓
User code
```

**Преимущества:**
- ✅ W3C стандартность
- ✅ Богатый набор событий из ui-events
- ✅ flui_interaction уже готов
- ✅ Будущая web совместимость

**Недостатки:**
- ❌ Дополнительная конвертация
- ❌ Зависимость от external crate

**Реализация:**
```rust
// flui-platform/src/platforms/winit/mod.rs
use ui_events::pointer::PointerEvent as W3CPointerEvent;

impl WinitPlatform {
    fn convert_winit_event(&self, event: winit::Event) -> W3CPointerEvent {
        // Convert winit → W3C
    }
}
```

### Option B: Unified Platform Events (GPUI-style)

**Архитектура:**
```
OS Events (winit)
    ↓
flui-platform events (custom)
    ↓
flui_interaction (адаптируется)
    ↓
User code
```

**Преимущества:**
- ✅ Полный контроль над типами
- ✅ Нет external dependencies
- ✅ Прямая конвертация

**Недостатки:**
- ❌ Нужно переписать flui_interaction
- ❌ Потеряем W3C стандартность
- ❌ Больше кода для поддержки

### Option C: Hybrid (Two-Layer)

**Архитектура:**
```
OS Events (winit)
    ↓
flui-platform events (low-level)
    ↓
Conversion Layer
    ↓
ui-events (high-level)
    ↓
flui_interaction
```

**Преимущества:**
- ✅ Гибкость
- ✅ Можно использовать обе системы

**Недостатки:**
- ❌ Сложность
- ❌ Дублирование кода
- ❌ Два API для событий

---

## Рекомендация: Option A

### Почему Option A?

1. **flui_interaction УЖЕ использует ui-events** ✅
2. **W3C стандартность** - будущая web поддержка ✅
3. **Богатый API** - ui-events покрывает все случаи ✅
4. **Меньше работы** - не нужно переписывать interaction ✅

### План Миграции

#### Phase 1: Исправить flui-platform Types

```rust
// flui-platform/src/traits/input.rs

// ❌ УДАЛИТЬ custom events
pub struct PointerEvent { ... }  // Конфликтует с ui-events
pub struct Velocity { ... }      // Дубликат flui_types

// ✅ ДОБАВИТЬ re-exports
pub use ui_events::pointer::PointerEvent;
pub use ui_events::keyboard::KeyboardEvent;
pub use flui_types::gestures::Velocity;
```

#### Phase 2: Конвертация в Platform

```rust
// flui-platform/src/platforms/winit/input.rs

use ui_events::pointer::*;
use flui_types::geometry::{Offset, Pixels, PixelDelta};

impl WinitPlatform {
    fn convert_pointer_event(
        &self,
        winit_event: &winit::event::WindowEvent,
    ) -> Option<PointerEvent> {
        match winit_event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.scale_factor();
                
                // Конвертируем в логические пиксели СРАЗУ
                let logical_pos = Offset::new(
                    Pixels((position.x / scale) as f32),
                    Pixels((position.y / scale) as f32),
                );
                
                // Вычисляем дельту
                let delta = if let Some(last) = self.last_position {
                    Offset::new(
                        PixelDelta((logical_pos.dx - last.dx).0),
                        PixelDelta((logical_pos.dy - last.dy).0),
                    )
                } else {
                    Offset::ZERO
                };
                
                self.last_position = Some(logical_pos);
                
                // Создаём W3C событие
                Some(PointerEvent::Move(PointerUpdate {
                    pointer_id: PointerId::primary(),
                    position: logical_pos,
                    movement: delta,
                    // ...
                }))
            }
            // ... другие события
        }
    }
}
```

#### Phase 3: Включить flui_interaction в Workspace

```toml
# Cargo.toml
[workspace]
members = [
    "crates/flui_types",
    "crates/flui-foundation",
    "crates/flui-tree",
    "crates/flui-platform",
    "crates/flui_interaction",  # ← ВКЛЮЧИТЬ!
    # ...
]
```

#### Phase 4: Подключить в flui_app

```rust
// flui_app/src/embedder/desktop.rs

use flui_interaction::{GestureBinding, PointerEvent};
use flui_platform::PlatformInput;

impl DesktopEmbedder {
    fn handle_platform_input(&mut self, input: PlatformInput) {
        match input {
            PlatformInput::Pointer(pointer_event) => {
                // Передаём напрямую в GestureBinding
                self.gesture_binding.handle_pointer_event(&pointer_event);
            }
            PlatformInput::Keyboard(key_event) => {
                self.gesture_binding.handle_key_event(&key_event);
            }
            // ...
        }
    }
}
```

---

## Type System Unification

### Geometry Types (Foundation)

**flui_types/src/geometry:**
```rust
pub struct Offset<T: Unit> {    // Generic definition
    pub dx: T,
    pub dy: T,
}

pub struct Point<T: Unit> {     // Generic definition
    pub x: T,
    pub y: T,
}

pub struct Pixels(pub f32);     // Absolute coordinates
pub struct PixelDelta(pub f32); // Relative changes
```

### Event Types (Concrete Usage)

**flui-platform → ui-events:**
```rust
// Platform layer creates events with concrete types
let event = PointerEvent::Move(PointerUpdate {
    position: Offset::<Pixels>::new(px(100.0), px(200.0)),
    movement: Offset::<PixelDelta>::new(delta_px(5.0), delta_px(-3.0)),
});
```

**flui_interaction → Uses W3C:**
```rust
// Interaction layer receives W3C events
impl GestureBinding {
    pub fn handle_pointer_event(&mut self, event: &PointerEvent) {
        // event уже W3C тип с правильными координатами
        let position: Offset<Pixels> = event.position();
        let delta: Offset<PixelDelta> = event.movement();
    }
}
```

---

## Migration Checklist

### Step 1: Clean Up flui-platform

- [ ] Remove custom `PointerEvent` struct
- [ ] Remove custom `Velocity` struct  
- [ ] Remove custom `VelocityTracker` struct
- [ ] Add `ui-events` dependency
- [ ] Re-export ui-events types
- [ ] Re-export flui_types::gestures types

### Step 2: Update Platform Implementations

- [ ] Winit: Convert to ui-events
- [ ] Windows: Convert to ui-events
- [ ] Headless: Implement ui-events

### Step 3: Enable flui_interaction

- [ ] Add to Cargo.toml workspace
- [ ] Verify compilation
- [ ] Run tests

### Step 4: Integrate with flui_app

- [ ] Update DesktopEmbedder
- [ ] Connect event flow
- [ ] Test end-to-end

---

## Event Flow Diagram (Final)

```
┌─────────────────────────┐
│   Operating System      │
│  (Windows, macOS, Linux)│
└──────────┬──────────────┘
           │ Native events (WM_MOUSEMOVE, NSEvent, etc.)
           ▼
┌─────────────────────────────────────────┐
│         winit Event Loop                │
│  winit::event::WindowEvent              │
└──────────┬──────────────────────────────┘
           │ winit events
           ▼
┌─────────────────────────────────────────┐
│      flui-platform                      │
│  Conversion: winit → ui-events          │
│                                          │
│  let pos = Offset::<Pixels>::new(...)   │ ← Конвертация здесь!
│  let delta = Offset::<PixelDelta>       │
│                                          │
│  PointerEvent (W3C ui-events)           │
└──────────┬──────────────────────────────┘
           │ W3C PointerEvent
           ▼
┌─────────────────────────────────────────┐
│      flui_app / DesktopEmbedder         │
│  Routing to bindings                    │
└──────────┬──────────────────────────────┘
           │ PointerEvent
           ▼
┌─────────────────────────────────────────┐
│      flui_interaction                   │
│  • GestureBinding                       │
│  • VelocityTracker (from flui_types)   │
│  • Gesture Recognizers                  │
│  • Hit Testing                          │
└──────────┬──────────────────────────────┘
           │ Gesture callbacks
           ▼
┌─────────────────────────────────────────┐
│         User Code                       │
│  onTap(), onDrag(), etc.                │
└─────────────────────────────────────────┘
```

---

## Benefits Summary

### After Integration

✅ **Single Event System** - W3C ui-events everywhere  
✅ **Type Safety** - `Offset<Pixels>` for positions, `Offset<PixelDelta>` for deltas  
✅ **No Duplication** - One `Velocity` in flui_types, one `PointerEvent` from ui-events  
✅ **Standards Compliant** - W3C Pointer Events spec  
✅ **Future Proof** - Easy web platform support  
✅ **Clean Architecture** - Clear boundaries between layers  

### Code Reuse

- `flui_types` - Foundation types (Pixels, PixelDelta, Velocity)
- `ui-events` - W3C event types (PointerEvent, KeyboardEvent)
- `flui-platform` - OS → W3C conversion
- `flui_interaction` - Gesture recognition

---

## Next Steps

1. **Прочитать `ui-events` crate API** - понять какие типы доступны
2. **Исправить flui-platform** - удалить дубликаты, использовать ui-events
3. **Включить flui_interaction** - добавить в workspace
4. **Написать конвертер** - winit → ui-events в platform layer
5. **Интегрировать с app** - подключить GestureBinding

---

**Status:** 🎯 Ready to implement Option A!

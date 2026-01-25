# flui_interaction ↔ flui-platform: Поток событий

> **Дата**: 2026-01-24  
> **Цель**: Объяснить как flui_interaction взаимодействует с flui-platform

---

## 🔄 Архитектура потока событий

```
┌─────────────────────────────────────────────────────────────────┐
│                    OS / Hardware Layer                           │
│  (Mouse движение, Touch, Keyboard, Pen, Gamepad)                │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                     flui-platform                                │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  WindowsPlatform / WinitPlatform / HeadlessPlatform        │ │
│  │                                                             │ │
│  │  Получает OS events:                                       │ │
│  │  • WM_MOUSEMOVE (Win32)                                    │ │
│  │  • WindowEvent::CursorMoved (winit)                        │ │
│  │  • WM_LBUTTONDOWN, WM_KEYDOWN, etc.                        │ │
│  └────────────────────────────────────────────────────────────┘ │
│                              ↓                                   │
│  Конвертирует в:                                                │
│  • Raw window events (position в physical pixels)               │
│  • Scale factor для DPI                                         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
         ┌────────────────────────────────────┐
         │   НЕТ прямой связи Platform →     │
         │   Interaction! Через промежуточный │
         │   слой: DesktopEmbedder            │
         └────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                  flui_app::embedder::DesktopEmbedder             │
│  (связующий слой между Platform и Framework)                    │
│                                                                  │
│  handle_window_event(winit::WindowEvent):                       │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  1. Получает raw event от Platform                       │  │
│  │  2. Конвертирует physical → logical pixels               │  │
│  │  3. Создает ui-events структуры (W3C compliant)          │  │
│  │  4. Вызывает AppBinding.handle_*()                       │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                      flui_app::AppBinding                        │
│  (Application singleton coordinator)                             │
│                                                                  │
│  Методы:                                                         │
│  • handle_pointer_move(position: Offset, device: PointerType)  │
│  • handle_pointer_button(...)                                   │
│  • handle_key_event(KeyboardEvent)                             │
│  • handle_scroll_event(ScrollEventData)                        │
│                                                                  │
│  Создает:                                                        │
│  • PointerEventData (compatibility struct)                      │
│  • Event::Pointer / Event::Keyboard                            │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    flui_interaction                              │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │              GestureBinding (singleton)                     │ │
│  │                                                             │ │
│  │  handle_pointer_event(event, hit_test_fn):                 │ │
│  │  ┌─────────────────────────────────────────────────────┐  │ │
│  │  │ 1. Hit Testing (кто под курсором?)                  │  │ │
│  │  │    → HitTestResult                                  │  │ │
│  │  │                                                      │  │ │
│  │  │ 2. Event Routing                                    │  │ │
│  │  │    → PointerRouter.route()                          │  │ │
│  │  │                                                      │  │ │
│  │  │ 3. Gesture Recognition                              │  │ │
│  │  │    → TapRecognizer, DragRecognizer, etc.           │  │ │
│  │  │                                                      │  │ │
│  │  │ 4. Arena Resolution (конфликты)                     │  │ │
│  │  │    → GestureArena.sweep()                           │  │ │
│  │  └─────────────────────────────────────────────────────┘  │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  Использует типы:                                               │
│  • ui_events::PointerEvent (W3C compliant) ✅                   │
│  • ui_events::KeyboardEvent (W3C compliant) ✅                  │
│  • Offset<Pixels> для позиций ← ВАЖНО!                         │
│  • PixelDelta для scroll delta                                 │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    User Widget Code                              │
│  (gesture callbacks, event handlers)                             │
└─────────────────────────────────────────────────────────────────┘
```

---

## 🎯 Ключевой момент: flui_interaction НЕ зависит от flui-platform!

### Архитектурное разделение:

```rust
// flui-platform
// Ответственность: OS integration
WindowsPlatform::run() {
    // Win32 message loop
    while GetMessage(&msg) {
        match msg.message {
            WM_MOUSEMOVE => {
                // Отправить WindowEvent
                callback(WindowEvent::PointerMoved { x, y })
            }
        }
    }
}
```

```rust
// flui_app::embedder
// Ответственность: Platform → Framework конверсия
impl DesktopEmbedder {
    fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position } => {
                // Physical → Logical pixels
                let logical_pos = position / scale_factor;
                
                // Вызов AppBinding (framework layer)
                AppBinding::instance().handle_pointer_move(
                    Offset::new(logical_pos.x, logical_pos.y),
                    PointerType::Mouse
                );
            }
        }
    }
}
```

```rust
// flui_interaction
// Ответственность: Framework events → Gestures
// НЕ ЗНАЕТ про Platform!
impl GestureBinding {
    pub fn handle_pointer_event<F>(
        &self,
        event: &PointerEvent,  // ui-events type (W3C)
        hit_test: F            // Closure для hit testing
    ) where F: FnOnce(Offset<Pixels>) -> HitTestResult
    {
        // Hit test
        let result = hit_test(event.position());
        
        // Route to gesture recognizers
        self.router.route(event, &result);
    }
}
```

---

## 📦 Типы данных в каждом слое

### 1. flui-platform Layer

```rust
// Raw OS data
WindowEvent::CursorMoved {
    position: PhysicalPosition<f64>,  // Physical pixels от OS
}

WindowEvent::MouseInput {
    state: ElementState,
    button: MouseButton,
}

WindowEvent::KeyboardInput {
    device_id: DeviceId,
    event: KeyEvent,
}
```

**Характеристики**:
- ❌ Не знает про logical pixels
- ❌ Не знает про gesture recognition
- ✅ Просто передает OS events вверх

---

### 2. DesktopEmbedder Layer (конверсия)

```rust
// Конвертирует Physical → Logical
let physical_pos = PhysicalPosition { x: 500.0, y: 300.0 };
let scale_factor = 2.0;  // HiDPI display

// → Logical pixels
let logical_x = physical_pos.x / scale_factor;  // 250.0
let logical_y = physical_pos.y / scale_factor;  // 150.0

// → Framework type
let offset = Offset::new(px(logical_x), px(logical_y));
```

**Характеристики**:
- ✅ Знает про DPI scaling
- ✅ Конвертирует в framework types
- ✅ Создает ui-events структуры

---

### 3. flui_interaction Layer

```rust
// Использует W3C compliant types
use ui_events::pointer::{PointerEvent, PointerType};
use ui_events::keyboard::KeyboardEvent;

// И framework geometry types
use flui_types::geometry::{Offset, Pixels, PixelDelta};

// Пример: PointerEventData (compatibility struct)
pub struct PointerEventData {
    pub position: Offset<Pixels>,        // Logical screen coords
    pub local_position: Offset<Pixels>,  // Widget-local coords
    pub device_kind: PointerType,
    pub pressure: f32,
    pub buttons: PointerButtons,
}
```

**Характеристики**:
- ✅ Работает с **logical pixels** (Offset<Pixels>)
- ✅ W3C compliant (ui-events crate)
- ❌ НЕ знает про Platform
- ❌ НЕ знает про физические пиксели

---

## 🤔 Решение для generic types в flui_interaction

### Текущая проблема:

```rust
// flui_interaction использует Offset для:

1. Позиции курсора/тача
   let position: Offset<???> = ...;
   
2. Velocity (скорость движения)
   let velocity: Offset<???> = ...;  // pixels per second
   
3. Delta (смещение)
   let delta: Offset<???> = ...;     // change in position
```

### Что приходит от Platform?

```rust
// DesktopEmbedder конвертирует:
Physical pixels → Logical pixels (Pixels)

AppBinding::handle_pointer_move(
    position: Offset<Pixels>,  // ← Logical pixels!
    device: PointerType
)
```

### Правильное решение: **Option C (Mixed)**

```rust
// flui_interaction должен использовать:

1. Позиции (position, local_position)
   → Offset<Pixels>  ✅ Logical screen coordinates

2. Velocity (скорость)
   → Offset<f32>  ✅ Unit-agnostic delta (pixels/second)
   
3. Delta (смещение)
   → Offset<f32>  ✅ Change in position (dimensionless)
```

### Почему Option C правильный:

#### ✅ Семантически корректно:
- **Position** = координата на экране → `Offset<Pixels>` (has unit)
- **Velocity** = изменение в секунду → `Offset<f32>` (dimensionless rate)
- **Delta** = разница позиций → `Offset<f32>` (dimensionless difference)

#### ✅ Совместимо с Platform:
```rust
// DesktopEmbedder отправляет:
AppBinding.handle_pointer_move(
    position: Offset<Pixels>  // ← Logical pixels
)

// GestureBinding получает:
handle_pointer_event(
    event.position: Offset<Pixels>  // ✅ Matches!
)

// Velocity tracking:
let delta: Offset<f32> = new_pos.to_f32() - old_pos.to_f32();
let velocity: Offset<f32> = delta / dt;  // ✅ No units!
```

#### ✅ Соответствует физике:
```
Position [Pixels]
Delta = Position₂ - Position₁ [Pixels - Pixels = dimensionless]
Velocity = Delta / Time [dimensionless / seconds = dimensionless/s]
```

---

## 📋 Конкретный план исправления

### Шаг 1: Определить типы

```rust
// flui_interaction/src/types.rs (new file)

/// Position in screen coordinates (logical pixels)
pub type ScreenPosition = Offset<Pixels>;

/// Velocity in pixels per second (dimensionless rate)
pub type Velocity = Offset<f32>;

/// Position delta (dimensionless change)
pub type PositionDelta = Offset<f32>;
```

### Шаг 2: Обновить PointerEventData

```rust
pub struct PointerEventData {
    /// Position in global coordinates (logical pixels)
    pub position: ScreenPosition,  // = Offset<Pixels>
    
    /// Position in local widget coordinates
    pub local_position: ScreenPosition,  // = Offset<Pixels>
    
    /// Device that generated the event
    pub device_kind: PointerType,
    
    // ... rest unchanged ...
}
```

### Шаг 3: Обновить Velocity tracking

```rust
// processing/velocity.rs
pub struct VelocityTracker {
    samples: Vec<(Instant, ScreenPosition)>,  // (time, position)
}

impl VelocityTracker {
    pub fn add_sample(&mut self, time: Instant, position: ScreenPosition) {
        self.samples.push((time, position));
    }
    
    pub fn compute_velocity(&self) -> Velocity {
        let (t1, p1) = self.samples[0];
        let (t2, p2) = self.samples.last().unwrap();
        
        let dt = (t2 - t1).as_secs_f32();
        
        // Convert Pixels to f32 for calculation
        let delta = PositionDelta::new(
            p2.x.0 - p1.x.0,  // f32
            p2.y.0 - p1.y.0   // f32
        );
        
        Velocity::new(delta.x / dt, delta.y / dt)
    }
}
```

### Шаг 4: Обновить DragRecognizer

```rust
// recognizers/drag.rs
pub struct DragGestureRecognizer {
    initial_position: Option<ScreenPosition>,  // Offset<Pixels>
    current_position: Option<ScreenPosition>,
}

impl DragGestureRecognizer {
    pub fn handle_move(&mut self, position: ScreenPosition) {
        if let Some(initial) = self.initial_position {
            // Compute delta as f32
            let delta = PositionDelta::new(
                position.x.0 - initial.x.0,
                position.y.0 - initial.y.0,
            );
            
            if delta.magnitude() > self.min_drag_distance {
                self.accept_gesture();
            }
        }
        
        self.current_position = Some(position);
    }
}
```

---

## ✅ Итоговая архитектура типов

```
Platform Layer (Physical)
    ↓
Physical pixels (i32 или f64)
    ↓ [DPI scaling]
DesktopEmbedder
    ↓
Logical pixels (Pixels = f32)
    ↓
AppBinding
    ↓
Offset<Pixels> для позиций
    ↓
flui_interaction::GestureBinding
    ├─ position: Offset<Pixels>        ✅ Screen coordinates
    ├─ delta: Offset<f32>              ✅ Dimensionless change
    └─ velocity: Offset<f32>           ✅ Pixels per second
    ↓
User callbacks
```

---

## 🎯 Вывод

### flui_interaction взаимодействует с flui-platform через:

1. **НЕ напрямую!** ❌
2. **Через DesktopEmbedder** (flui_app) ✅
3. **Через AppBinding** (flui_app) ✅

### Поток данных:

```
OS → Platform → DesktopEmbedder → AppBinding → GestureBinding → Recognizers → User
```

### Типы:

| Слой | Position Type | Delta Type | Velocity Type |
|------|---------------|------------|---------------|
| **Platform** | Physical pixels | - | - |
| **Embedder** | Logical pixels → Offset<Pixels> | - | - |
| **AppBinding** | Offset<Pixels> | - | - |
| **Interaction** | Offset<Pixels> | **Offset<f32>** | **Offset<f32>** |

### Решение для generic types:

✅ **Option C: Mixed approach**
- Positions: `Offset<Pixels>` (has unit)
- Deltas: `Offset<f32>` (dimensionless)
- Velocities: `Offset<f32>` (dimensionless rate)

---

**Документ актуален**: 2026-01-24  
**Следующий шаг**: Применить Option C к flui_interaction (592 ошибки)

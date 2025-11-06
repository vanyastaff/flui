# Gesture System Research - Other Frameworks

Comparison of gesture handling in different UI frameworks to inform FLUI's design.

## 1. Flutter (Our Primary Inspiration)

### Architecture
```
User Input → Hit Testing → Gesture Arena → Winner Recognizer → Callback
```

### GestureDetector API
```dart
GestureDetector(
  behavior: HitTestBehavior.opaque,  // or translucent, deferToChild
  onTap: () => print('Tapped!'),
  onTapDown: (details) => {},
  onTapUp: (details) => {},
  onTapCancel: () => {},
  onDoubleTap: () => {},
  onLongPress: () => {},
  onPanStart: (details) => {},
  onPanUpdate: (details) => {},
  onPanEnd: (details) => {},
  onScaleStart: (details) => {},
  onScaleUpdate: (details) => {},
  onScaleEnd: (details) => {},
  child: Container(...),
)
```

### Key Concepts

**Gesture Arena:**
- Multiple recognizers compete for each pointer
- First come, first served for nested detectors
- Recognizers can:
  - Eliminate themselves (concede)
  - Declare victory (win immediately)
  - Wait for arena resolution

**Hit Test Behavior:**
- `opaque` - Always receives events, blocks parents
- `translucent` - Receives events, passes to parents
- `deferToChild` - Only receives if child doesn't handle

**Callback Timing:**
- Some callbacks fire BEFORE arena resolves (e.g., `onTapDown`)
- Final callbacks fire AFTER winning arena (e.g., `onTap`)

### Strengths
✅ Comprehensive gesture coverage (tap, drag, scale, long-press)
✅ Proper conflict resolution via gesture arena
✅ Flexible hit testing behavior
✅ Callback phases (down/start/update/end/cancel)

## 2. Dioxus (Web-focused)

### Event Handler Syntax
```rust
rsx! {
    button {
        onclick: move |event| {
            log::info!("Clicked! Event: {event:?}");
        },
        "Click me"
    }
}
```

### Key Features

**'static Closures:**
```rust
let count = use_signal(|| 0);
let increment = move |_| count += 1;  // Signal is Copy

button {
    onclick: increment,
    "Increment"
}
```

**Event Types:**
- Mouse events: `onclick`, `onmousedown`, `onmouseup`, `onmousemove`
- Touch events: `ontouchstart`, `ontouchend`, `ontouchmove`
- Pointer events: `onpointerup`, `onpointerdown`
- Keyboard events: `onkeydown`, `onkeyup`

**Event Propagation:**
```rust
onclick: move |event| {
    event.stop_propagation();  // Prevent bubbling
    event.prevent_default();   // Prevent browser action
}
```

**Async Support:**
```rust
onclick: move |_| async move {
    let data = fetch_data().await;
    process(data);
}
```

### Strengths
✅ Simple, declarative syntax
✅ Copy signals work seamlessly with closures
✅ Async event handlers out of the box
✅ Web standard event model

### Weaknesses
❌ No gesture recognition (just raw events)
❌ No gesture arena concept
❌ Web-specific (not native gestures)

## 3. Leptos (Fine-grained Reactivity)

### Event Handler Syntax
```rust
view! {
    <button on:click=move |_| set_count.update(|n| *n += 1)>
        "Increment"
    </button>
}
```

### Key Features

**Signal Integration:**
```rust
let (count, set_count) = create_signal(0);

// Direct signal updates in handlers
on:click=move |_| set_count.update(|n| *n += 1)

// Signals are Copy (like our implementation!)
```

**Event Attributes:**
- Prefix: `on:` (not `onclick`, but `on:click`)
- Any DOM event works: `on:click`, `on:input`, `on:submit`
- Can pass closures or functions

### Strengths
✅ Elegant syntax with `on:` prefix
✅ Copy signals (no cloning needed)
✅ Fine-grained reactivity (only updates what changed)

### Weaknesses
❌ Web-focused (no native gesture recognition)
❌ No gesture arena
❌ Basic event model only

## 4. Iced (Elm Architecture)

### Event Handling Pattern
```rust
#[derive(Debug, Clone)]
enum Message {
    ButtonPressed,
    InputChanged(String),
}

impl Application for MyApp {
    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ButtonPressed => {
                self.count += 1;
                Command::none()
            }
            Message::InputChanged(value) => {
                self.input = value;
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<Message> {
        button("Click me")
            .on_press(Message::ButtonPressed)
    }
}
```

### Key Features

**Message Passing:**
- All interactions produce messages
- Central update() function handles all state changes
- Type-safe message enums

**Event as Messages:**
- Button press → Message
- Text input → Message
- Mouse events → Message

### Strengths
✅ Type-safe message system
✅ Centralized state management
✅ Predictable update flow

### Weaknesses
❌ No gesture recognition layer
❌ More boilerplate than closure-based systems
❌ Not as ergonomic for simple interactions

## 5. egui (Immediate Mode)

### Interaction Pattern
```rust
if ui.button("Click me").clicked() {
    count += 1;
}

let response = ui.button("Hover me");
if response.hovered() {
    // Show tooltip
}
if response.dragged() {
    // Handle drag
}
```

### Key Features

**Response Object:**
```rust
struct Response {
    clicked: bool,
    hovered: bool,
    dragged: bool,
    // ... many more
}

// Check interactions
let r = ui.add(widget);
if r.clicked() { ... }
if r.double_clicked() { ... }
if r.drag_started() { ... }
```

**PointerState:**
```rust
let pointer = ui.input(|i| i.pointer.clone());
if pointer.primary_pressed() { ... }
if pointer.any_down() { ... }
if let Some(pos) = pointer.interact_pos() { ... }
```

### Strengths
✅ Simple, direct interaction checks
✅ No need for callbacks (immediate mode)
✅ Pointer state easily accessible

### Weaknesses
❌ Immediate mode paradigm (not retained)
❌ No gesture recognition abstraction
❌ No gesture arena (first widget that claims it wins)

## Recommendations for FLUI

Based on this research, here's what we should adopt:

### 1. Flutter-Style Gesture System (Core Architecture)
✅ **Adopt:** Gesture arena for conflict resolution
✅ **Adopt:** Multiple recognizers per detector
✅ **Adopt:** Hit testing integration
✅ **Adopt:** Callback phases (down/start/update/end/cancel)

### 2. Dioxus-Style Ergonomics (API Design)
✅ **Adopt:** Closures with move semantics
✅ **Adopt:** Copy signals (no cloning!)
✅ **Consider:** Async support for handlers

### 3. Type-Safe Callbacks (Rust Best Practices)
✅ **Adopt:** Generic callback types
✅ **Adopt:** Arc-wrapped callbacks for thread safety
✅ **Adopt:** Send + Sync bounds where appropriate

## Proposed FLUI GestureDetector API

```rust
use flui_gestures::prelude::*;

// Simple tap
GestureDetector::builder()
    .on_tap(|| println!("Tapped!"))
    .child(container)
    .build()

// With event data
GestureDetector::builder()
    .on_tap_down(|event| {
        println!("Tap at: {:?}", event.position);
    })
    .on_tap(|event| {
        println!("Tap completed!");
    })
    .on_tap_up(|event| {
        println!("Pointer released");
    })
    .child(container)
    .build()

// Working with signals (Copy, no clone!)
let count = use_signal(ctx, 0);

GestureDetector::builder()
    .on_tap(move || count.update(|n| n + 1))  // No clone needed!
    .child(Text::new(format!("Count: {}", count.get())))
    .build()

// Hit test behavior
GestureDetector::builder()
    .behavior(HitTestBehavior::Translucent)  // Pass through to parent
    .on_tap(|| println!("Tapped!"))
    .child(container)
    .build()
```

## Implementation Priority

### Phase 1: Basic Tap Recognition (Current)
- ✅ TapGestureRecognizer struct
- ⏳ PointerRouter for event dispatch
- ⏳ GestureDetector widget
- ⏳ Integration with flui_engine::EventRouter

### Phase 2: Gesture Arena
- GestureArena for conflict resolution
- Multiple recognizers per detector
- Arena entry/exit/winning logic

### Phase 3: Additional Gestures
- LongPressGestureRecognizer
- DragGestureRecognizer (vertical, horizontal, pan)
- ScaleGestureRecognizer (pinch/zoom)

### Phase 4: Advanced Features
- Async callbacks (like Dioxus)
- Gesture customization (timeouts, thresholds)
- Multi-touch support

## Key Design Decisions

1. **Use flui_engine::EventRouter** - Don't reinvent hit testing
2. **Copy signals** - Already implemented, works great!
3. **Builder pattern** - Ergonomic API like Flutter
4. **Arc<dyn Fn()>** - Thread-safe callbacks
5. **Gesture arena** - Proper conflict resolution
6. **Separate crate** - Clean architecture boundary

## References

- Flutter gestures: https://docs.flutter.dev/ui/interactivity/gestures
- Dioxus events: https://dioxuslabs.com/learn/0.6/reference/event_handlers/
- Leptos signals: https://book.leptos.dev/
- Iced architecture: https://book.iced.rs/
- egui response: https://docs.rs/egui/latest/egui/struct.Response.html

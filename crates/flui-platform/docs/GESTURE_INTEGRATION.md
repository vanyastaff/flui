# Gesture System Integration Architecture

This document explains how `flui-gesture` will integrate with the pointer event system in `flui-platform`.

## Overview

The pointer event system in `flui-platform` is designed from the ground up to support gesture recognition. It provides all the data and abstractions needed for gesture recognizers to detect taps, drags, pinches, rotates, and other multi-touch gestures.

## Architecture Layers

```
┌─────────────────────────────────────────────┐
│          flui-widgets                       │
│  (Widgets like GestureDetector)            │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│          flui-gesture                       │
│  (Gesture Recognizers: Tap, Drag, etc.)    │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│          flui-platform                      │
│  (PointerEvent, VelocityTracker)           │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│     Platform (winit/Android/iOS)            │
│  (Raw input events from OS)                 │
└─────────────────────────────────────────────┘
```

## PointerEvent: Foundation for Gestures

The `PointerEvent` struct contains everything gesture recognizers need:

```rust
pub struct PointerEvent {
    // Identity tracking
    pub pointer_id: u64,        // Stable across down/move/up
    pub device_id: u32,         // Distinguish multiple devices
    
    // Position and movement
    pub position: Point<Pixels>, // Current position
    pub delta: Point<Pixels>,    // Movement since last event
    
    // Timing
    pub timestamp: Instant,      // For velocity calculation
    
    // State
    pub phase: PointerPhase,     // Down/Move/Up/Cancel
    pub kind: PointerKind,       // Mouse/Touch/Pen
    
    // Gesture hints
    pub click_count: usize,      // Double-click detection
    pub pressure: Option<f32>,   // Force touch gestures
    pub tilt: Option<PointerTilt>, // Pen gestures
    
    pub modifiers: Modifiers,
}
```

### Key Design Decisions

1. **Unified API**: Same event type for mouse, touch, and pen
   - `PointerKind::Mouse(MouseButton::Left)` - Desktop
   - `PointerKind::Touch { id: 0 }` - Mobile/Tablet
   - `PointerKind::Pen` - Stylus

2. **Multi-touch Support**: `pointer_id` allows tracking multiple fingers
   - Each finger gets unique ID
   - Gestures can track 2+ pointers (pinch, rotate)

3. **Velocity Tracking**: Built-in `VelocityTracker`
   - Calculates pixels/second from event stream
   - Essential for fling/swipe gestures

4. **Delta Tracking**: Every event includes movement delta
   - Critical for drag gesture detection
   - No need to store previous position

## VelocityTracker: Fling Detection

```rust
let mut tracker = VelocityTracker::new();

// Feed pointer events
tracker.add_sample(&pointer_event);

// Calculate velocity
if let Some(velocity) = tracker.velocity() {
    if velocity.is_fling(50.0) {  // 50 px/sec threshold
        // Trigger fling gesture
    }
}
```

**Usage in Gestures:**
- **Swipe**: Detect fast directional movement
- **Fling**: Detect high-velocity release for momentum scrolling
- **Drag**: Distinguish slow drag from fast swipe

## Gesture Recognizer Pattern (Flutter-inspired)

Based on Flutter's gesture arena system, `flui-gesture` will implement:

### 1. Gesture Arena

Multiple gesture recognizers compete to handle the same pointer event:

```rust
pub struct GestureArena {
    recognizers: Vec<Box<dyn GestureRecognizer>>,
    active_gestures: HashMap<u64, GestureState>,
}
```

**Winner Takes All**: When a recognizer claims victory (e.g., drag moved enough), others are cancelled.

### 2. Gesture Recognizer Trait

```rust
pub trait GestureRecognizer {
    /// Handle pointer event
    fn handle_event(&mut self, event: &PointerEvent) -> GestureDisposition;
    
    /// Called when gesture wins arena
    fn accept(&mut self);
    
    /// Called when gesture loses arena
    fn reject(&mut self);
}

pub enum GestureDisposition {
    /// Still competing (waiting for more events)
    Pending,
    
    /// Claiming victory (drag threshold exceeded)
    Accepted,
    
    /// Giving up (wrong button, wrong direction)
    Rejected,
}
```

### 3. Common Gesture Recognizers

#### TapGestureRecognizer

```rust
pub struct TapGestureRecognizer {
    on_tap: Option<Box<dyn Fn(Point<Pixels>)>>,
    down_position: Option<Point<Pixels>>,
    max_tap_slop: f32,  // 8-10 pixels typical
}

impl GestureRecognizer for TapGestureRecognizer {
    fn handle_event(&mut self, event: &PointerEvent) -> GestureDisposition {
        match event.phase {
            PointerPhase::Down if event.is_primary() => {
                self.down_position = Some(event.position);
                GestureDisposition::Pending
            }
            PointerPhase::Move => {
                // Check if moved too far for tap
                if event.moved_significantly(self.max_tap_slop) {
                    GestureDisposition::Rejected
                } else {
                    GestureDisposition::Pending
                }
            }
            PointerPhase::Up => {
                if let Some(down_pos) = self.down_position {
                    if event.distance_to(down_pos) <= self.max_tap_slop {
                        // Valid tap!
                        if let Some(callback) = &self.on_tap {
                            callback(event.position);
                        }
                        return GestureDisposition::Accepted;
                    }
                }
                GestureDisposition::Rejected
            }
            _ => GestureDisposition::Rejected,
        }
    }
}
```

#### DragGestureRecognizer

```rust
pub struct DragGestureRecognizer {
    on_start: Option<Box<dyn Fn(DragStartDetails)>>,
    on_update: Option<Box<dyn Fn(DragUpdateDetails)>>,
    on_end: Option<Box<dyn Fn(DragEndDetails)>>,
    
    down_position: Option<Point<Pixels>>,
    drag_threshold: f32,  // Typically 18 pixels
    is_dragging: bool,
    velocity_tracker: VelocityTracker,
}

pub struct DragStartDetails {
    pub global_position: Point<Pixels>,
    pub timestamp: Instant,
}

pub struct DragUpdateDetails {
    pub global_position: Point<Pixels>,
    pub delta: Point<Pixels>,
    pub timestamp: Instant,
}

pub struct DragEndDetails {
    pub velocity: Velocity,
    pub global_position: Point<Pixels>,
}

impl GestureRecognizer for DragGestureRecognizer {
    fn handle_event(&mut self, event: &PointerEvent) -> GestureDisposition {
        match event.phase {
            PointerPhase::Down if event.is_primary() => {
                self.down_position = Some(event.position);
                self.velocity_tracker.clear();
                self.velocity_tracker.add_sample(event);
                GestureDisposition::Pending
            }
            PointerPhase::Move => {
                self.velocity_tracker.add_sample(event);
                
                if !self.is_dragging {
                    // Check if exceeded threshold
                    if event.moved_significantly(self.drag_threshold) {
                        self.is_dragging = true;
                        
                        if let Some(callback) = &self.on_start {
                            callback(DragStartDetails {
                                global_position: event.position,
                                timestamp: event.timestamp,
                            });
                        }
                        
                        return GestureDisposition::Accepted;
                    }
                    GestureDisposition::Pending
                } else {
                    // Already dragging - send update
                    if let Some(callback) = &self.on_update {
                        callback(DragUpdateDetails {
                            global_position: event.position,
                            delta: event.delta,
                            timestamp: event.timestamp,
                        });
                    }
                    GestureDisposition::Accepted
                }
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                if self.is_dragging {
                    if let Some(callback) = &self.on_end {
                        let velocity = self.velocity_tracker
                            .velocity()
                            .unwrap_or(Velocity { x: 0.0, y: 0.0 });
                        
                        callback(DragEndDetails {
                            velocity,
                            global_position: event.position,
                        });
                    }
                    GestureDisposition::Accepted
                } else {
                    GestureDisposition::Rejected
                }
            }
            _ => GestureDisposition::Rejected,
        }
    }
}
```

#### PinchGestureRecognizer (Multi-touch)

```rust
pub struct PinchGestureRecognizer {
    on_start: Option<Box<dyn Fn(ScaleStartDetails)>>,
    on_update: Option<Box<dyn Fn(ScaleUpdateDetails)>>,
    on_end: Option<Box<dyn Fn(ScaleEndDetails)>>,
    
    pointers: HashMap<u64, Point<Pixels>>, // Track 2+ fingers
    initial_span: Option<f32>,
}

impl GestureRecognizer for PinchGestureRecognizer {
    fn handle_event(&mut self, event: &PointerEvent) -> GestureDisposition {
        match event.phase {
            PointerPhase::Down => {
                // Track this pointer
                self.pointers.insert(event.pointer_id, event.position);
                
                if self.pointers.len() >= 2 {
                    // Two fingers down - calculate initial span
                    let positions: Vec<_> = self.pointers.values().copied().collect();
                    let span = positions[0].distance_to(positions[1]);
                    self.initial_span = Some(span);
                    
                    if let Some(callback) = &self.on_start {
                        callback(ScaleStartDetails {
                            focal_point: self.calculate_focal_point(),
                        });
                    }
                    
                    GestureDisposition::Accepted
                } else {
                    GestureDisposition::Pending
                }
            }
            PointerPhase::Move => {
                if self.pointers.len() >= 2 {
                    // Update pointer position
                    self.pointers.insert(event.pointer_id, event.position);
                    
                    // Calculate scale
                    let positions: Vec<_> = self.pointers.values().copied().collect();
                    let current_span = positions[0].distance_to(positions[1]);
                    let scale = current_span / self.initial_span.unwrap_or(1.0);
                    
                    if let Some(callback) = &self.on_update {
                        callback(ScaleUpdateDetails {
                            focal_point: self.calculate_focal_point(),
                            scale,
                        });
                    }
                    
                    GestureDisposition::Accepted
                } else {
                    GestureDisposition::Pending
                }
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                self.pointers.remove(&event.pointer_id);
                
                if self.pointers.is_empty() {
                    if let Some(callback) = &self.on_end {
                        callback(ScaleEndDetails {});
                    }
                    GestureDisposition::Accepted
                } else {
                    GestureDisposition::Pending
                }
            }
        }
    }
}
```

## Integration with Widget System

In `flui-widgets`, a `GestureDetector` widget will wire up gesture recognizers:

```rust
GestureDetector::new()
    .on_tap(|position| {
        println!("Tapped at {:?}", position);
    })
    .on_double_tap(|position| {
        println!("Double tapped");
    })
    .on_long_press(|position| {
        println!("Long pressed");
    })
    .on_drag_start(|details| {
        println!("Drag started");
    })
    .on_drag_update(|details| {
        println!("Dragging: delta={:?}", details.delta);
    })
    .on_drag_end(|details| {
        println!("Fling velocity: {:?}", details.velocity);
    })
    .child(Container::new())
```

## Platform Integration (winit)

The next step is converting winit events to `PointerEvent`:

```rust
// In WinitApp::window_event()
match event {
    WinitWindowEvent::CursorMoved { position, .. } => {
        let pointer_event = PointerEvent {
            pointer_id: 0,  // Mouse is always ID 0
            device_id: 0,
            kind: PointerKind::Mouse(/* current button */),
            position: Point::new(px(position.x), px(position.y)),
            delta: /* calculate from last position */,
            phase: PointerPhase::Move,
            timestamp: Instant::now(),
            click_count: 0,
            pressure: None,
            tilt: None,
            modifiers: /* from winit */,
        };
        
        // Dispatch to gesture arena
    }
    
    WinitWindowEvent::MouseInput { button, state, .. } => {
        let phase = match state {
            ElementState::Pressed => PointerPhase::Down,
            ElementState::Released => PointerPhase::Up,
        };
        
        let pointer_event = PointerEvent {
            phase,
            kind: PointerKind::Mouse(button.into()),
            // ... fill in fields
        };
    }
    
    WinitWindowEvent::Touch(touch) => {
        let phase = match touch.phase {
            TouchPhase::Started => PointerPhase::Down,
            TouchPhase::Moved => PointerPhase::Move,
            TouchPhase::Ended => PointerPhase::Up,
            TouchPhase::Cancelled => PointerPhase::Cancel,
        };
        
        let pointer_event = PointerEvent {
            pointer_id: touch.id,
            kind: PointerKind::Touch { id: touch.id },
            position: Point::new(px(touch.location.x), px(touch.location.y)),
            phase,
            pressure: Some(touch.force.unwrap_or(1.0)),
            // ... fill in fields
        };
    }
}
```

## Benefits of This Architecture

1. **Platform Agnostic**: Same gesture code works on desktop and mobile
2. **Multi-touch Ready**: `pointer_id` tracks multiple fingers naturally
3. **Velocity Built-in**: No need for gesture recognizers to track velocity
4. **Type Safe**: Rust enums prevent invalid pointer kinds
5. **GPUI Proven**: Based on production framework patterns
6. **Flutter Inspired**: Gesture arena pattern is battle-tested
7. **Extensible**: Easy to add new gesture types (rotate, swipe, etc.)

## Future Enhancements

1. **Gamepad Gestures**: Add `PointerKind::Gamepad` for controller input
2. **Gesture Customization**: Allow apps to configure thresholds
3. **Accessibility**: Integration with screen readers and assistive tech
4. **Platform-specific Gestures**: 
   - macOS: Force touch, swipe between pages
   - iOS: Edge swipe, 3D touch
   - Android: Back gesture, app switching
5. **Web Support**: Map pointer events to web PointerEvent API

## Next Steps

1. ✅ Design `PointerEvent` with all gesture data
2. ✅ Implement `VelocityTracker` for fling detection
3. ⏳ Convert winit events to `PointerEvent`
4. ⏳ Create `flui-gesture` crate with gesture arena
5. ⏳ Implement basic gesture recognizers (Tap, Drag, LongPress)
6. ⏳ Create `GestureDetector` widget
7. ⏳ Add multi-touch gestures (Pinch, Rotate)
8. ⏳ Platform-specific optimizations

## References

- Flutter Gesture System: `.flutter/src/gestures/`
- GPUI Interactive: `.gpui/src/interactive.rs`
- winit Touch Events: winit 0.30 documentation

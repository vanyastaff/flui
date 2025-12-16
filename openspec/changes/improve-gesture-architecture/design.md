# Design: Gesture Architecture Improvements

## Context

Flutter's gesture system has evolved over years of production use. Key patterns that make it robust:
1. **Eager Winner** - first to accept wins immediately when arena closes
2. **GestureBinding** - single coordination point for all gesture handling
3. **State Machine** - predictable recognizer lifecycle
4. **Hold/Release** - delayed decisions without blocking other pointers

Our current implementation has arena and recognizers but lacks these refinements.

## Goals

- Match Flutter's gesture disambiguation behavior
- Provide clear integration point for rendering system
- Enable device-specific gesture tuning
- Maintain thread-safety and performance

## Non-Goals

- Full Flutter API compatibility (we use Rust idioms)
- Multi-window support (future work)
- Accessibility gesture handling (separate spec)

## Decisions

### Decision 1: Eager Winner in Arena

**What**: Add `eager_winner: Option<Arc<dyn GestureArenaMember>>` to `ArenaEntry`

**Why**: 
- Recognizer can claim victory while arena still accepting members
- Resolution happens atomically when arena closes
- Prevents race conditions between accept and close

**Implementation**:
```rust
pub fn resolve(&self, pointer: PointerId, disposition: GestureDisposition) {
    let mut entries = self.entries.write();
    if let Some(entry) = entries.get_mut(&pointer) {
        match disposition {
            GestureDisposition::Accepted => {
                if entry.is_open {
                    // Store as eager winner, resolve on close
                    entry.eager_winner = Some(member.clone());
                } else {
                    // Arena closed, resolve immediately
                    self.resolve_in_favor_of(entry, member);
                }
            }
            GestureDisposition::Rejected => {
                entry.members.retain(|m| !Arc::ptr_eq(m, &member));
                member.reject_gesture(pointer);
            }
        }
    }
}
```

### Decision 2: GestureBinding Structure

**What**: Central coordinator struct

```rust
pub struct GestureBinding {
    hit_tests: DashMap<PointerId, HitTestResult>,
    pointer_router: PointerRouter,
    arena: GestureArena,
    settings: GestureSettings,
}

impl GestureBinding {
    pub fn handle_pointer_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Down(e) => {
                let result = self.hit_test(e.position());
                self.hit_tests.insert(pointer_id, result.clone());
                self.dispatch_event(event, &result);
                self.arena.close(pointer_id);
            }
            PointerEvent::Up(e) | PointerEvent::Cancel(_) => {
                if let Some(result) = self.hit_tests.remove(&pointer_id) {
                    self.dispatch_event(event, &result.1);
                }
                self.arena.sweep(pointer_id);
            }
            _ => {
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                }
            }
        }
    }
}
```

**Why**:
- Single entry point for platform integration
- Hit test caching avoids redundant tree traversal
- Clear lifecycle: down → route → close → (moves) → up → sweep

### Decision 3: Recognizer Trait Hierarchy

```
GestureArenaMember (trait)
    │
    ├── GestureRecognizer (trait) - base with add_pointer, handle_event
    │       │
    │       └── OneSequenceGestureRecognizer (trait) - single pointer tracking
    │               │
    │               └── PrimaryPointerGestureRecognizer (trait) - state machine + deadline
```

**Why**:
- Clear separation of concerns
- Each level adds specific functionality
- Follows Flutter's proven hierarchy

### Decision 4: State Machine for PrimaryPointerGestureRecognizer

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureRecognizerState {
    Ready,      // No gesture in progress
    Possible,   // Tracking, not yet accepted
    Accepted,   // Won arena, gesture active
    Defunct,    // Rejected, waiting for pointer release
}
```

**State Transitions**:
```
Ready ─────────────────► Possible (on pointer down)
  ▲                          │
  │                          ├──► Accepted (arena win)
  │                          │        │
  │                          │        └──► Ready (pointer up)
  │                          │
  │                          └──► Defunct (arena loss / slop exceeded)
  │                                   │
  └───────────────────────────────────┘ (all pointers released)
```

### Decision 5: GestureSettings

```rust
pub struct GestureSettings {
    pub touch_slop: f32,           // Default: 18.0
    pub pan_slop: f32,             // Default: 18.0  
    pub scale_slop: f32,           // Default: 1.0
    pub double_tap_slop: f32,      // Default: 100.0
    pub double_tap_timeout: Duration,  // Default: 300ms
    pub long_press_timeout: Duration,  // Default: 500ms
}

impl GestureSettings {
    pub fn for_device(device_kind: PointerType) -> Self {
        match device_kind {
            PointerType::Touch => Self::touch_defaults(),
            PointerType::Mouse => Self::mouse_defaults(),
            PointerType::Pen => Self::pen_defaults(),
            _ => Self::default(),
        }
    }
}
```

**Why**:
- Different devices need different tolerances
- Touch needs larger slop than mouse
- Runtime configurable for accessibility

### Decision 6: Hold/Release Mechanism

```rust
impl GestureArena {
    pub fn hold(&self, pointer: PointerId) {
        if let Some(mut entry) = self.entries.get_mut(&pointer) {
            entry.is_held = true;
        }
    }
    
    pub fn release(&self, pointer: PointerId) {
        if let Some(mut entry) = self.entries.get_mut(&pointer) {
            entry.is_held = false;
            if entry.has_pending_sweep {
                self.sweep(pointer);
            }
        }
    }
    
    pub fn sweep(&self, pointer: PointerId) {
        if let Some(mut entry) = self.entries.get_mut(&pointer) {
            if entry.is_held {
                entry.has_pending_sweep = true;
                return;
            }
            // ... normal sweep logic
        }
    }
}
```

**Why**:
- Long-press needs to delay resolution until timer fires
- Hold prevents premature sweep on pointer up
- Release triggers deferred sweep

## Alternatives Considered

### Alternative A: Keep flat recognizer structure
- **Rejected**: Less code reuse, harder to maintain
- Flutter's hierarchy is proven over years

### Alternative B: Use async for deadlines
- **Rejected**: Adds complexity, tokio dependency
- Simple timer checks on frame tick are sufficient

### Alternative C: Settings as trait object
- **Rejected**: Overhead for simple configuration
- Struct with device-specific constructors is simpler

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| Breaking arena behavior | Extensive tests, gradual rollout |
| Performance regression | Benchmark before/after |
| API complexity | Good defaults, builder pattern |

## Migration Plan

1. Add eager winner to arena (backward compatible)
2. Add hold/release to arena (backward compatible)
3. Add GestureSettings (backward compatible)
4. Add OneSequenceGestureRecognizer trait
5. Add PrimaryPointerGestureRecognizer trait
6. Migrate existing recognizers to new traits
7. Add GestureBinding
8. Update integration tests

## Open Questions

1. Should GestureBinding be a singleton or instance-based?
   - Leaning toward instance for testability
   
2. How to handle multi-window scenarios?
   - Defer to future work, single binding per window

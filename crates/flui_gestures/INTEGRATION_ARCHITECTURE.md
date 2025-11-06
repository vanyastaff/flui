# FLUI Gesture System - Integration Architecture

## Current Situation Analysis

### What Exists

**1. flui_engine::EventRouter** ✅ (Complete)
- Already does hit testing: `root.hit_test(position, &mut result)`
- Already dispatches events: `result.dispatch(event)`
- Tracks pointer down/up for drag handling
- Integrates with Layer::hit_test() trait method

**2. Event Types** ✅ (Complete)
- `PointerEvent`: Down, Up, Move, Enter, Exit, Cancel
- `HitTestResult`: Contains Vec<HitTestEntry>
- `HitTestEntry`: local_position, bounds, **handler: Option<PointerEventHandler>**
- `PointerEventHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>`

**3. Layer Trait** ✅ (Complete)
```rust
pub trait Layer {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Default: check bounds
        // Override to add HitTestEntry with handler
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Default: event not handled
    }
}
```

**4. RenderPointerListener** ⏳ (Skeleton Only)
- Located: `crates/flui_rendering/src/objects/interaction/pointer_listener.rs`
- Has SingleRender implementation
- **NOT FINISHED**: Uses `fn()` instead of `Arc<dyn Fn>`, no hit testing
- TODO comment: "Register hit test area for pointer events"

**5. GestureDetector Widget** ⚠️ (Temporary Solution)
- Located: `crates/flui_widgets/src/gestures/gesture_detector.rs`
- Uses **GLOBAL REGISTRY** (hack!)
- `GESTURE_HANDLERS: Lazy<RwLock<Vec<Arc<GestureHandler>>>>`
- NOT using EventRouter or hit testing
- app.rs calls `dispatch_gesture_event()` directly with egui events

### Problems with Current Implementation

❌ **Global registry hack** - All gesture detectors fire for ALL events
❌ **No hit testing** - Can't tell which widget was actually clicked
❌ **No gesture arena** - Can't resolve conflicts between nested detectors
❌ **Not using EventRouter** - Bypassing the proper event flow
❌ **RenderPointerListener incomplete** - Missing hit_test override

## Correct Architecture

### Event Flow (Should Be)

```
1. egui Input
       ↓
2. flui_app converts to PointerEvent
       ↓
3. flui_engine::EventRouter.route_event()
       ↓
4. EventRouter.route_pointer_event()
       ↓
5. root.hit_test(position, &mut result)
       ↓
6. RenderObject::hit_test() override (e.g., RenderPointerListener)
   - Checks if position is within bounds
   - Adds HitTestEntry with handler: Arc<dyn Fn(&PointerEvent)>
       ↓
7. HitTestResult now contains ALL hit widgets (front to back)
       ↓
8. EventRouter calls result.dispatch(event)
       ↓
9. HitTestResult.dispatch() calls each entry's handler
       ↓
10. Handler contains GestureRecognizer
       ↓
11. Recognizer analyzes event stream and fires user callbacks (onTap, etc.)
```

### Key Insight: NO SEPARATE POINTER ROUTER NEEDED!

❌ **WRONG** (Original TODO.md plan):
```
flui_engine::EventRouter → flui_gestures::PointerRouter → Recognizers
```

✅ **CORRECT** (Actual architecture):
```
flui_engine::EventRouter → RenderObject.hit_test() adds handlers → Handlers contain Recognizers
```

**EventRouter already does everything PointerRouter was supposed to do!**

## Implementation Plan (Revised)

### Phase 1: Fix RenderPointerListener (~2 hours)

**File:** `crates/flui_rendering/src/objects/interaction/pointer_listener.rs`

**Changes needed:**

1. Update callbacks to use Arc instead of fn():
```rust
pub struct PointerCallbacks {
    pub on_pointer_down: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
    pub on_pointer_up: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
    pub on_pointer_move: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
}
```

2. Implement LeafRender instead of SingleRender (if no child):
```rust
impl LeafRender for RenderPointerListener {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Fill available space
        constraints.biggest()
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Create a layer that registers hit test handler
        let mut layer = PictureLayer::new();
        layer.set_bounds(Rect::from_origin_size(offset, self.size));

        // Create handler that calls our callbacks
        let callbacks = self.callbacks.clone();
        let handler = Arc::new(move |event: &PointerEvent| {
            match event {
                PointerEvent::Down(_) => {
                    if let Some(cb) = &callbacks.on_pointer_down {
                        cb(event);
                    }
                }
                PointerEvent::Up(_) => {
                    if let Some(cb) = &callbacks.on_pointer_up {
                        cb(event);
                    }
                }
                PointerEvent::Move(_) => {
                    if let Some(cb) = &callbacks.on_pointer_move {
                        cb(event);
                    }
                }
                _ => {}
            }
        });

        // Register handler via hit test
        // (Need to store handler somewhere accessible during hit_test)
        Box::new(layer)
    }
}
```

**PROBLEM:** Render traits don't have access to hit_test()!

Layer trait has hit_test(), but RenderObject doesn't control its Layer's hit_test directly.

### Solution: Layer Wrapper with Handler

We need a **new Layer type** that wraps another layer and adds event handling:

**File:** `crates/flui_engine/src/layer/pointer_listener_layer.rs` (NEW)

```rust
pub struct PointerListenerLayer {
    /// Child layer to render
    child: BoxedLayer,

    /// Event handler
    handler: PointerEventHandler,

    /// Bounds for hit testing
    bounds: Rect,
}

impl PointerListenerLayer {
    pub fn new(child: BoxedLayer, handler: PointerEventHandler, bounds: Rect) -> Self {
        Self { child, handler, bounds }
    }
}

impl Layer for PointerListenerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        self.child.paint(painter);
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Check if we're hit
        if !self.bounds.contains(position) {
            return false;
        }

        // Add ourselves to hit test result
        let local_pos = position - self.bounds.origin.to_vector();
        let entry = HitTestEntry::with_handler(
            local_pos,
            self.bounds.size,
            self.handler.clone(),
        );
        result.add(entry);

        // Also hit test child
        self.child.hit_test(position, result);

        true
    }
}
```

Then RenderPointerListener.paint() creates this layer:

```rust
fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
    let child_layer = tree.paint_child(child_id, offset);
    let bounds = Rect::from_origin_size(offset, self.size);

    // Create handler from callbacks
    let callbacks = self.callbacks.clone();
    let handler = Arc::new(move |event: &PointerEvent| {
        // Call appropriate callback
        // ... implementation ...
    });

    Box::new(PointerListenerLayer::new(child_layer, handler, bounds))
}
```

### Phase 2: Create GestureRecognizer Integration (~2 hours)

**File:** `crates/flui_gestures/src/detector.rs` (NEW, replacing widgets version)

```rust
pub struct GestureDetector {
    pub child: Box<dyn AnyView>,
    pub on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
    pub on_tap_down: Option<Arc<dyn Fn(&PointerEventData) + Send + Sync>>,
    // ... other callbacks
}

impl View for GestureDetector {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create RenderPointerListener with callbacks that use TapGestureRecognizer
        let mut recognizer = TapGestureRecognizer::new();

        if let Some(on_tap) = self.on_tap {
            recognizer = recognizer.with_on_tap(move |_| on_tap());
        }

        // Return RenderPointerListener with child
        // RenderPointerListener will create PointerListenerLayer
        // which registers hit test handler
        // ...
    }
}
```

### Phase 3: Update flui_app to use EventRouter (~1 hour)

**File:** `crates/flui_app/src/app.rs`

Remove `process_pointer_events()` and `dispatch_gesture_event()`.

Instead, in the render loop:

```rust
// Convert egui events to PointerEvent
if let Some(event) = convert_egui_event(&ui.input()) {
    self.event_router.route_event(&mut scene.root, &event);
}
```

### Phase 4: Remove temporary GestureDetector (~0.5 hours)

**Files to modify:**
- Delete `crates/flui_widgets/src/gestures/gesture_detector.rs`
- Update `crates/flui_widgets/src/gestures/mod.rs`
- Export new GestureDetector from flui_gestures instead

## File Structure (Final)

```
crates/
├── flui_engine/
│   └── src/
│       ├── event_router.rs          ✅ Already complete
│       └── layer/
│           └── pointer_listener.rs  ⏳ NEW - PointerListenerLayer
│
├── flui_gestures/                   ⏳ NEW CRATE
│   └── src/
│       ├── lib.rs
│       ├── detector.rs              → GestureDetector widget
│       ├── recognizers/
│       │   ├── mod.rs
│       │   └── tap.rs               ✅ Already created
│       └── arena.rs                 📅 Future: gesture arena
│
├── flui_rendering/
│   └── src/objects/interaction/
│       └── pointer_listener.rs      ⏳ UPDATE - Use Arc callbacks
│
└── flui_widgets/
    └── src/gestures/
        └── gesture_detector.rs      ❌ DELETE - Temporary hack
```

## Estimated Effort (Revised)

| Task | Original Estimate | Revised Estimate | Notes |
|------|------------------|------------------|-------|
| PointerRouter | 2-3 hours | ❌ NOT NEEDED | EventRouter already does this! |
| PointerListenerLayer | - | 2 hours | NEW - Layer with hit_test |
| Update RenderPointerListener | - | 1 hour | Use Arc callbacks |
| GestureDetector widget | 2-3 hours | 2 hours | Use RenderPointerListener |
| Integration & app.rs | 2-3 hours | 1 hour | Use EventRouter properly |
| Testing & examples | - | 2 hours | Test counter with buttons |
| **Total** | **6-9 hours** | **8 hours** | More accurate understanding |

## Benefits of This Architecture

✅ **Proper hit testing** - Only hit widgets receive events
✅ **No global state** - Each detector manages its own handler
✅ **Gesture arena ready** - Can add conflict resolution later
✅ **Thread-safe** - All handlers are Send + Sync
✅ **Composable** - Layers can nest properly
✅ **Flutter-like** - Matches Flutter's architecture exactly

## Next Steps

1. ✅ Create this architecture document
2. ⏳ Implement PointerListenerLayer in flui_engine
3. ⏳ Update RenderPointerListener to use Arc callbacks
4. ⏳ Create GestureDetector in flui_gestures using RenderPointerListener
5. ⏳ Update app.rs to use EventRouter
6. ⏳ Test with counter example
7. ⏳ Remove temporary GestureDetector from flui_widgets

## Key Decisions

1. **No PointerRouter** - EventRouter already handles routing ✅
2. **PointerListenerLayer** - New layer type for hit test handlers ✅
3. **RenderPointerListener** - Updates to use Arc<dyn Fn> ✅
4. **GestureRecognizer** - In handler, analyzes event stream ✅
5. **Gesture arena** - Phase 2, after basic tap works ✅

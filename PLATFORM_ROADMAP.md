# FLUI Cross-Platform Roadmap

**Goal:** Achieve 9+/10 production readiness across Desktop, Android, iOS, and Web with maximum code sharing.

**Current Status:** Hit testing system is production-ready (9.8/10). Platform implementations need completion.

---

## Architecture: Unified Platform Layer

### Design Principles

1. **Maximum Code Sharing** - 90%+ shared logic in `EmbedderCore`
2. **Minimal Platform Code** - Platform embedders only handle OS-specific APIs
3. **Type Safety** - Zero unsafe code in platform layer
4. **Consistent API** - Same event types, same widget behavior across all platforms

### Current Architecture (Proven on Desktop)

```
┌─────────────────────────────────────────────────────────┐
│ Application Code (100% shared)                          │
│  - Widgets, Views, Business Logic                       │
└─────────────────────────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────┐
│ FLUI Framework (100% shared)                            │
│  ├─ flui_core: Pipeline, Element Tree                   │
│  ├─ flui_rendering: RenderObjects, Layout               │
│  ├─ flui_interaction: Hit Testing, Events (✅ READY)    │
│  ├─ flui_painting: Canvas, Layers                       │
│  └─ flui_widgets: Pre-built Widgets                     │
└─────────────────────────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────┐
│ flui-platform: Platform Abstraction (90% shared)        │
│                                                          │
│  ┌──────────────────────────────────────────────────┐  │
│  │ EmbedderCore (SHARED - 90% of logic)             │  │
│  │  ├─ Event routing (hit testing)        ✅ DONE  │  │
│  │  ├─ Frame coordination                 ✅ DONE  │  │
│  │  ├─ Scene caching                      ✅ DONE  │  │
│  │  ├─ Pointer state tracking             ✅ DONE  │  │
│  │  ├─ Gesture binding                    ✅ DONE  │  │
│  │  └─ Lifecycle management               ✅ DONE  │  │
│  └──────────────────────────────────────────────────┘  │
│                         ↓                                │
│  ┌──────────────────────────────────────────────────┐  │
│  │ Platform Embedders (10% platform-specific)       │  │
│  │                                                   │  │
│  │  DesktopEmbedder    ✅ IMPLEMENTED               │  │
│  │  AndroidEmbedder    ⚠️  PARTIAL (60%)            │  │
│  │  iOSEmbedder        ❌ NOT STARTED               │  │
│  │  WebEmbedder        ❌ NOT STARTED               │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────┐
│ Platform APIs (OS-specific)                             │
│  ├─ Desktop: winit + wgpu                               │
│  ├─ Android: winit + wgpu (Vulkan)                      │
│  ├─ iOS: UIKit + wgpu (Metal)                           │
│  └─ Web: web-sys + wgpu (WebGPU)                        │
└─────────────────────────────────────────────────────────┘
```

---

## Phase 1: Complete Desktop + Android (Priority)

### Desktop Embedder (Current: 8/10 → Target: 9.5/10)

**What Works:**
- ✅ Mouse events (down, up, move, hover)
- ✅ Scroll wheel events
- ✅ Window lifecycle (focus, resize, close)
- ✅ Hit testing with transforms
- ✅ GPU rendering (wgpu)

**Missing (2-3 weeks):**

1. **Keyboard Events** (1-2 days)
   ```rust
   // Add to desktop.rs
   WindowEvent::KeyboardInput { event, .. } => {
       let key_event = convert_keyboard_input(&event);
       self.core.handle_key_event(key_event);
   }

   // Add to embedder_core.rs
   pub fn handle_key_event(&mut self, event: KeyEvent) {
       self.route_event(Event::Key(event));
   }
   ```
   - **Blocker:** Need winit → FLUI key conversion
   - **Types ready:** ✅ PhysicalKey, LogicalKey, KeyEventData
   - **Effort:** Low (conversion table needed)

2. **Scroll Physics Integration** (3-5 days)
   - **Physics ready:** ✅ FrictionSimulation, SpringSimulation
   - **Widget exists:** ✅ Scrollable structure
   - **Missing:** Gesture → Physics → Animation loop

   ```rust
   // Scrollable needs:
   struct ScrollableState {
       controller: ScrollController,
       simulation: Option<Box<dyn Simulation>>,
       animation_controller: AnimationController,
       drag_recognizer: DragGestureRecognizer,
   }

   impl ScrollableState {
       fn on_drag_update(&mut self, delta: f32) {
           self.controller.offset += delta;
           self.request_rebuild();
       }

       fn on_drag_end(&mut self, velocity: f32) {
           // Start physics simulation
           let friction = FrictionSimulation::new(0.05,
               self.controller.offset, velocity);
           self.simulation = Some(Box::new(friction));
           self.animation_controller.start();
       }

       fn tick(&mut self, dt: f32) {
           if let Some(sim) = &self.simulation {
               let new_offset = sim.position(dt);
               self.controller.offset = new_offset;
               if sim.is_done(dt) {
                   self.simulation = None;
               }
           }
       }
   }
   ```

3. **Drag Gesture Widget** (2-3 days)
   - **Recognizer ready:** ✅ DragGestureRecognizer
   - **Missing:** Widget-level API

   ```rust
   // New widget: Draggable
   pub struct Draggable {
       child: Child,
       on_drag_start: Option<DragStartCallback>,
       on_drag_update: Option<DragUpdateCallback>,
       on_drag_end: Option<DragEndCallback>,
   }

   impl Draggable {
       pub fn build(&self, ctx: &BuildContext) -> impl IntoElement {
           GestureDetector::new(self.child.clone())
               .on_pan_start(self.on_drag_start.clone())
               .on_pan_update(self.on_drag_update.clone())
               .on_pan_end(self.on_drag_end.clone())
       }
   }
   ```

4. **Examples** (2-3 days)
   - Click/Tap example
   - Drag-and-drop example
   - Scrolling list example
   - Multi-touch example (Desktop: pinch on trackpad)

**Desktop Roadmap:**
- Week 1: Keyboard events + basic examples
- Week 2: Scroll physics integration
- Week 3: Drag widgets + comprehensive examples

---

### Android Embedder (Current: 6/10 → Target: 9/10)

**What Works:**
- ✅ Touch events (down, up, move, cancel)
- ✅ Window lifecycle (pause, resume)
- ✅ GPU rendering (wgpu + Vulkan)
- ✅ Basic hit testing

**Missing (3-4 weeks):**

1. **Multi-Touch Gesture Disambiguation** (1 week)
   ```rust
   // AndroidEmbedder enhancement
   WindowEvent::Touch(touch) => {
       let pointer_id = PointerId::new(touch.id);

       match touch.phase {
           TouchPhase::Started => {
               // Register touch with gesture arena
               self.core.handle_pointer_down(
                   pointer_id, position, PointerDeviceKind::Touch
               );
           }
           TouchPhase::Moved => {
               // Multi-touch tracking
               self.core.handle_pointer_move(pointer_id, position);
           }
           // ...
       }
   }
   ```

2. **Keyboard Input** (1 week)
   - Soft keyboard support
   - IME (Input Method Editor) integration
   - Text input events

   ```rust
   // Need Android-specific text input handling
   WindowEvent::Ime(ime_event) => {
       match ime_event {
           Ime::Enabled => self.core.show_keyboard(),
           Ime::Preedit(text, cursor) => {
               self.core.handle_ime_preedit(text, cursor)
           }
           Ime::Commit(text) => {
               self.core.handle_ime_commit(text)
           }
           Ime::Disabled => self.core.hide_keyboard(),
       }
   }
   ```

3. **Scroll Physics** (shared with Desktop)
   - Same Scrollable widget
   - Touch-optimized physics parameters
   - Overscroll glow effect (Android-specific)

4. **Android-Specific Features** (1-2 weeks)
   - Back button handling
   - System navigation bar
   - Notification integration
   - Deep linking
   - Permissions

5. **Examples on Android** (1 week)
   - Same examples as Desktop
   - Platform testing

**Android Roadmap:**
- Week 1: Multi-touch + gesture arena
- Week 2: Soft keyboard + IME
- Week 3: Scroll physics (shared)
- Week 4: Platform-specific features + examples

---

## Phase 2: iOS Support (New Platform)

### iOS Embedder (Current: 0/10 → Target: 9/10)

**Architecture (based on Desktop/Android success):**

```rust
// flui-platform/src/platforms/ios.rs
pub struct iOSEmbedder {
    core: EmbedderCore,      // ✅ SHARED (90%)
    window: UIWindowWrapper,  // iOS-specific
    renderer: GpuRenderer,   // ✅ SHARED (wgpu + Metal)
    capabilities: iOSCapabilities,
}

impl iOSEmbedder {
    pub async fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<Scheduler>,
        event_router: Arc<RwLock<EventRouter>>,
        ui_window: UIWindow,  // iOS UIKit window
    ) -> Result<Self> {
        // 1. Wrap UIWindow
        let window = UIWindowWrapper::new(ui_window);

        // 2. Create Metal surface via wgpu
        let renderer = GpuRenderer::new_async_with_metal_layer(
            window.metal_layer(), width, height
        ).await;

        // 3. Create shared core (same as Desktop/Android!)
        let core = EmbedderCore::new(
            pipeline_owner, needs_redraw, scheduler, event_router
        );

        Ok(Self { core, window, renderer, capabilities })
    }

    // iOS touch events → FLUI events (like Android)
    pub fn handle_touch(&mut self, touches: NSSet<UITouch>) {
        for touch in touches {
            let position = touch.location_in_view(self.window.view());
            let phase = touch.phase();

            match phase {
                UITouchPhaseBeegan => {
                    self.core.handle_pointer_down(
                        PointerId::new(touch.hash()),
                        Offset::new(position.x, position.y),
                        PointerDeviceKind::Touch,
                    );
                }
                UITouchPhaseMoved => {
                    self.core.handle_pointer_move(
                        PointerId::new(touch.hash()),
                        Offset::new(position.x, position.y),
                    );
                }
                // ...
            }
        }
    }
}
```

**iOS-Specific Work (6-8 weeks):**

1. **Platform Setup** (1 week)
   - `winit` iOS support or custom UIKit integration
   - Metal surface creation via wgpu
   - App lifecycle (AppDelegate, SceneDelegate)
   - View controller setup

2. **Touch Events** (1 week)
   - UITouch → FLUI PointerEvent conversion
   - Multi-touch gesture tracking
   - Force touch (3D Touch) support

3. **Keyboard & Text Input** (1-2 weeks)
   - UITextView integration
   - Keyboard appearance/dismissal
   - Text selection
   - Autocorrect/suggestions

4. **iOS-Specific Features** (2-3 weeks)
   - Safe area insets (notch, home indicator)
   - Dark mode support
   - Haptic feedback
   - System gestures (swipe back)
   - Status bar styling

5. **Platform Widgets** (1-2 weeks)
   - Cupertino-style widgets
   - Native navigation
   - Platform-specific scrolling physics

6. **Examples & Testing** (1 week)
   - Port examples to iOS
   - Device testing (iPhone, iPad)
   - Simulator testing

**iOS Roadmap:**
- Month 1: Platform setup + touch events + keyboard
- Month 2: iOS-specific features + widgets + examples

---

## Phase 3: Web/WASM Support

### Web Embedder (Current: 0/10 → Target: 9/10)

**Architecture:**

```rust
// flui-platform/src/platforms/web.rs
pub struct WebEmbedder {
    core: EmbedderCore,      // ✅ SHARED (90%)
    canvas: HtmlCanvasElement, // Web-specific
    renderer: GpuRenderer,   // ✅ SHARED (wgpu + WebGPU)
    event_listeners: EventListeners,
    capabilities: WebCapabilities,
}

impl WebEmbedder {
    pub async fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<Scheduler>,
        event_router: Arc<RwLock<EventRouter>>,
        canvas_id: &str,
    ) -> Result<Self> {
        // 1. Get HTML canvas
        let document = web_sys::window()?.document()?;
        let canvas = document
            .get_element_by_id(canvas_id)?
            .dyn_into::<HtmlCanvasElement>()?;

        // 2. Create WebGPU surface
        let renderer = GpuRenderer::new_async_with_canvas(
            &canvas, width, height
        ).await;

        // 3. Setup event listeners
        let listeners = EventListeners::new(&canvas);

        // 4. Create shared core (same as all platforms!)
        let core = EmbedderCore::new(
            pipeline_owner, needs_redraw, scheduler, event_router
        );

        Ok(Self { core, canvas, renderer, event_listeners, capabilities })
    }

    // Browser events → FLUI events
    fn setup_event_listeners(&mut self) {
        // Mouse events
        self.listeners.add("mousedown", |event: MouseEvent| {
            self.core.handle_pointer_down(
                PointerId::new(0),
                Offset::new(event.client_x(), event.client_y()),
                PointerDeviceKind::Mouse,
            );
        });

        // Touch events
        self.listeners.add("touchstart", |event: TouchEvent| {
            for touch in event.changed_touches() {
                self.core.handle_pointer_down(
                    PointerId::new(touch.identifier()),
                    Offset::new(touch.client_x(), touch.client_y()),
                    PointerDeviceKind::Touch,
                );
            }
        });

        // Keyboard events
        self.listeners.add("keydown", |event: KeyboardEvent| {
            let key_event = convert_keyboard_event(&event);
            self.core.handle_key_event(key_event);
        });
    }
}
```

**Web-Specific Work (6-8 weeks):**

1. **Platform Setup** (1 week)
   - Canvas setup
   - WebGPU initialization
   - WASM build configuration
   - HTML/CSS integration

2. **Event Handling** (1-2 weeks)
   - Mouse events (click, move, wheel)
   - Touch events (mobile browsers)
   - Keyboard events
   - Focus management
   - Pointer events API

3. **Browser Integration** (2-3 weeks)
   - Request Animation Frame loop
   - Page Visibility API
   - Resize handling (responsive)
   - Browser back/forward
   - URL routing

4. **Web-Specific Features** (1-2 weeks)
   - Accessibility (ARIA)
   - SEO considerations
   - Service workers (PWA)
   - Clipboard API
   - Local storage

5. **Performance** (1 week)
   - WASM optimization
   - Bundle size reduction
   - Code splitting
   - Lazy loading

6. **Examples & Testing** (1 week)
   - Web demos
   - Cross-browser testing
   - Mobile browser testing

**Web Roadmap:**
- Month 1: Platform setup + events + browser integration
- Month 2: Web features + performance + examples

---

## Shared Components (All Platforms)

### What's Already Shared (✅ DONE)

1. **EmbedderCore** (90% of logic)
   - Event routing via hit testing
   - Frame coordination
   - Scene caching
   - Pointer state tracking
   - Lifecycle management

2. **Event System**
   - PointerEvent (unified across platforms)
   - ScrollEventData
   - KeyEventData (types ready)
   - Event propagation control

3. **Physics System**
   - FrictionSimulation
   - SpringSimulation
   - GravitySimulation
   - ClampedSimulation

4. **Rendering**
   - wgpu (Desktop: DX12/Vulkan/Metal)
   - wgpu (Android: Vulkan)
   - wgpu (iOS: Metal)
   - wgpu (Web: WebGPU)

### What Needs Sharing (TODO)

1. **Keyboard Conversion** (1 week)
   ```rust
   // flui-platform/src/conversion/keyboard.rs

   #[cfg(target_os = "windows")]
   pub fn convert_keyboard_event(event: &winit::KeyEvent) -> KeyEvent {
       // winit → FLUI conversion
   }

   #[cfg(target_os = "android")]
   pub fn convert_keyboard_event(event: &winit::KeyEvent) -> KeyEvent {
       // Same implementation!
   }

   #[cfg(target_os = "ios")]
   pub fn convert_keyboard_event(event: &UIKeyEvent) -> KeyEvent {
       // iOS-specific but same output type
   }

   #[cfg(target_arch = "wasm32")]
   pub fn convert_keyboard_event(event: &web_sys::KeyboardEvent) -> KeyEvent {
       // Web-specific but same output type
   }
   ```

2. **Gesture Recognition** (shared logic)
   - TapGestureRecognizer ✅
   - DragGestureRecognizer ✅
   - ScaleGestureRecognizer ✅
   - Need widget-level API

3. **Scrollable Widget** (fully shared)
   ```rust
   // flui_widgets/src/scrolling/scrollable.rs
   // This widget works on ALL platforms!

   pub struct Scrollable {
       // Platform-agnostic implementation
       // Physics are same everywhere
       // Only platform-specific: overscroll indicators
   }
   ```

4. **Animation System**
   - AnimationController (shared)
   - Ticker (platform-specific timing)
   - Curves (shared)

---

## Implementation Strategy

### Code Organization

```
flui-platform/
├── src/
│   ├── core/
│   │   ├── embedder_core.rs      ✅ SHARED (all platforms)
│   │   ├── frame_coordinator.rs  ✅ SHARED
│   │   ├── scene_cache.rs        ✅ SHARED
│   │   └── pointer_state.rs      ✅ SHARED
│   │
│   ├── conversion/               ⚠️ NEW (shared conversions)
│   │   ├── keyboard.rs           ❌ TODO (winit/UIKit/web)
│   │   ├── mouse.rs              ✅ DONE
│   │   └── touch.rs              ⚠️ PARTIAL
│   │
│   ├── platforms/
│   │   ├── desktop.rs            ✅ 120 lines (thin wrapper)
│   │   ├── android.rs            ⚠️ 200 lines (needs work)
│   │   ├── ios.rs                ❌ Placeholder
│   │   └── web.rs                ❌ Placeholder
│   │
│   └── bindings/
│       ├── gesture_binding.rs    ✅ SHARED
│       └── scheduler_binding.rs  ✅ SHARED
```

### Testing Strategy

1. **Unit Tests** (platform-agnostic)
   - ✅ EmbedderCore: 9 tests
   - ✅ Hit testing: 198 tests
   - ✅ Physics: comprehensive
   - ❌ Keyboard conversion: TODO
   - ❌ Gesture widgets: TODO

2. **Integration Tests** (platform-specific)
   - ❌ Desktop: end-to-end examples
   - ❌ Android: device testing
   - ❌ iOS: simulator + device
   - ❌ Web: browser testing

3. **Manual Testing**
   - Examples app on each platform
   - Performance profiling
   - User experience testing

---

## Timeline & Priorities

### Phase 1: Desktop + Android Production (3-4 weeks)

**Priority: HIGH**

- Week 1: Keyboard events + basic examples (Desktop)
- Week 2: Scroll physics + drag widgets
- Week 3: Multi-touch + Android-specific
- Week 4: Polish + comprehensive examples

**Deliverable:** Desktop + Android at 9/10

### Phase 2: iOS Support (2 months)

**Priority: MEDIUM**

- Month 1: Platform setup + core events
- Month 2: iOS features + examples

**Deliverable:** iOS at 9/10

### Phase 3: Web Support (2 months)

**Priority: MEDIUM**

- Month 1: Platform setup + browser integration
- Month 2: Web features + optimization

**Deliverable:** Web at 9/10

---

## Success Metrics

### Desktop (Target: 9.5/10)
- ✅ Mouse, keyboard, scroll fully working
- ✅ Smooth 60fps rendering
- ✅ Examples demonstrate all features
- ✅ Zero unsafe code
- ✅ Hot reload support

### Android (Target: 9/10)
- ✅ Touch, multi-touch, gestures
- ✅ Soft keyboard + IME
- ✅ Smooth 60fps on mid-range devices
- ✅ Platform integration (back button, etc.)
- ✅ Examples on Play Store

### iOS (Target: 9/10)
- ✅ Touch, gestures, force touch
- ✅ Keyboard + text input
- ✅ 60fps on older devices
- ✅ Safe area handling
- ✅ Examples on App Store

### Web (Target: 8.5/10)
- ✅ Mouse, touch, keyboard
- ✅ 60fps in modern browsers
- ✅ Responsive design
- ✅ PWA capabilities
- ✅ Online demos

---

## Risk Mitigation

### Technical Risks

1. **wgpu WebGPU support not ready**
   - Mitigation: Fallback to WebGL2 via wgpu
   - Timeline impact: +1 week

2. **iOS Metal integration issues**
   - Mitigation: wgpu handles Metal, proven track record
   - Timeline impact: Minimal

3. **Performance on low-end Android**
   - Mitigation: GPU acceleration, efficient rendering
   - Timeline impact: +1 week optimization

### Process Risks

1. **Scope creep**
   - Mitigation: Stick to roadmap, defer non-critical features
   - Use feature flags for experimental features

2. **Platform API changes**
   - Mitigation: Abstract platform APIs behind traits
   - Keep platform code minimal (10%)

---

## Next Steps

### Immediate (This Week)

1. ✅ Review and approve this roadmap
2. ❌ Create GitHub issues for Phase 1 tasks
3. ❌ Start keyboard conversion module
4. ❌ Begin scroll physics integration

### Short Term (Month 1)

1. Complete Desktop + Android to 9/10
2. Create comprehensive examples
3. Document platform differences
4. Performance benchmarking

### Long Term (Months 2-4)

1. iOS platform implementation
2. Web platform implementation
3. Platform parity testing
4. Production deployments

---

## Conclusion

**FLUI's cross-platform strategy:**
- ✅ **90% shared code** via EmbedderCore (proven on Desktop)
- ✅ **10% platform adapters** (thin wrappers)
- ✅ **Type-safe** (zero unsafe in platform layer)
- ✅ **Consistent UX** across all platforms

**Current achievement:**
- Hit testing system: **9.8/10** ✅
- Platform architecture: **proven** ✅
- Physics system: **complete** ✅

**Remaining work:**
- Desktop polish: **2-3 weeks**
- Android completion: **3-4 weeks**
- iOS greenfield: **2 months**
- Web greenfield: **2 months**

**Total timeline to full cross-platform:** **~4-5 months**

With proper execution, FLUI will be a **world-class cross-platform UI framework** with **maximum code sharing** and **minimum platform hassles**.

---

*Document Version: 1.0*
*Date: 2025-12-07*
*Status: Draft for Review*

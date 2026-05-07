# Custom Render Callback — Design Document

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow developers to embed custom wgpu rendering (2D canvas, 3D scenes, shader effects, compute visualizations) as first-class widgets in the FLUI widget tree, with full layout participation, dirty tracking, and resource caching.

**Motivation:** Flutter forces developers to use painful workarounds (PlatformView, Texture widget, Flame) for custom GPU rendering. Since FLUI is built on wgpu, custom rendering should be native and zero-friction.

**Architecture:** Single `RenderCallback` trait with `RenderMode` enum (Flat/Scene). Three-phase lifecycle (init/update/render). Type-erased storage in engine. Offscreen render-to-texture with cross-frame caching.

---

## 1. Core Trait: RenderCallback

```rust
/// What kind of rendering context the callback needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// 2D rendering — color target only.
    /// No depth buffer, no MSAA. Zero overhead for simple 2D.
    /// Use for: canvas drawing, charts, custom widgets, 2D games.
    Flat,

    /// 3D rendering — color + depth buffer + optional MSAA.
    /// Full GPU pipeline with configurable sample count.
    /// Use for: 3D scenes, shader effects, compute visualizations.
    Scene { sample_count: u32 },
}

impl Default for RenderMode {
    fn default() -> Self { RenderMode::Flat }
}

/// How the callback's alpha output should be interpreted during compositing.
///
/// GPU rendering typically outputs premultiplied alpha. If your shader outputs
/// straight alpha (common in some 3D pipelines), set `Straight` to avoid
/// dark fringe artifacts at transparent edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaMode {
    /// Output is premultiplied alpha (rgb already multiplied by alpha).
    /// Compositor uses: src=One, dst=OneMinusSrcAlpha.
    /// This is the standard for GPU rendering. Use this unless you know otherwise.
    #[default]
    Premultiplied,
    /// Output is straight alpha (rgb NOT multiplied by alpha).
    /// Compositor uses: src=SrcAlpha, dst=OneMinusSrcAlpha.
    /// Or converts to premultiplied during compositing (rgb *= alpha).
    Straight,
}

/// Trait for custom GPU rendering within the FLUI widget tree.
///
/// Implement this to render any custom wgpu content inside a FLUI widget.
/// FLUI handles offscreen texture management, dirty tracking, layout integration,
/// and compositing automatically.
///
/// # Lifecycle
///
/// 1. `init()` — called once when widget first appears. Create pipelines, load assets.
/// 2. `update()` — called each frame. Update uniforms, check if re-render needed.
/// 3. `render()` — called only when dirty. Draw into pre-configured RenderContext.
///
/// # State Management
///
/// The `State` associated type is created in `init()`, owned by FLUI, and passed
/// back on each `update()` and `render()` call. It survives widget rebuilds —
/// analogous to Flutter's `State` surviving `Widget` rebuilds.
///
/// The `RenderCallback` implementor itself carries configuration (like a Flutter Widget).
/// It is updated on rebuild, while `State` persists.
pub trait RenderCallback: Send + Sync + 'static {
    /// Persistent state created during init, owned by FLUI.
    type State: Send + Sync + 'static;

    /// Declare what rendering context you need. Default: Flat (2D).
    fn mode(&self) -> RenderMode { RenderMode::Flat }

    /// Declare how alpha is encoded in your render output.
    /// Default: Premultiplied (standard for GPU rendering).
    /// Set to Straight if your 3D shader outputs non-premultiplied alpha.
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Premultiplied }

    /// Called once when the widget is first attached to the render tree.
    /// Create pipelines, load meshes, compile shaders, allocate buffers.
    ///
    /// `device` and `queue` are `Arc` — clone and store in State if you need
    /// them later (e.g., pipeline recreation, hot-reload, dynamic resources).
    ///
    /// `config` contains everything needed for pipeline creation:
    /// format, sample_count, depth_format, viewport_size.
    ///
    /// Return Err to show a placeholder widget instead of crashing.
    /// The error will be logged via tracing::error.
    fn init(
        &self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        config: RenderConfig,
    ) -> Result<Self::State, Box<dyn std::error::Error + Send + Sync>>;

    /// Handle input events directly in the callback (optional).
    ///
    /// Called before `update()` when input events target this widget.
    /// Return `true` to consume the event (widget won't receive it).
    /// Return `false` to let the event propagate to the widget system.
    ///
    /// Use this for 3D-specific interactions (orbit camera, object picking)
    /// instead of routing events through widget → config → callback.
    ///
    /// Default: no-op, all events go to widget system.
    fn on_input(
        &self,
        state: &mut Self::State,
        event: &InputEvent,
    ) -> bool { false }

    /// Called each frame before rendering. Update uniforms, camera, animations.
    ///
    /// Return `true` ONLY if the visual output changed. GPU buffer writes
    /// should happen here only when returning `true`. When returning `false`,
    /// the cached texture is reused — `render()` will NOT be called.
    ///
    /// For continuous animations, call `ctx.request_redraw()` to schedule
    /// the next frame. Without this, FLUI uses on-demand rendering and
    /// won't call update() again until an external event occurs.
    fn update(
        &self,
        state: &mut Self::State,
        ctx: &UpdateContext,
    ) -> bool;

    /// Called each frame when dirty. Render into the provided RenderContext.
    fn render(
        &self,
        state: &Self::State,
        ctx: &mut RenderContext,
    );
}
```

---

## 2. RenderContext — GPU context provided to callbacks

```rust
/// Configuration passed to init() — everything needed for pipeline creation.
pub struct RenderConfig {
    /// Surface color format (e.g. Bgra8UnormSrgb).
    pub format: wgpu::TextureFormat,
    /// MSAA sample count (1 = no MSAA, 4 = 4x MSAA).
    /// Use this when creating render pipelines.
    pub sample_count: u32,
    /// Depth buffer format. None for Flat mode, Some(Depth32Float) for Scene.
    /// Use this in pipeline DepthStencilState.
    pub depth_format: Option<wgpu::TextureFormat>,
    /// Initial viewport size in physical pixels.
    pub viewport_size: (u32, u32),
}

/// Input events delivered to on_input() callback.
///
/// Subset of FLUI input events relevant for custom rendering interaction.
pub enum InputEvent {
    /// Pointer button pressed at position (relative to widget).
    PointerDown { position: (f32, f32), button: PointerButton },
    /// Pointer moved to position (relative to widget).
    PointerMove { position: (f32, f32) },
    /// Pointer button released.
    PointerUp { button: PointerButton },
    /// Pointer dragged (delta since last move).
    PointerDrag { delta: (f32, f32), button: PointerButton },
    /// Scroll wheel.
    Scroll { delta: f32 },
    /// Pinch zoom gesture.
    PinchZoom { scale: f32 },
}

/// Frame-level data passed to update().
pub struct UpdateContext<'a> {
    /// wgpu queue for buffer writes during update.
    pub queue: &'a wgpu::Queue,
    /// Viewport size in physical pixels.
    pub viewport_size: (u32, u32),
    /// Time since last frame in seconds.
    ///
    /// **Warning:** With on-demand rendering (`ControlFlow::Wait`), this value
    /// can be very large (seconds or minutes) if no events occurred.
    /// For physics/animation, clamp it: `delta_time.min(1.0 / 30.0)`.
    pub delta_time: f32,
    /// Monotonic frame counter.
    pub frame_number: u64,
    /// Request another frame for continuous animation.
    ///
    /// Call this if your content is animated (rotating model, particle system).
    /// Without this call, FLUI uses on-demand rendering and will NOT schedule
    /// another frame automatically — your animation will freeze.
    ///
    /// Safe to call multiple times per frame (idempotent).
    pub request_redraw: &'a dyn Fn(),
}

/// Full GPU rendering context for custom content.
///
/// Provides pre-configured render targets with depth buffer (Scene mode)
/// and access to shared FLUI caches. Use `begin_render_pass()` for the
/// common case, or access raw device/encoder for advanced multi-pass.
pub struct RenderContext<'a> {
    // === Core wgpu access ===
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,

    // === Pre-configured targets ===
    color_target: &'a wgpu::TextureView,
    depth_target: Option<&'a wgpu::TextureView>,  // Some in Scene mode
    msaa_target: Option<&'a wgpu::TextureView>,    // Some if sample_count > 1

    // === Metadata ===
    viewport_size: (u32, u32),
    format: wgpu::TextureFormat,
    mode: RenderMode,

    // === Shared caches from FLUI engine ===
    pipeline_cache: &'a PipelineCache,
    texture_cache: &'a mut TextureCache,
}

impl<'a> RenderContext<'a> {
    /// Begin a render pass with pre-configured attachments.
    /// Depth attachment included automatically for Scene mode.
    /// Default clear: transparent black. Pass `Some(color)` to override.
    ///
    /// When MSAA is enabled (Scene mode, sample_count > 1):
    /// - Renders to the MSAA texture
    /// - Automatically resolves to the single-sample color target
    /// - resolve_target is set in the color attachment
    ///
    /// # Multi-pass rendering
    ///
    /// For advanced techniques (shadow maps, post-processing), drop the
    /// first render pass and create additional ones via `self.encoder()`:
    /// ```rust
    /// fn render(&self, state: &Self::State, ctx: &mut RenderContext) {
    ///     // Pass 1: shadow map (own texture, own depth)
    ///     { let pass = ctx.encoder().begin_render_pass(&shadow_desc); ... }
    ///     // Pass 2: main scene (framework's targets)
    ///     { let pass = ctx.begin_render_pass(Some(Color::BLACK)); ... }
    /// }
    /// ```
    pub fn begin_render_pass(
        &mut self,
        clear_color: Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'_> {
        let color = clear_color.unwrap_or(wgpu::Color::TRANSPARENT);
        let (view, resolve) = match self.msaa_target {
            Some(msaa) => (msaa, Some(self.color_target)),  // MSAA → resolve
            None => (self.color_target, None),               // Direct render
        };
        // ... create render pass with depth if available
    }

    /// Begin a compute pass for GPU-driven techniques.
    pub fn begin_compute_pass(&mut self) -> wgpu::ComputePass<'_> { ... }

    /// Raw device access for creating custom resources.
    pub fn device(&self) -> &wgpu::Device { self.device }
    pub fn queue(&self) -> &wgpu::Queue { self.queue }
    pub fn encoder(&mut self) -> &mut wgpu::CommandEncoder { self.encoder }

    /// Whether depth buffer is available (true in Scene mode).
    pub fn has_depth(&self) -> bool { self.depth_target.is_some() }

    /// Depth buffer view. Returns None in Flat mode.
    pub fn depth_view(&self) -> Option<&wgpu::TextureView> { self.depth_target }

    /// Current viewport size in physical pixels.
    pub fn size(&self) -> (u32, u32) { self.viewport_size }

    /// Target texture format.
    pub fn format(&self) -> wgpu::TextureFormat { self.format }

    /// GPU limits for platform-adaptive rendering (buffer sizes, texture dims, etc.)
    pub fn limits(&self) -> &wgpu::Limits { ... }

    /// GPU features available on this device.
    pub fn features(&self) -> wgpu::Features { ... }
}
```

---

## 3. Type Erasure — ErasedCallback

The renderer stores callbacks as trait objects. `CallbackWrapper<T>` erases the
associated `State` type while keeping callback + state together:

```rust
/// Type-erased callback interface used internally by the engine.
trait ErasedCallback: Send + Sync {
    fn mode(&self) -> RenderMode;
    fn update(&mut self, ctx: &UpdateContext) -> bool;
    fn render(&self, ctx: &mut RenderContext);
    /// Update the callback configuration on widget rebuild.
    /// Uses Any for type-safe downcasting internally.
    fn update_config_any(&mut self, config: Box<dyn Any + Send + Sync>);
    /// Handle input event. Returns true if consumed.
    fn on_input(&mut self, event: &InputEvent) -> bool;
    /// TypeId of the concrete RenderCallback impl (for type change detection).
    fn callback_type_id(&self) -> TypeId;
    /// Alpha encoding of render output (for correct compositing blend mode).
    fn alpha_mode(&self) -> AlphaMode;
}

/// Wraps a concrete RenderCallback with its State.
/// Callback = configuration (updated on rebuild).
/// State = persistent data (survives rebuilds).
struct CallbackWrapper<T: RenderCallback> {
    callback: T,
    state: T::State,
}

impl<T: RenderCallback> ErasedCallback for CallbackWrapper<T> {
    fn mode(&self) -> RenderMode {
        self.callback.mode()
    }

    fn update(&mut self, ctx: &UpdateContext) -> bool {
        self.callback.update(&mut self.state, ctx)
    }

    fn render(&self, ctx: &mut RenderContext) {
        self.callback.render(&self.state, ctx);
    }

    fn update_config_any(&mut self, config: Box<dyn Any + Send + Sync>) {
        if let Ok(typed) = config.downcast::<T>() {
            self.callback = *typed;
        }
    }

    fn callback_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn on_input(&mut self, event: &InputEvent) -> bool {
        self.callback.on_input(&mut self.state, event)
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.callback.alpha_mode()
    }
}
```

When the widget rebuilds with new configuration (e.g., `camera_angle` changed),
the engine replaces `callback` inside the wrapper while keeping `state` intact.
This requires a method:

```rust
impl<T: RenderCallback> CallbackWrapper<T> {
    fn update_callback(&mut self, new_callback: T) {
        self.callback = new_callback;
    }
}
```

---

## 4. Engine Storage — CallbackStateStore

```rust
/// Stores callback state for all active CustomRender widgets.
/// Lives in the Renderer, survives widget rebuilds.
pub struct CallbackStateStore {
    entries: HashMap<CallbackId, CallbackEntry>,
    active_this_frame: HashSet<CallbackId>,
}

struct CallbackEntry {
    /// Type-erased callback + state. None if init() failed.
    callback: Option<Box<dyn ErasedCallback>>,

    /// TypeId of the concrete RenderCallback impl.
    /// Used to detect type changes at the same position (conditional rendering).
    /// On type mismatch, the entry is fully reinitialized.
    callback_type_id: TypeId,

    /// Error message from init() failure (for placeholder rendering).
    init_error: Option<String>,

    /// Last RenderMode (detect mode changes at runtime, e.g. MSAA settings).
    /// On mismatch, full reinit is triggered (pipelines need recreation).
    last_mode: RenderMode,

    /// Cached render result (owned, lives across frames)
    cached_color: Option<wgpu::Texture>,
    cached_view: Option<wgpu::TextureView>,

    /// Depth buffer (Scene mode only, owned)
    depth_texture: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,

    /// MSAA resolve target (Scene mode with sample_count > 1)
    msaa_texture: Option<wgpu::Texture>,
    msaa_view: Option<wgpu::TextureView>,

    /// Last rendered size (detect resize → recreate textures)
    last_size: (u32, u32),
}

/// Stable identity for matching callback state across rebuilds.
/// Derived from ElementId in the element tree.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CallbackId(pub NonZeroUsize);

impl CallbackStateStore {
    pub fn new() -> Self { ... }

    /// Get or initialize a callback entry.
    ///
    /// - If entry exists with same TypeId, returns it.
    /// - If entry exists with different TypeId, reinitializes (type changed).
    /// - If entry doesn't exist, calls init_fn to create it.
    /// - If init_fn returns Err, creates entry with error placeholder.
    pub fn get_or_init(
        &mut self,
        id: CallbackId,
        type_id: TypeId,
        mode: RenderMode,
        init_fn: impl FnOnce() -> Result<Box<dyn ErasedCallback>, Box<dyn Error>>,
    ) -> &mut CallbackEntry {
        self.active_this_frame.insert(id);

        // Check for type mismatch (conditional rendering changed callback type)
        // or mode change (user changed MSAA settings at runtime)
        if let Some(entry) = self.entries.get(&id) {
            let needs_reinit = entry.callback_type_id != type_id
                || entry.last_mode != mode;
            if needs_reinit {
                self.entries.remove(&id); // Force full reinit
            }
        }

        self.entries.entry(id).or_insert_with(|| {
            match init_fn() {
                Ok(callback) => CallbackEntry {
                    callback: Some(callback),
                    callback_type_id: type_id,
                    init_error: None,
                    last_mode: mode,
                    cached_color: None, cached_view: None,
                    depth_texture: None, depth_view: None,
                    msaa_texture: None, msaa_view: None,
                    last_size: (0, 0),
                },
                Err(e) => {
                    tracing::error!("CustomRender init failed: {}", e);
                    CallbackEntry {
                        callback: None,
                        callback_type_id: type_id,
                        init_error: Some(e.to_string()),
                        last_mode: mode,
                        cached_color: None, cached_view: None,
                        depth_texture: None, depth_view: None,
                        msaa_texture: None, msaa_view: None,
                        last_size: (0, 0),
                    }
                }
            }
        })
    }

    /// Mark a callback as active this frame.
    pub fn mark_active(&mut self, id: CallbackId) {
        self.active_this_frame.insert(id);
    }

    /// Remove entries for callbacks that were not referenced this frame.
    /// Call at end of render_scene() to free GPU resources for removed widgets.
    pub fn gc(&mut self) {
        self.entries.retain(|id, _| self.active_this_frame.contains(id));
        self.active_this_frame.clear();
    }
}
```

---

## 5. Layer Integration — CustomRenderLayer

```rust
// === In flui-layer ===

/// New Layer variant for custom GPU rendering.
pub struct CustomRenderLayer {
    /// Bounds in parent coordinate space (from layout result).
    bounds: Rect<Pixels>,
    /// Stable ID matching CallbackStateStore entry.
    callback_id: CallbackId,
    /// Rendering mode (determines depth/MSAA texture creation).
    mode: RenderMode,
}

// === In flui-engine, render_layer_recursive() ===

Layer::CustomRender(layer) => {
    // 1. Occlusion check
    let b = layer.bounds();
    if occlusion.is_occluded(b.left().0, b.top().0, b.width().0, b.height().0) {
        return;
    }

    // 2. Get or init callback entry
    let entry = self.callback_store.get_or_init(layer.callback_id, || {
        // init() called here on first frame
        create_erased_callback(...)
    });

    let size = (b.width().0 as u32, b.height().0 as u32);

    // 3. Recreate textures on resize
    if entry.last_size != size && size.0 > 0 && size.1 > 0 {
        entry.cached_color = None;
        entry.cached_view = None;

        // Depth buffer for Scene mode
        if matches!(layer.mode, RenderMode::Scene { .. }) {
            let depth = device.create_texture(&TextureDescriptor {
                label: Some("custom_render_depth"),
                size: Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                format: TextureFormat::Depth32Float,
                sample_count: layer.mode.sample_count(),
                ..
            });
            entry.depth_view = Some(depth.create_view(&Default::default()));
            entry.depth_texture = Some(depth);
        }

        // MSAA for Scene mode with sample_count > 1
        if layer.mode.sample_count() > 1 {
            // create MSAA texture ...
        }

        entry.last_size = size;
    }

    // 4. Update → dirty check
    let update_ctx = UpdateContext {
        queue: &self.queue,
        viewport_size: size,
        delta_time: 0.016, // from frame timing
        frame_number: self.frame_number,
    };
    let dirty = entry.callback.update(&update_ctx);

    // 5. Ensure color texture exists (create once, reuse across frames)
    if entry.cached_color.is_none() {
        let color_tex = device.create_texture(&TextureDescriptor {
            label: Some("custom_render_color"),
            size: Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            format: surface_format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            sample_count: 1,
            ..
        });
        entry.cached_view = Some(color_tex.create_view(&Default::default()));
        entry.cached_color = Some(color_tex);
    }

    // 6. Render only if dirty
    if dirty {
        let color_view = entry.cached_view.as_ref().unwrap();

        // Build RenderContext
        let mut ctx = RenderContext {
            device: &self.device,
            queue: &self.queue,
            encoder: &mut encoder,
            color_target: color_view,
            depth_target: entry.depth_view.as_ref(),
            msaa_target: entry.msaa_view.as_ref(),
            viewport_size: size,
            format: surface_format,
            mode: layer.mode,
            pipeline_cache: &self.pipeline_cache,
            texture_cache: &mut self.texture_cache,
        };

        let start = std::time::Instant::now();
        entry.callback.render(&mut ctx);
        let elapsed = start.elapsed();
        if elapsed > std::time::Duration::from_millis(16) {
            tracing::warn!(
                callback_id = ?layer.callback_id,
                elapsed_ms = elapsed.as_millis(),
                "CustomRender callback exceeded frame budget (16ms)"
            );
        }
    }

    // 7. Composite into main surface (respecting alpha mode)
    if let Some(ref callback) = entry.callback {
        if let Some(view) = &entry.cached_view {
            match callback.alpha_mode() {
                AlphaMode::Premultiplied => {
                    // src=One, dst=OneMinusSrcAlpha (standard)
                    painter.draw_texture_rect(view, layer.bounds);
                }
                AlphaMode::Straight => {
                    // src=SrcAlpha, dst=OneMinusSrcAlpha
                    // Or: convert to premultiplied in compositing shader (rgb *= a)
                    painter.draw_texture_rect_straight_alpha(view, layer.bounds);
                }
            }
        }
    } else if let Some(error) = &entry.init_error {
        // Render placeholder for failed init
        painter.draw_error_placeholder(layer.bounds, error);
    }

    // 7. Register as opaque (3D scenes are typically opaque)
    if matches!(layer.mode, RenderMode::Scene { .. }) {
        occlusion.add_opaque(b.left().0, b.top().0, b.width().0, b.height().0);
    }
}

// After full tree traversal:
self.callback_store.gc();  // cleanup removed widgets
```

---

## 6. Widget API — CustomRender

```rust
/// Widget that renders custom wgpu content (2D or 3D) in the UI tree.
///
/// Participates in layout like any other widget. Renders to an offscreen
/// texture that is composited into the surface at the correct Z-position.
///
/// # Examples
///
/// ## 3D Scene
/// ```rust
/// CustomRender::new(MyScene {
///     model: "assets/car.glb".into(),
///     camera_angle: self.angle,
/// })
/// .width(Length::Fill)
/// .height(px(400.0))
/// ```
///
/// ## 2D Chart
/// ```rust
/// CustomRender::new(LineChart {
///     data: self.prices.clone(),
///     color: Color::BLUE,
/// })
/// .width(px(300.0))
/// .height(px(200.0))
/// ```
pub struct CustomRender<T: RenderCallback> {
    callback: T,
    width: Length,
    height: Length,
}

impl<T: RenderCallback> CustomRender<T> {
    pub fn new(callback: T) -> Self {
        Self {
            callback,
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}
```

---

## 7. Data Flow Summary

```
Widget rebuild:
  CustomRender<MyScene> created with new config
  ↓
Element tree diff:
  Same callback_id → update callback in CallbackWrapper, keep State
  New callback_id → call init(), create CallbackWrapper
  ↓
Layout phase:
  CustomRender.layout(constraints) → Size
  ↓
Paint phase:
  Push CustomRenderLayer { bounds, callback_id, mode }
  ↓
Render phase (render_layer_recursive):
  1. Occlusion check → skip if behind opaque layer
  2. Resize textures if bounds changed
  3. callback.update() → dirty flag
  4. If dirty → callback.render() into offscreen texture
  5. If not dirty → reuse cached texture
  6. Composite texture into main surface
  ↓
Frame end:
  callback_store.gc() → free removed widget resources
```

---

## 8. Key Design Properties

| Property | How achieved |
|----------|-------------|
| **Zero-friction 3D** | One trait, one widget, pre-configured depth/MSAA |
| **2D + 3D unified** | RenderMode enum, same trait, same widget |
| **No per-frame waste** | Dirty flag → skip render, reuse cached texture |
| **Layout integration** | Standard constraints, flexible sizing |
| **Resource sharing** | Shared device/queue/pipeline_cache/texture_cache |
| **Lifecycle clarity** | init (once) → update (each frame) → render (if dirty) |
| **Memory safety** | GC cleanup when widget removed from tree |
| **Occlusion culling** | OcclusionTracker skips hidden 3D widgets |
| **Damage tracking** | DamageTracker integration via dirty flag |
| **Type safety** | Associated type State, no Any downcasting in user code |

---

## 9. Callback → Widget Communication Pattern

CustomRender callbacks may need to send data back to the widget tree
(e.g., object picking, measurement results, collision detection).
Since `RenderCallback` has no direct output channel, use `flume` channel:

```rust
use flume::{Sender, Receiver};

// === Events from callback to widget ===
enum ViewportEvent {
    ObjectPicked(ObjectId),
    SelectionChanged(Vec<ObjectId>),
    CameraChanged(CameraState),
}

// === Callback config carries the sender ===
struct SceneViewport {
    scene: Arc<Scene>,
    event_sender: Sender<ViewportEvent>,
}

impl RenderCallback for SceneViewport {
    type State = ViewportState;

    fn on_input(&self, state: &mut ViewportState, event: &InputEvent) -> bool {
        match event {
            InputEvent::PointerDown { position, .. } => {
                if let Some(object) = state.raycast(*position) {
                    self.event_sender.send(ViewportEvent::ObjectPicked(object)).ok();
                }
                true
            }
            InputEvent::PointerDrag { delta, .. } => {
                state.camera.orbit(*delta);
                self.event_sender.send(ViewportEvent::CameraChanged(
                    state.camera.state()
                )).ok();
                true
            }
            _ => false,
        }
    }

    fn update(&self, state: &mut ViewportState, ctx: &UpdateContext) -> bool {
        ctx.request_redraw();  // continuous orbit animation
        true
    }

    // ...
}

// === Widget polls the receiver ===
struct EditorView {
    event_rx: Receiver<ViewportEvent>,
    event_tx: Sender<ViewportEvent>,
    selected: Option<ObjectId>,
    scene: Arc<Scene>,
}

impl EditorView {
    fn build(&mut self) -> impl View {
        // Poll events from callback
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ViewportEvent::ObjectPicked(id) => self.selected = Some(id),
                _ => {}
            }
        }

        Row::new()
            .child(
                CustomRender::new(SceneViewport {
                    scene: self.scene.clone(),
                    event_sender: self.event_tx.clone(),
                })
                .width(Length::FillPortion(3))
                .height(Length::Fill)
            )
            .child(
                PropertiesPanel::new(self.selected)  // shows selected object
                    .width(Length::FillPortion(1))
            )
    }
}
```

This pattern works with the current trait design — no framework changes needed.
The `flume` crate is already a FLUI dependency.

---

## 10. Roadmap: Phase 2 and Phase 3

All high-level widgets are built ON TOP of `CustomRender` (Phase 1), not as
separate systems. Each implements `RenderCallback` internally.

### Phase 2: High-Level 3D Widgets (crate: `flui-3d`)

```
flui-3d/
├── model_viewer.rs      — glTF model viewer widget
├── canvas_3d.rs         — programmatic 3D drawing API
├── gltf/
│   ├── loader.rs        — async glTF 2.0 loading (gltf crate)
│   ├── mesh.rs          — GPU mesh buffers from glTF primitives
│   ├── material.rs      — PBR metallic-roughness materials
│   ├── animation.rs     — skeletal animation playback + blending
│   └── skin.rs          — skinning (bone matrices)
├── render/
│   ├── pbr_pipeline.rs  — PBR fragment shader + pipeline
│   ├── ibl.rs           — image-based lighting (environment maps)
│   ├── shadow.rs        — shadow mapping (directional + spot)
│   └── skybox.rs        — skybox/environment rendering
├── camera/
│   ├── orbit.rs         — orbit camera controller
│   ├── fly.rs           — fly-through camera
│   └── projection.rs    — perspective + orthographic
└── helpers/
    ├── pipeline_builder.rs — reduce pipeline creation boilerplate
    └── mesh_builder.rs     — procedural geometry (cube, sphere, plane)
```

**Target API for 3D modeler (Артём):**

```rust
// Simple model viewer — 5 lines instead of 420
ModelViewer::new("assets/shoe.glb")
    .environment("assets/studio.hdr")    // HDR environment map
    .auto_rotate(true)                    // slow turntable
    .orbit_enabled(true)                  // mouse orbit
    .animation("unfold")                  // play glTF animation
    .animation_progress(self.progress)    // slider control
    .background(Color::from_hex("#f5f5f5"))
    .width(Length::Fill)
    .height(px(500.0))
```

**Target API for programmer:**

```rust
// Programmatic 3D — draw API like Canvas but 3D
Canvas3D::new(|ctx| {
    ctx.set_camera(Camera::perspective(60.0, 0.1, 100.0));
    ctx.draw_grid(10.0, 10);
    ctx.draw_mesh(&my_mesh, Transform::rotate_y(angle));
    ctx.draw_line_3d(start, end, Color::RED);
    ctx.draw_wireframe(&debug_mesh, Color::GREEN);
})
.orbit_camera(true)
.width(Length::Fill)
.height(px(400.0))
```

### Phase 3: Editor Toolkit (crate: `flui-editor`)

```
flui-editor/
├── gizmo/
│   ├── translate.rs     — translation handles (arrows)
│   ├── rotate.rs        — rotation handles (rings)
│   ├── scale.rs         — scale handles (cubes)
│   └── transform.rs     — combined gizmo with mode switching
├── viewport/
│   ├── scene_view.rs    — editable 3D viewport widget
│   ├── multi_viewport.rs — synchronized multi-viewport layout
│   └── grid.rs          — configurable grid overlay
├── selection/
│   ├── raycast.rs       — GPU/CPU raycasting
│   ├── box_select.rs    — rectangular selection
│   └── highlight.rs     — selection outline rendering
└── tools/
    ├── measure.rs       — distance/angle measurement
    └── snap.rs          — grid/vertex/edge snapping
```

**Target API for level designer (Дина):**

```rust
// Editor viewport with gizmo and picking
SceneView::new(&self.scene)
    .camera(self.editor_camera.clone())
    .gizmo(GizmoMode::Translate)           // translation arrows
    .grid(Grid::new(1.0).subdivisions(10)) // 1m grid with 10 subdivisions
    .selection(&self.selected_objects)       // highlight selected
    .on_object_picked(|id| msg::SelectObject(id))
    .on_transform_changed(|id, transform| msg::UpdateTransform(id, transform))
    .width(Length::Fill)
    .height(Length::Fill)

// Multi-viewport layout
MultiViewport::quad()  // 2x2 grid
    .scene(&self.scene)
    .top_left(ViewportCamera::Perspective(self.camera.clone()))
    .top_right(ViewportCamera::Front)
    .bottom_left(ViewportCamera::Top)
    .bottom_right(ViewportCamera::Side)
    .synchronized(true)  // selection syncs across viewports
```

### Phase dependency chain

```
Phase 1: CustomRender + RenderCallback trait     ← THIS DOCUMENT
    ↓
Phase 2: flui-3d (ModelViewer, Canvas3D)          ← glTF + PBR + camera
    ↓
Phase 3: flui-editor (SceneView, Gizmo, Selection) ← editor tools
```

Each phase is independently useful. Phase 1 alone unblocks ALL custom GPU rendering.
Phase 2 makes 3D accessible to non-graphics-programmers.
Phase 3 enables tool/editor development.

---

## 11. Files to Create/Modify (Phase 1)

| File | Action | Content |
|------|--------|---------|
| `crates/flui-engine/src/callback.rs` | Create | `RenderCallback` trait, `RenderMode`, `AlphaMode`, `RenderConfig`, `InputEvent`, `UpdateContext`, `RenderContext` |
| `crates/flui-engine/src/callback_store.rs` | Create | `CallbackStateStore`, `CallbackEntry`, `CallbackId`, `ErasedCallback`, `CallbackWrapper` |
| `crates/flui-engine/src/wgpu/renderer.rs` | Modify | Add `callback_store` field, handle `CustomRenderLayer` in traversal, GC, mode change detection |
| `crates/flui-layer/src/layer/custom_render.rs` | Create | `CustomRenderLayer` struct |
| `crates/flui-layer/src/layer/mod.rs` | Modify | Add `CustomRender` variant to `Layer` enum |
| `crates/flui-engine/src/lib.rs` | Modify | Re-export callback API |

**Estimated scope:** ~1000-1500 lines of new code across 4 new files + 2 modifications.

---

## 11. Review Findings (Post-Design Review)

Issues found during architectural review, all addressed in the document above:

### Fixed in this document

1. **`update_config_any()` added to `ErasedCallback`** — enables widget rebuild to update
   callback configuration without losing persistent State. Uses `Box<dyn Any>` for internal
   type-safe downcasting (section 3).

2. **Color texture reuse across frames** — no longer recreated on every dirty frame. Texture
   is created once and reused; `LoadOp::Clear` handles cleanup. Only recreated on resize
   (section 5, step 5).

3. **MSAA resolve step documented** — `begin_render_pass()` now shows how MSAA target is used
   as render view with `resolve_target` pointing to single-sample color texture (section 2).

4. **`init()` returns `Result`** — graceful failure with placeholder widget instead of panic
   on shader compilation failure or asset loading error (section 1).

5. **Multi-pass rendering pattern documented** — shows how callbacks can create multiple render
   passes via `ctx.encoder()` for shadow maps, post-processing, etc. (section 2).

### Fixed in review round 2 (self-interrogation)

6. **`request_redraw()` added to UpdateContext** — without it, animated 3D content freezes
   in on-demand rendering mode (`ControlFlow::Wait`). Idempotent, safe to call multiple times.

7. **`Arc<Device>` / `Arc<Queue>` in init()** — developer can clone and store in State for
   later use (pipeline recreation, hot-reload, dynamic resource creation). Zero overhead
   (Arc clone = atomic increment).

8. **`limits()` / `features()` on RenderContext** — for platform-adaptive rendering
   (WebGPU has stricter limits than Vulkan/Metal).

9. **`TypeId` check on rebuild** — detects callback type change at same tree position
   (conditional rendering). On mismatch, entry is fully reinitialized instead of silent
   downcast failure.

10. **`init_error` in CallbackEntry** — when init() returns Err, entry stores error message
    and renders placeholder instead of crashing. Error logged via `tracing::error`.

11. **`delta_time` warning documented** — can be very large with on-demand rendering.
    Developer should clamp for physics: `delta_time.min(1.0 / 30.0)`.

### Fixed in review round 3 (deep architectural questions)

12. **`AlphaMode` enum added** — callbacks declare whether output is premultiplied or straight
    alpha. Compositor selects correct blend mode. Without this, straight-alpha 3D content gets
    dark fringe artifacts at transparent edges. (sections 1, 3, 5)

13. **Performance warning on slow callbacks** — `tracing::warn` emitted if `render()` exceeds
    16ms frame budget. Includes callback_id and elapsed time for debugging. (section 5)

14. **Error placeholder rendering** — when `entry.callback` is None (init failed), compositor
    draws error placeholder with the error message instead of silently skipping. (section 5)

### Fixed in review round 4 (developer perspective)

15. **`RenderConfig` replaces `TextureFormat` in init()** — provides format, sample_count,
    depth_format, viewport_size. Developer can create correct pipelines from day one without
    guessing MSAA or depth settings. (sections 1, 2)

16. **`last_mode` check in CallbackStateStore** — detects RenderMode changes at runtime
    (e.g., user changes MSAA quality in settings). Triggers full reinit with new config
    so pipelines are recreated with correct sample_count. (section 4)

17. **`on_input()` optional method on RenderCallback** — direct input handling for 3D
    interactions (orbit camera, object picking). Returns bool to consume event. Avoids
    verbose widget → config → callback round-trip for common interactions. (section 1)

18. **`InputEvent` enum** — PointerDown/Move/Up/Drag, Scroll, PinchZoom. Positions relative
    to widget bounds. Subset of FLUI events relevant for custom rendering. (section 2)

### Fixed in review round 5 (user persona interviews)

19. **Callback → widget communication pattern** — documented `flume` channel pattern
    for sending data (object picking, selection, camera state) from callback back to
    the widget tree. No trait changes needed — uses existing `flume` dependency. (section 9)

20. **Phase 2 scope defined** — `flui-3d` crate with `ModelViewer` (glTF + PBR + orbit
    camera + animation), `Canvas3D` (programmatic 3D drawing), and supporting infrastructure
    (pipeline builder, mesh builder, camera controllers). Concrete API examples. (section 10)

21. **Phase 3 scope defined** — `flui-editor` crate with `SceneView`, `TransformGizmo`,
    `MultiViewport`, raycast selection, grid, snapping. Concrete API examples. (section 10)

22. **Phase dependency chain documented** — Phase 1 (CustomRender) → Phase 2 (flui-3d) →
    Phase 3 (flui-editor). Each independently useful. (section 10)

### Deferred to future phases

- **Phase 2: PipelineBuilder / MeshBuilder** — utility helpers to reduce init() boilerplate
- **Phase 2: ModelViewer** — glTF viewer with PBR, IBL, orbit camera, animation
- **Phase 2: Canvas3D** — programmatic 3D draw API (draw_mesh, draw_line_3d, draw_grid)
- **Phase 2: rasterize_text()** — render text to texture for use in 3D scenes
- **Phase 3: TransformGizmo** — translation/rotation/scale handles
- **Phase 3: SceneView** — editable 3D viewport with picking and selection
- **Phase 3: MultiViewport** — synchronized multi-viewport layout
- **capture()** — screenshot export of custom render content
- **GPU timestamp profiling** — performance measurement helpers in RenderContext

### Verified correct (no changes needed)

- **Opacity compositing stack** — CustomRender inside OpacityLayer works correctly.
  3D renders at full alpha to offscreen, then group opacity applied during compositing.
- **Resize during animation** — old texture dropped before new one created, no stale compositing.
- **Resource sharing** — `Arc<T>` for shared meshes/textures between callbacks. No framework support needed.
- **Compute-only in update()** — developer creates own encoder via stored `Arc<Device>`, returns false.
- **Async asset loading** — `tokio::spawn` in init(), poll `JoinHandle` in update().
- **Depth precision** — `begin_render_pass` clears depth to 1.0 (standard Z). For reverse-Z, use raw `encoder()`.

### Accepted trade-offs

- **Per-widget depth texture** — each Scene-mode widget gets its own depth buffer (~8MB at
  1080p). Sharing across same-size widgets is a future optimization.
- **Memory cost** — Scene mode with 4x MSAA at 1080p = ~48MB per widget. Logged via
  `tracing::info` on creation. Developer's responsibility to manage.
- **GC removes state on widget removal** — tab-based keep-alive is a higher-level concern,
  handled by keeping the widget in the tree (hidden, not removed).
- **Input events via widget system** — 3D interaction (orbit camera, picking) is user-level
  code. Widget handles mouse events, updates config fields, callback reads them.
- **CustomRender is a leaf widget** — no FLUI children inside custom render content.
  For overlay UI over 3D, use `Stack` with `CustomRender` + `Positioned` children.
- **Canvas vs CustomRender** — for simple custom 2D shapes (bezier, custom path), use
  the existing Canvas/DisplayList API. CustomRender is for raw wgpu access (3D, compute,
  custom shaders that Canvas doesn't support).
- **HDR rendering** — framework's offscreen texture is LDR (surface format). For HDR,
  create own Rgba16Float texture via `ctx.encoder()`, tonemap in a second pass to the
  framework's color target.

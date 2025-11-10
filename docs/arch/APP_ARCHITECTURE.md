# FLUI App Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the complete architecture for FLUI's application framework (`flui_app`), based on Flutter's embedder, binding, and platform channel patterns. The system provides **cross-platform application hosting** with **native platform integration**.

**Key Design Principles:**
1. **Cross-Platform by Default**: Single codebase runs on Windows, Linux, macOS, iOS, Android, Web
2. **Binding System**: Layered mixins (like Flutter) for Services, Scheduler, Gesture, Renderer, Widgets
3. **Platform Channels**: MethodChannel, EventChannel for native communication
4. **Embedder Pattern**: Platform-specific hosts (Win32, GTK, Cocoa, etc.)
5. **runApp() API**: Flutter-compatible app initialization
6. **Zero-Cost Abstraction**: Platform code only compiled on target platform

**Architecture Overview:**
```
┌─────────────────────────────────────────────────────────────┐
│                     User Application                        │
│  fn main() {                                                │
│      runApp(MyApp::new())                                   │
│  }                                                          │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                       flui_app                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  App, runApp(), WidgetsFlutterBinding                │   │
│  │  Platform Channels (MethodChannel, EventChannel)     │   │
│  │  Binding Mixins (Services, Scheduler, Gesture, etc.) │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│             Platform Embedders (per-platform)               │
│  ┌──────────┬──────────┬──────────┬──────────┬─────────┐  │
│  │ Windows  │  Linux   │  macOS   │ Android  │   iOS   │  │
│  │ (Win32)  │  (GTK)   │ (Cocoa)  │ (JNI)    │ (UIKit) │  │
│  └──────────┴──────────┴──────────┴──────────┴─────────┘  │
└─────────────────────────────────────────────────────────────┘
```

**Total Work Estimate:** ~3,500 LOC (core ~800 + bindings ~1,200 + embedders ~1,500)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Binding System](#binding-system)
3. [runApp() and App Initialization](#runapp-and-app-initialization)
4. [Platform Channels](#platform-channels)
5. [Platform Embedders](#platform-embedders)
6. [Window Management](#window-management)
7. [Event Loop Integration](#event-loop-integration)
8. [Plugin System](#plugin-system)
9. [Implementation Plan](#implementation-plan)
10. [Usage Examples](#usage-examples)
11. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### Three-Layer System

```text
┌─────────────────────────────────────────────────────────────┐
│                    Framework Layer                          │
│  (flui_app, flui_widgets, flui_core)                        │
│  • App initialization (runApp)                              │
│  • Widget tree management                                   │
│  • State management                                         │
│  • Platform channels                                        │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                     Engine Layer                            │
│  (flui_engine, flui_rendering)                              │
│  • Rendering pipeline (wgpu)                                │
│  • Layout engine                                            │
│  • Gesture recognition                                      │
│  • Animation system                                         │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                    Embedder Layer                           │
│  (platform-specific in flui_app)                            │
│  • Window creation                                          │
│  • Event loop                                               │
│  • Input handling                                           │
│  • Platform services                                        │
└─────────────────────────────────────────────────────────────┘
```

### Initialization Flow

```text
main()
  ↓
runApp(root_widget)
  ↓
WidgetsFlutterBinding::ensure_initialized()
  ↓
Initialize Bindings (layered):
  1. GestureBinding (hit testing)
  2. SchedulerBinding (frame scheduling)
  3. ServicesBinding (platform plugins)
  4. PaintingBinding (image cache)
  5. SemanticsBinding (accessibility)
  6. RendererBinding (render tree)
  7. WidgetsBinding (widget tree)
  ↓
Create Platform Embedder (Windows/Linux/macOS/etc.)
  ↓
Create Window
  ↓
Attach Root Widget
  ↓
Schedule First Frame
  ↓
Run Event Loop
```

---

## Binding System

### BindingBase (Foundation)

```rust
// In flui_app/src/binding/base.rs

/// Base class for all bindings
///
/// Inspired by Flutter's BindingBase, this provides the foundation
/// for the binding mixin system.
pub trait BindingBase: Send + Sync {
    /// Initialize this binding
    fn init_instances(&mut self);

    /// Lock bindings (prevent further initialization)
    fn lock_events(&self);

    /// Unlock bindings
    fn unlock_events(&self);

    /// Handle memory pressure
    fn handle_memory_pressure(&self) {
        // Default: do nothing
    }
}

/// Global binding instance
static BINDING: OnceLock<Arc<Mutex<Box<dyn BindingBase>>>> = OnceLock::new();

pub fn current_binding() -> Arc<Mutex<Box<dyn BindingBase>>> {
    BINDING
        .get()
        .expect("Binding not initialized. Call WidgetsFlutterBinding::ensure_initialized() first")
        .clone()
}

pub fn set_binding(binding: Box<dyn BindingBase>) {
    BINDING.set(Arc::new(Mutex::new(binding))).ok();
}
```

### GestureBinding

```rust
// In flui_app/src/binding/gesture.rs

/// Binding for gesture recognition
///
/// Provides hit testing and pointer event routing.
pub struct GestureBinding {
    pointer_router: Arc<PointerRouter>,
    hit_test_result: Arc<Mutex<Option<HitTestResult>>>,
}

impl GestureBinding {
    pub fn new() -> Self {
        Self {
            pointer_router: Arc::new(PointerRouter::new()),
            hit_test_result: Arc::new(Mutex::new(None)),
        }
    }

    /// Handle pointer event from platform
    pub fn handle_pointer_event(&self, event: PointerEvent) {
        match event {
            PointerEvent::Down(ref down) => {
                // Perform hit testing
                let mut result = HitTestResult::new();
                if let Some(root) = self.render_view() {
                    root.hit_test(&mut result, down.position);
                }
                *self.hit_test_result.lock() = Some(result);
            }
            _ => {}
        }

        // Route event to gesture recognizers
        self.pointer_router.route(&event);
    }

    pub fn pointer_router(&self) -> Arc<PointerRouter> {
        self.pointer_router.clone()
    }

    fn render_view(&self) -> Option<Arc<RenderView>> {
        // Get render view from RendererBinding
        None // TODO: Implement
    }
}

impl BindingBase for GestureBinding {
    fn init_instances(&mut self) {
        tracing::debug!("GestureBinding initialized");
    }

    fn lock_events(&self) {
        // Lock pointer router
    }

    fn unlock_events(&self) {
        // Unlock pointer router
    }
}
```

### SchedulerBinding

```rust
// In flui_app/src/binding/scheduler.rs

/// Binding for frame scheduling
///
/// Provides frame callbacks and vsync synchronization.
pub struct SchedulerBinding {
    frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,
    post_frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,
    frame_scheduler: Arc<FrameScheduler>,
    vsync_enabled: AtomicBool,
}

pub type FrameCallback = Arc<dyn Fn(Duration) + Send + Sync>;

impl SchedulerBinding {
    pub fn new() -> Self {
        Self {
            frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            frame_scheduler: Arc::new(FrameScheduler::new()),
            vsync_enabled: AtomicBool::new(true),
        }
    }

    /// Schedule a frame
    pub fn schedule_frame(&self) {
        if !self.vsync_enabled.load(Ordering::Relaxed) {
            return;
        }

        self.frame_scheduler.request_frame();
    }

    /// Add a persistent frame callback
    pub fn add_persistent_frame_callback(&self, callback: FrameCallback) {
        self.frame_callbacks.lock().push(callback);
    }

    /// Add a one-time post-frame callback
    pub fn add_post_frame_callback(&self, callback: FrameCallback) {
        self.post_frame_callbacks.lock().push(callback);
    }

    /// Handle frame (called by embedder)
    pub fn handle_begin_frame(&self, timestamp: Duration) {
        // Call persistent callbacks
        let callbacks = self.frame_callbacks.lock().clone();
        for callback in callbacks {
            callback(timestamp);
        }
    }

    pub fn handle_draw_frame(&self) {
        // Call post-frame callbacks
        let callbacks = std::mem::take(&mut *self.post_frame_callbacks.lock());
        for callback in callbacks {
            callback(Duration::ZERO);
        }
    }

    pub fn frame_scheduler(&self) -> Arc<FrameScheduler> {
        self.frame_scheduler.clone()
    }
}

impl BindingBase for SchedulerBinding {
    fn init_instances(&mut self) {
        tracing::debug!("SchedulerBinding initialized");
    }

    fn lock_events(&self) {
        self.vsync_enabled.store(false, Ordering::Relaxed);
    }

    fn unlock_events(&self) {
        self.vsync_enabled.store(true, Ordering::Relaxed);
    }
}
```

### ServicesBinding

```rust
// In flui_app/src/binding/services.rs

/// Binding for platform services
///
/// Provides access to platform channels and plugins.
pub struct ServicesBinding {
    method_channels: Arc<Mutex<HashMap<String, Arc<MethodChannel>>>>,
    event_channels: Arc<Mutex<HashMap<String, Arc<EventChannel>>>>,
    default_binary_messenger: Arc<BinaryMessenger>,
}

impl ServicesBinding {
    pub fn new() -> Self {
        Self {
            method_channels: Arc::new(Mutex::new(HashMap::new())),
            event_channels: Arc::new(Mutex::new(HashMap::new())),
            default_binary_messenger: Arc::new(BinaryMessenger::new()),
        }
    }

    /// Get the default binary messenger
    pub fn default_binary_messenger(&self) -> Arc<BinaryMessenger> {
        self.default_binary_messenger.clone()
    }

    /// Register a method channel
    pub fn register_method_channel(&self, name: String, channel: Arc<MethodChannel>) {
        self.method_channels.lock().insert(name, channel);
    }

    /// Register an event channel
    pub fn register_event_channel(&self, name: String, channel: Arc<EventChannel>) {
        self.event_channels.lock().insert(name, channel);
    }

    /// Handle platform message
    pub fn handle_platform_message(&self, channel: &str, data: Vec<u8>) -> Option<Vec<u8>> {
        // Dispatch to appropriate channel
        if let Some(method_channel) = self.method_channels.lock().get(channel) {
            return method_channel.handle_message(data);
        }

        if let Some(event_channel) = self.event_channels.lock().get(channel) {
            event_channel.handle_message(data);
        }

        None
    }
}

impl BindingBase for ServicesBinding {
    fn init_instances(&mut self) {
        tracing::debug!("ServicesBinding initialized");
    }

    fn lock_events(&self) {
        // Lock message handling
    }

    fn unlock_events(&self) {
        // Unlock message handling
    }
}
```

### RendererBinding

```rust
// In flui_app/src/binding/renderer.rs

/// Binding for rendering
///
/// Manages the render tree and paint pipeline.
pub struct RendererBinding {
    pipeline_owner: Arc<PipelineOwner>,
    render_view: Arc<Mutex<Option<Arc<RenderView>>>>,
}

impl RendererBinding {
    pub fn new() -> Self {
        Self {
            pipeline_owner: Arc::new(PipelineOwner::new()),
            render_view: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the render view (root of render tree)
    pub fn set_render_view(&self, view: Arc<RenderView>) {
        *self.render_view.lock() = Some(view);
    }

    /// Get the render view
    pub fn render_view(&self) -> Option<Arc<RenderView>> {
        self.render_view.lock().clone()
    }

    /// Get the pipeline owner
    pub fn pipeline_owner(&self) -> Arc<PipelineOwner> {
        self.pipeline_owner.clone()
    }

    /// Draw frame (called by SchedulerBinding)
    pub fn draw_frame(&self) {
        if let Some(view) = self.render_view() {
            // Run pipeline
            self.pipeline_owner.flush_layout();
            self.pipeline_owner.flush_compositing_bits();
            self.pipeline_owner.flush_paint();

            // Composite layers
            let scene = view.composite_scene();

            // Send to embedder for display
            // TODO: Implement
        }
    }
}

impl BindingBase for RendererBinding {
    fn init_instances(&mut self) {
        tracing::debug!("RendererBinding initialized");

        // Listen for frame callbacks
        // TODO: Connect to SchedulerBinding
    }

    fn lock_events(&self) {
        // Prevent rendering
    }

    fn unlock_events(&self) {
        // Resume rendering
    }
}
```

### WidgetsBinding

```rust
// In flui_app/src/binding/widgets.rs

/// Binding for widgets
///
/// Manages the widget tree and element tree.
pub struct WidgetsBinding {
    element_tree: Arc<ElementTree>,
    root_element: Arc<Mutex<Option<ElementId>>>,
    build_owner: Arc<BuildOwner>,
}

impl WidgetsBinding {
    pub fn new() -> Self {
        Self {
            element_tree: Arc::new(ElementTree::new()),
            root_element: Arc::new(Mutex::new(None)),
            build_owner: Arc::new(BuildOwner::new()),
        }
    }

    /// Attach root widget
    pub fn attach_root_widget(&self, widget: impl View + 'static) {
        let root_element = self.element_tree.inflate(Box::new(widget), None);
        *self.root_element.lock() = Some(root_element);

        // Schedule initial build
        self.schedule_build_for(root_element);
    }

    /// Schedule build for an element
    pub fn schedule_build_for(&self, element_id: ElementId) {
        self.build_owner.schedule_build(element_id);
    }

    /// Get the element tree
    pub fn element_tree(&self) -> Arc<ElementTree> {
        self.element_tree.clone()
    }

    /// Get the build owner
    pub fn build_owner(&self) -> Arc<BuildOwner> {
        self.build_owner.clone()
    }

    /// Handle frame (called by SchedulerBinding)
    fn handle_build_frame(&self) {
        // Flush build
        self.build_owner.flush_build(&self.element_tree);
    }
}

impl BindingBase for WidgetsBinding {
    fn init_instances(&mut self) {
        tracing::debug!("WidgetsBinding initialized");

        // Listen for frame callbacks
        // TODO: Connect to SchedulerBinding
    }

    fn lock_events(&self) {
        // Prevent building
    }

    fn unlock_events(&self) {
        // Resume building
    }
}
```

### WidgetsFlutterBinding (Combined)

```rust
// In flui_app/src/binding/widgets_flutter_binding.rs

/// Combined binding for Flutter-style apps
///
/// This is the main entry point, combining all binding mixins.
pub struct WidgetsFlutterBinding {
    gesture: GestureBinding,
    scheduler: SchedulerBinding,
    services: ServicesBinding,
    renderer: RendererBinding,
    widgets: WidgetsBinding,
}

impl WidgetsFlutterBinding {
    /// Ensure the binding is initialized (idempotent)
    pub fn ensure_initialized() -> Arc<Self> {
        static INSTANCE: OnceLock<Arc<WidgetsFlutterBinding>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| {
                let binding = Self {
                    gesture: GestureBinding::new(),
                    scheduler: SchedulerBinding::new(),
                    services: ServicesBinding::new(),
                    renderer: RendererBinding::new(),
                    widgets: WidgetsBinding::new(),
                };

                // Initialize all bindings in order
                binding.init_instances();

                Arc::new(binding)
            })
            .clone()
    }

    fn init_instances(&self) {
        tracing::info!("Initializing WidgetsFlutterBinding");

        // Initialize in dependency order (matches Flutter)
        let mut gesture = self.gesture;
        gesture.init_instances();

        let mut scheduler = self.scheduler;
        scheduler.init_instances();

        let mut services = self.services;
        services.init_instances();

        let mut renderer = self.renderer;
        renderer.init_instances();

        let mut widgets = self.widgets;
        widgets.init_instances();

        tracing::info!("WidgetsFlutterBinding initialized successfully");
    }

    /// Attach root widget and start app
    pub fn attach_root_widget(&self, widget: impl View + 'static) {
        self.widgets.attach_root_widget(widget);
        self.scheduler.schedule_frame();
    }

    // Accessor methods for each binding
    pub fn gesture(&self) -> &GestureBinding {
        &self.gesture
    }

    pub fn scheduler(&self) -> &SchedulerBinding {
        &self.scheduler
    }

    pub fn services(&self) -> &ServicesBinding {
        &self.services
    }

    pub fn renderer(&self) -> &RendererBinding {
        &self.renderer
    }

    pub fn widgets(&self) -> &WidgetsBinding {
        &self.widgets
    }
}

impl BindingBase for WidgetsFlutterBinding {
    fn init_instances(&mut self) {
        // Already handled in constructor
    }

    fn lock_events(&self) {
        self.gesture.lock_events();
        self.scheduler.lock_events();
        self.services.lock_events();
        self.renderer.lock_events();
        self.widgets.lock_events();
    }

    fn unlock_events(&self) {
        self.gesture.unlock_events();
        self.scheduler.unlock_events();
        self.services.unlock_events();
        self.renderer.unlock_events();
        self.widgets.unlock_events();
    }

    fn handle_memory_pressure(&self) {
        // Clear caches, etc.
        tracing::warn!("Handling memory pressure");
    }
}
```

---

## runApp() and App Initialization

### runApp() Function

```rust
// In flui_app/src/lib.rs

/// Run a Flutter-style app
///
/// This is the main entry point for FLUI applications.
/// It initializes bindings, creates the platform embedder,
/// and starts the event loop.
///
/// # Example
///
/// ```rust
/// use flui_app::runApp;
/// use flui_widgets::MaterialApp;
///
/// fn main() {
///     runApp(MaterialApp::new("My App", MyHomeView::new()));
/// }
/// ```
pub fn runApp(app: impl View + 'static) {
    // 1. Initialize bindings
    let binding = WidgetsFlutterBinding::ensure_initialized();

    // 2. Attach root widget
    binding.attach_root_widget(app);

    // 3. Create platform embedder
    let embedder = create_platform_embedder(binding.clone());

    // 4. Run event loop
    embedder.run();
}

/// Create the appropriate platform embedder
fn create_platform_embedder(binding: Arc<WidgetsFlutterBinding>) -> Box<dyn PlatformEmbedder> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsEmbedder::new(binding))
    }

    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxEmbedder::new(binding))
    }

    #[cfg(target_os = "macos")]
    {
        Box::new(MacOSEmbedder::new(binding))
    }

    #[cfg(target_os = "android")]
    {
        Box::new(AndroidEmbedder::new(binding))
    }

    #[cfg(target_os = "ios")]
    {
        Box::new(IOSEmbedder::new(binding))
    }

    #[cfg(target_family = "wasm")]
    {
        Box::new(WebEmbedder::new(binding))
    }
}
```

### App Widget

```rust
// In flui_app/src/app.rs

/// Base App widget
///
/// Provides common app-level functionality.
#[derive(Debug)]
pub struct App {
    title: String,
    home: AnyElement,
    theme: Option<Theme>,
    routes: HashMap<String, Box<dyn Fn() -> AnyElement>>,
}

impl App {
    pub fn new(title: impl Into<String>, home: AnyElement) -> Self {
        Self {
            title: title.into(),
            home,
            theme: None,
            routes: HashMap::new(),
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn routes(mut self, routes: HashMap<String, Box<dyn Fn() -> AnyElement>>) -> Self {
        self.routes = routes;
        self
    }
}

impl View for App {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Provide theme
        let theme = self.theme.unwrap_or_default();

        ThemeProvider::new(theme, self.home)
    }
}
```

---

## Platform Channels

### BinaryMessenger

```rust
// In flui_app/src/platform_channels/binary_messenger.rs

/// Low-level message passing between Flutter and platform
pub struct BinaryMessenger {
    handlers: Arc<Mutex<HashMap<String, MessageHandler>>>,
}

pub type MessageHandler = Arc<dyn Fn(Vec<u8>) -> Option<Vec<u8>> + Send + Sync>;

impl BinaryMessenger {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send a message to the platform
    pub fn send(&self, channel: &str, message: Vec<u8>) -> Option<Vec<u8>> {
        let handlers = self.handlers.lock();
        handlers.get(channel).and_then(|handler| handler(message))
    }

    /// Set a message handler for a channel
    pub fn set_message_handler(&self, channel: String, handler: MessageHandler) {
        self.handlers.lock().insert(channel, handler);
    }
}
```

### MethodChannel

```rust
// In flui_app/src/platform_channels/method_channel.rs

/// Method channel for platform communication
///
/// Supports request-response style communication.
pub struct MethodChannel {
    name: String,
    codec: Arc<dyn MethodCodec>,
    messenger: Arc<BinaryMessenger>,
}

impl MethodChannel {
    pub fn new(
        name: impl Into<String>,
        codec: Arc<dyn MethodCodec>,
        messenger: Arc<BinaryMessenger>,
    ) -> Self {
        Self {
            name: name.into(),
            codec,
            messenger,
        }
    }

    /// Invoke a method on the platform
    pub async fn invoke_method(
        &self,
        method: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, MethodChannelError> {
        // Encode method call
        let call = MethodCall {
            method: method.to_string(),
            arguments,
        };

        let encoded = self.codec.encode_method_call(&call)?;

        // Send to platform
        let response = self
            .messenger
            .send(&self.name, encoded)
            .ok_or(MethodChannelError::NoResponse)?;

        // Decode response
        self.codec.decode_envelope(&response)
    }

    /// Set a method call handler
    pub fn set_method_call_handler<F>(&self, handler: F)
    where
        F: Fn(MethodCall) -> Result<serde_json::Value, String> + Send + Sync + 'static,
    {
        let codec = self.codec.clone();
        let handler = Arc::new(handler);

        self.messenger.set_message_handler(
            self.name.clone(),
            Arc::new(move |data| {
                // Decode method call
                let call = codec.decode_method_call(&data).ok()?;

                // Handle call
                let result = handler(call);

                // Encode response
                match result {
                    Ok(value) => codec.encode_success_envelope(&value).ok(),
                    Err(error) => codec.encode_error_envelope(&error).ok(),
                }
            }),
        );
    }
}

#[derive(Debug, Clone)]
pub struct MethodCall {
    pub method: String,
    pub arguments: Option<serde_json::Value>,
}

pub trait MethodCodec: Send + Sync {
    fn encode_method_call(&self, call: &MethodCall) -> Result<Vec<u8>, MethodChannelError>;
    fn decode_method_call(&self, data: &[u8]) -> Result<MethodCall, MethodChannelError>;
    fn encode_success_envelope(&self, result: &serde_json::Value) -> Result<Vec<u8>, MethodChannelError>;
    fn encode_error_envelope(&self, error: &str) -> Result<Vec<u8>, MethodChannelError>;
    fn decode_envelope(&self, data: &[u8]) -> Result<serde_json::Value, MethodChannelError>;
}

/// JSON method codec (default)
pub struct JSONMethodCodec;

impl MethodCodec for JSONMethodCodec {
    fn encode_method_call(&self, call: &MethodCall) -> Result<Vec<u8>, MethodChannelError> {
        Ok(serde_json::to_vec(call)?)
    }

    fn decode_method_call(&self, data: &[u8]) -> Result<MethodCall, MethodChannelError> {
        Ok(serde_json::from_slice(data)?)
    }

    fn encode_success_envelope(&self, result: &serde_json::Value) -> Result<Vec<u8>, MethodChannelError> {
        Ok(serde_json::to_vec(&vec![result])?)
    }

    fn encode_error_envelope(&self, error: &str) -> Result<Vec<u8>, MethodChannelError> {
        Ok(serde_json::to_vec(&vec![serde_json::Value::Null, error])?)
    }

    fn decode_envelope(&self, data: &[u8]) -> Result<serde_json::Value, MethodChannelError> {
        let envelope: Vec<serde_json::Value> = serde_json::from_slice(data)?;
        if envelope.is_empty() {
            return Err(MethodChannelError::InvalidEnvelope);
        }
        Ok(envelope[0].clone())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MethodChannelError {
    #[error("No response from platform")]
    NoResponse,

    #[error("Invalid envelope")]
    InvalidEnvelope,

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}
```

### EventChannel

```rust
// In flui_app/src/platform_channels/event_channel.rs

/// Event channel for streaming data from platform
///
/// Supports continuous data streams (e.g., sensor data, location updates).
pub struct EventChannel {
    name: String,
    codec: Arc<dyn MethodCodec>,
    messenger: Arc<BinaryMessenger>,
    stream: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<serde_json::Value>>>>,
}

impl EventChannel {
    pub fn new(
        name: impl Into<String>,
        codec: Arc<dyn MethodCodec>,
        messenger: Arc<BinaryMessenger>,
    ) -> Self {
        Self {
            name: name.into(),
            codec,
            messenger,
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// Receive events from platform (as async stream)
    pub async fn receive_events(&self) -> impl Stream<Item = serde_json::Value> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        *self.stream.lock() = Some(rx);

        // Set up message handler
        let tx = Arc::new(Mutex::new(tx));
        let codec = self.codec.clone();

        self.messenger.set_message_handler(
            self.name.clone(),
            Arc::new(move |data| {
                // Decode event
                if let Ok(event) = codec.decode_envelope(&data) {
                    let _ = tx.lock().try_send(event);
                }
                None
            }),
        );

        tokio_stream::wrappers::ReceiverStream::new(
            self.stream.lock().take().unwrap()
        )
    }
}
```

---

## Platform Embedders

### PlatformEmbedder Trait

```rust
// In flui_app/src/embedder/mod.rs

/// Platform embedder interface
///
/// Each platform implements this trait to provide platform-specific functionality.
pub trait PlatformEmbedder: Send + Sync {
    /// Create a window
    fn create_window(&mut self, config: WindowConfig) -> Result<WindowId, EmbedderError>;

    /// Run the event loop (blocks until app exit)
    fn run(&mut self);

    /// Handle platform event
    fn handle_event(&mut self, event: PlatformEvent);

    /// Get the rendering surface
    fn get_surface(&self, window_id: WindowId) -> Option<Arc<RenderSurface>>;
}

#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub decorated: bool,
    pub transparent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

pub enum PlatformEvent {
    WindowResized { window_id: WindowId, width: u32, height: u32 },
    WindowClosed { window_id: WindowId },
    PointerEvent { window_id: WindowId, event: PointerEvent },
    KeyEvent { window_id: WindowId, event: KeyEvent },
    WindowFocusChanged { window_id: WindowId, focused: bool },
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    #[error("Window creation failed: {0}")]
    WindowCreationFailed(String),

    #[error("Platform error: {0}")]
    PlatformError(String),
}
```

### Windows Embedder (Win32)

```rust
// In flui_app/src/embedder/windows.rs

#[cfg(target_os = "windows")]
pub struct WindowsEmbedder {
    binding: Arc<WidgetsFlutterBinding>,
    windows: HashMap<WindowId, Win32Window>,
    next_window_id: u64,
}

#[cfg(target_os = "windows")]
impl WindowsEmbedder {
    pub fn new(binding: Arc<WidgetsFlutterBinding>) -> Self {
        Self {
            binding,
            windows: HashMap::new(),
            next_window_id: 1,
        }
    }
}

#[cfg(target_os = "windows")]
impl PlatformEmbedder for WindowsEmbedder {
    fn create_window(&mut self, config: WindowConfig) -> Result<WindowId, EmbedderError> {
        let window_id = WindowId(self.next_window_id);
        self.next_window_id += 1;

        let window = Win32Window::new(window_id, config)?;
        self.windows.insert(window_id, window);

        Ok(window_id)
    }

    fn run(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let mut msg = MSG::default();

            loop {
                if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                    break;
                }

                TranslateMessage(&msg);
                DispatchMessageW(&msg);

                // Schedule frame if needed
                self.binding.scheduler().schedule_frame();
            }
        }
    }

    fn handle_event(&mut self, event: PlatformEvent) {
        match event {
            PlatformEvent::PointerEvent { event, .. } => {
                self.binding.gesture().handle_pointer_event(event);
            }
            PlatformEvent::WindowResized { window_id, width, height } => {
                // Update render surface size
            }
            _ => {}
        }
    }

    fn get_surface(&self, window_id: WindowId) -> Option<Arc<RenderSurface>> {
        self.windows.get(&window_id).map(|w| w.surface.clone())
    }
}

#[cfg(target_os = "windows")]
struct Win32Window {
    id: WindowId,
    hwnd: HWND,
    surface: Arc<RenderSurface>,
}

// Implementation details...
```

### Linux Embedder (GTK)

```rust
// In flui_app/src/embedder/linux.rs

#[cfg(target_os = "linux")]
pub struct LinuxEmbedder {
    binding: Arc<WidgetsFlutterBinding>,
    windows: HashMap<WindowId, GtkWindow>,
    next_window_id: u64,
}

#[cfg(target_os = "linux")]
impl LinuxEmbedder {
    pub fn new(binding: Arc<WidgetsFlutterBinding>) -> Self {
        // Initialize GTK
        gtk::init().expect("Failed to initialize GTK");

        Self {
            binding,
            windows: HashMap::new(),
            next_window_id: 1,
        }
    }
}

#[cfg(target_os = "linux")]
impl PlatformEmbedder for LinuxEmbedder {
    fn create_window(&mut self, config: WindowConfig) -> Result<WindowId, EmbedderError> {
        let window_id = WindowId(self.next_window_id);
        self.next_window_id += 1;

        let window = GtkWindow::new(window_id, config)?;
        self.windows.insert(window_id, window);

        Ok(window_id)
    }

    fn run(&mut self) {
        // Run GTK main loop
        gtk::main();
    }

    fn handle_event(&mut self, event: PlatformEvent) {
        // GTK event handling
    }

    fn get_surface(&self, window_id: WindowId) -> Option<Arc<RenderSurface>> {
        self.windows.get(&window_id).map(|w| w.surface.clone())
    }
}

// GTK window implementation...
```

### macOS Embedder (Cocoa)

```rust
// In flui_app/src/embedder/macos.rs

#[cfg(target_os = "macos")]
pub struct MacOSEmbedder {
    binding: Arc<WidgetsFlutterBinding>,
    windows: HashMap<WindowId, CocoaWindow>,
    next_window_id: u64,
}

#[cfg(target_os = "macos")]
impl MacOSEmbedder {
    pub fn new(binding: Arc<WidgetsFlutterBinding>) -> Self {
        Self {
            binding,
            windows: HashMap::new(),
            next_window_id: 1,
        }
    }
}

// Cocoa/AppKit integration...
```

---

## Window Management

### Window Trait

```rust
// In flui_app/src/window.rs

/// Window management
pub trait Window {
    /// Get window ID
    fn id(&self) -> WindowId;

    /// Set window title
    fn set_title(&self, title: &str);

    /// Set window size
    fn set_size(&self, width: u32, height: u32);

    /// Get window size
    fn size(&self) -> (u32, u32);

    /// Show window
    fn show(&self);

    /// Hide window
    fn hide(&self);

    /// Close window
    fn close(&self);

    /// Is window visible?
    fn is_visible(&self) -> bool;

    /// Set window position
    fn set_position(&self, x: i32, y: i32);

    /// Get window position
    fn position(&self) -> (i32, i32);

    /// Maximize window
    fn maximize(&self);

    /// Minimize window
    fn minimize(&self);

    /// Restore window
    fn restore(&self);

    /// Is window maximized?
    fn is_maximized(&self) -> bool;

    /// Is window minimized?
    fn is_minimized(&self) -> bool;
}
```

---

## Event Loop Integration

### Frame Scheduling

```rust
// In flui_app/src/frame_scheduler.rs

/// Frame scheduler for vsync
pub struct FrameScheduler {
    frame_requested: AtomicBool,
    last_frame_time: Arc<Mutex<Option<Instant>>>,
    target_fps: AtomicU64,
}

impl FrameScheduler {
    pub fn new() -> Self {
        Self {
            frame_requested: AtomicBool::new(false),
            last_frame_time: Arc::new(Mutex::new(None)),
            target_fps: AtomicU64::new(60),
        }
    }

    /// Request a frame
    pub fn request_frame(&self) {
        self.frame_requested.store(true, Ordering::Relaxed);
    }

    /// Check if frame is requested
    pub fn is_frame_requested(&self) -> bool {
        self.frame_requested.load(Ordering::Relaxed)
    }

    /// Clear frame request
    pub fn clear_frame_request(&self) {
        self.frame_requested.store(false, Ordering::Relaxed);
    }

    /// Get target frame duration
    pub fn target_frame_duration(&self) -> Duration {
        let fps = self.target_fps.load(Ordering::Relaxed);
        Duration::from_secs_f64(1.0 / fps as f64)
    }

    /// Set target FPS
    pub fn set_target_fps(&self, fps: u64) {
        self.target_fps.store(fps, Ordering::Relaxed);
    }
}
```

---

## Plugin System

### Plugin Trait

```rust
// In flui_app/src/plugin.rs

/// Plugin interface for extending FLUI
pub trait Plugin: Send + Sync {
    /// Plugin name
    fn name(&self) -> &str;

    /// Initialize plugin
    fn init(&mut self, binding: Arc<WidgetsFlutterBinding>);

    /// Register platform channels
    fn register_channels(&self, services: &ServicesBinding);
}

/// Plugin registry
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    /// Initialize all plugins
    pub fn init_all(&mut self, binding: Arc<WidgetsFlutterBinding>) {
        for plugin in &mut self.plugins {
            plugin.init(binding.clone());
            plugin.register_channels(binding.services());
        }
    }
}
```

---

## Implementation Plan

### Phase 1: Core Binding System (~800 LOC)

1. **binding/base.rs** (~100 LOC)
   - `BindingBase` trait
   - Global binding storage

2. **binding/gesture.rs** (~150 LOC)
   - `GestureBinding`
   - Pointer event handling

3. **binding/scheduler.rs** (~150 LOC)
   - `SchedulerBinding`
   - Frame callbacks

4. **binding/services.rs** (~150 LOC)
   - `ServicesBinding`
   - Platform channel management

5. **binding/renderer.rs** (~150 LOC)
   - `RendererBinding`
   - Paint pipeline integration

6. **binding/widgets.rs** (~150 LOC)
   - `WidgetsBinding`
   - Widget tree management

7. **binding/widgets_flutter_binding.rs** (~150 LOC)
   - Combined binding

**Total Phase 1:** ~800 LOC

### Phase 2: Platform Channels (~400 LOC)

8. **platform_channels/binary_messenger.rs** (~100 LOC)
   - `BinaryMessenger`

9. **platform_channels/method_channel.rs** (~200 LOC)
   - `MethodChannel`
   - `MethodCodec`

10. **platform_channels/event_channel.rs** (~100 LOC)
    - `EventChannel`

**Total Phase 2:** ~400 LOC

### Phase 3: Platform Embedders (~1,500 LOC)

11. **embedder/mod.rs** (~200 LOC)
    - `PlatformEmbedder` trait
    - Common types

12. **embedder/windows.rs** (~400 LOC)
    - Windows/Win32 embedder

13. **embedder/linux.rs** (~400 LOC)
    - Linux/GTK embedder

14. **embedder/macos.rs** (~400 LOC)
    - macOS/Cocoa embedder

15. **embedder/web.rs** (~100 LOC)
    - Web/WASM embedder (basic)

**Total Phase 3:** ~1,500 LOC

### Phase 4: App & runApp (~300 LOC)

16. **lib.rs** (~100 LOC)
    - `runApp()` function
    - Platform embedder selection

17. **app.rs** (~200 LOC)
    - `App` widget
    - Theme provider

**Total Phase 4:** ~300 LOC

### Phase 5: Window Management & Plugins (~500 LOC)

18. **window.rs** (~200 LOC)
    - `Window` trait
    - Window management

19. **frame_scheduler.rs** (~100 LOC)
    - `FrameScheduler`

20. **plugin.rs** (~200 LOC)
    - `Plugin` trait
    - `PluginRegistry`

**Total Phase 5:** ~500 LOC

---

## Usage Examples

### Example 1: Basic App

```rust
use flui_app::runApp;
use flui_widgets::*;

fn main() {
    runApp(
        App::new("Hello FLUI",
            Box::new(Center::new(
                Text::new("Hello, World!")
            ))
        )
    );
}
```

### Example 2: MaterialApp

```rust
use flui_app::runApp;
use flui_widgets::*;

fn main() {
    runApp(
        MaterialApp::builder()
            .title("My App")
            .theme(ThemeData::light())
            .home(MyHomeView::new())
            .build()
    );
}
```

### Example 3: Platform Channel

```rust
use flui_app::*;

fn main() {
    let binding = WidgetsFlutterBinding::ensure_initialized();

    // Register method channel
    let channel = MethodChannel::new(
        "com.example.battery",
        Arc::new(JSONMethodCodec),
        binding.services().default_binary_messenger(),
    );

    channel.set_method_call_handler(|call| {
        match call.method.as_str() {
            "getBatteryLevel" => {
                // Get battery level from platform
                Ok(serde_json::json!(85))
            }
            _ => Err("Method not found".to_string()),
        }
    });

    runApp(MyApp::new());
}
```

### Example 4: Custom Plugin

```rust
use flui_app::*;

struct BatteryPlugin;

impl Plugin for BatteryPlugin {
    fn name(&self) -> &str {
        "battery"
    }

    fn init(&mut self, binding: Arc<WidgetsFlutterBinding>) {
        println!("Battery plugin initialized");
    }

    fn register_channels(&self, services: &ServicesBinding) {
        let channel = MethodChannel::new(
            "com.example.battery",
            Arc::new(JSONMethodCodec),
            services.default_binary_messenger(),
        );

        channel.set_method_call_handler(|call| {
            // Handle battery methods
            Ok(serde_json::json!(null))
        });

        services.register_method_channel("com.example.battery".to_string(), Arc::new(channel));
    }
}

fn main() {
    let mut registry = PluginRegistry::new();
    registry.register(Box::new(BatteryPlugin));

    let binding = WidgetsFlutterBinding::ensure_initialized();
    registry.init_all(binding.clone());

    runApp(MyApp::new());
}
```

---

## Testing Strategy

### Unit Tests

1. **Binding Initialization:**
   - Test binding order
   - Test idempotency (ensure_initialized)
   - Test accessor methods

2. **Platform Channels:**
   - Test MethodChannel invoke
   - Test EventChannel streaming
   - Test codec encoding/decoding

3. **Frame Scheduling:**
   - Test frame request
   - Test vsync timing
   - Test callback execution

### Integration Tests

1. **Cross-Platform:**
   - Test app startup on Windows
   - Test app startup on Linux
   - Test app startup on macOS

2. **Window Management:**
   - Test window creation
   - Test window resize
   - Test window close

3. **Performance:**
   - Benchmark frame scheduling overhead
   - Test 1000 frame callbacks
   - Measure memory usage

---

## Crate Dependencies

```toml
# crates/flui_app/Cargo.toml

[package]
name = "flui_app"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_core = { path = "../flui_core" }
flui_types = { path = "../flui_types" }
flui_widgets = { path = "../flui_widgets" }
flui_rendering = { path = "../flui_rendering" }
flui_engine = { path = "../flui_engine" }
flui_gestures = { path = "../flui_gestures" }
flui_animation = { path = "../flui_animation" }

# Async runtime
tokio = { version = "1.43", features = ["sync", "time"] }
tokio-stream = "0.1"
async-trait = "0.1"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Logging
tracing = "0.1"

# Error handling
thiserror = "1.0"

# Platform-specific dependencies
[target.'cfg(windows)'.dependencies]
windows = { version = "0.52", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_System_LibraryLoader",
] }

[target.'cfg(target_os = "linux")'.dependencies]
gtk = "0.18"

[target.'cfg(target_os = "macos")'.dependencies]
cocoa = "0.25"
objc = "0.2"

[dev-dependencies]
tokio = { version = "1.43", features = ["full", "test-util"] }
```

---

## Open Questions

1. **Android/iOS Support:**
   - Should we use JNI for Android?
   - Should we use Swift or Objective-C for iOS?
   - How to package as APK/IPA?

2. **Web Support:**
   - Full WASM support with wgpu?
   - Canvas 2D fallback?
   - WebGL backend?

3. **Hot Reload:**
   - Should we support hot reload like Flutter?
   - How to preserve state during reload?

4. **Multi-Window:**
   - Should we support multiple windows?
   - How to manage window lifecycle?

---

## Version History

| Version | Date       | Author | Changes                   |
|---------|------------|--------|---------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial app architecture  |

---

## References

- [Flutter Architectural Overview](https://docs.flutter.dev/resources/architectural-overview)
- [Flutter WidgetsFlutterBinding](https://api.flutter.dev/flutter/widgets/WidgetsFlutterBinding-class.html)
- [Flutter Platform Channels](https://docs.flutter.dev/platform-integration/platform-channels)
- [Flutter Embedder API](https://docs.flutter.dev/embedded)

---

## Conclusion

This architecture provides a **complete, Flutter-compatible application framework** for FLUI:

✅ **Cross-platform by default** (Windows, Linux, macOS, + mobile/web planned)
✅ **Binding system** (layered mixins like Flutter)
✅ **runApp() API** (Flutter-compatible entry point)
✅ **Platform channels** (MethodChannel, EventChannel)
✅ **Platform embedders** (Win32, GTK, Cocoa)
✅ **Window management** (create, resize, close)
✅ **Frame scheduling** (vsync, frame callbacks)
✅ **Plugin system** (extensible platform integration)

**Key Patterns:**
1. **Binding Mixins**: Services → Scheduler → Gesture → Renderer → Widgets (Flutter order)
2. **runApp()**: Single entry point, automatic binding initialization
3. **Platform Embedders**: Platform-specific code only compiled on target OS
4. **WidgetsFlutterBinding**: Combined binding with all mixins

**Estimated Total Work:** ~3,500 LOC
- Core bindings (~800 LOC)
- Platform channels (~400 LOC)
- Platform embedders (~1,500 LOC) - **desktop only** (mobile/web separate)
- App & runApp (~300 LOC)
- Window management & plugins (~500 LOC)

This provides a solid foundation for cross-platform FLUI applications! 🚀🖥️

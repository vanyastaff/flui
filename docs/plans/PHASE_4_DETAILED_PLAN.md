# Phase 4: Application Layer - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущие фазы**: Phase 1 (Foundation) + Phase 2 (Rendering) + Phase 3 (Interaction) должны быть завершены  
> **Референсы**: `.gpui/src/app.rs`, Flutter's `WidgetsFlutterBinding`, `runApp()` pattern  
> **Цель**: Production-ready application framework с полным lifecycle management и multi-window support

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui_app
- ✅ Cargo.toml с зависимостями на все core crates
- ✅ Модульная структура: `app/`, `bindings/`, `debug/`, `embedder/`, `overlay/`, `theme/`
- ✅ Базовый lib.rs с re-exports
- ✅ Flutter naming: `WidgetsFlutterBinding`, `run_app()`
- ✅ Debug infrastructure: `DebugFlags`

#### Зависимости готовы
- ✅ flui-platform (Phase 1) - platform abstraction
- ✅ flui_engine (Phase 2) - rendering
- ✅ flui_interaction (Phase 3) - events/gestures
- ✅ flui-view - widget tree
- ✅ flui_rendering - render tree
- ✅ flui-scheduler - frame scheduling

### ❌ Что Нужно Доделать / Улучшить

#### Core Application
1. **Application** - main app entry point
2. **AppBuilder** - fluent configuration API
3. **AppContext** - dependency container
4. **Lifecycle Events** - onCreate, onStart, onResume, onPause, onStop, onDestroy

#### Window Management
1. **WindowManager** - multi-window support
2. **Window** - individual window lifecycle
3. **WindowOptions** - configuration (size, title, decorations)
4. **WindowHandle** - safe reference to windows

#### Bindings Integration
1. **WidgetsFlutterBinding** - combines all bindings
2. **SchedulerBinding** - frame scheduling integration
3. **GestureBinding** - event routing integration
4. **RenderingBinding** - render pipeline integration

#### Runner
1. **EventLoop Integration** - platform event loop
2. **Frame Callbacks** - requestAnimationFrame equivalent
3. **Hot Reload** - development mode support (bonus)

---

## Детальный План Реализации

### Этап 4.1: Core Application (Неделя 7, Дни 1-3)

#### День 1: Application & AppBuilder

**Цель**: Create main Application entry point с builder pattern

**Референсы**:
- `.gpui/src/app.rs` - GPUI Application struct
- Plan `3.5.2 App Design` - App as Coordinator spec
- Flutter `runApp()` pattern

**Задачи**:

1. **Создать `app/application.rs`**
   ```rust
   use std::sync::Arc;
   use parking_lot::RwLock;
   
   /// FLUI Application
   ///
   /// Central coordinator for the entire application.
   /// Manages platform, engine, interaction, and all bindings.
   pub struct Application {
       /// Application context (shared state)
       context: Arc<AppContext>,
       
       /// Window manager
       window_manager: Arc<WindowManager>,
       
       /// Lifecycle state
       lifecycle: Arc<RwLock<AppLifecycle>>,
       
       /// Configuration
       config: AppConfig,
   }
   
   impl Application {
       /// Create new application with builder
       pub fn builder() -> AppBuilder {
           AppBuilder::default()
       }
       
       /// Run the application
       ///
       /// This starts the platform event loop and never returns.
       pub fn run(self) -> ! {
           tracing::info!("Starting FLUI application");
           
           // Initialize logging
           if self.config.enable_logging {
               flui_log::init(self.config.log_level);
           }
           
           // Trigger lifecycle: onCreate
           self.lifecycle.write().transition_to(LifecycleState::Created);
           
           // Create root window if requested
           if let Some(root_config) = self.config.root_window {
               self.window_manager.create_window(root_config)
                   .expect("Failed to create root window");
           }
           
           // Trigger lifecycle: onStart
           self.lifecycle.write().transition_to(LifecycleState::Started);
           
           // Get platform and start event loop
           let platform = self.context.platform.clone();
           let context = Arc::clone(&self.context);
           
           platform.run(Box::new(move || {
               tracing::info!("Platform ready, application running");
               
               // Trigger lifecycle: onResume
               context.lifecycle.write().transition_to(LifecycleState::Resumed);
               
               // Call user's onReady callback
               if let Some(callback) = context.config.on_ready.take() {
                   callback(&context);
               }
           }));
           
           // Note: platform.run() never returns
           unreachable!()
       }
   }
   
   /// Application builder (fluent API)
   #[derive(Default)]
   pub struct AppBuilder {
       config: AppConfig,
       platform: Option<Arc<dyn Platform>>,
       root_window: Option<WindowOptions>,
       on_ready: Option<Box<dyn FnOnce(&AppContext) + Send>>,
   }
   
   impl AppBuilder {
       /// Set custom platform (default: auto-detect)
       pub fn platform(mut self, platform: Arc<dyn Platform>) -> Self {
           self.platform = Some(platform);
           self
       }
       
       /// Configure root window
       pub fn with_root_window(mut self, options: WindowOptions) -> Self {
           self.root_window = Some(options);
           self
       }
       
       /// Set window title (convenience)
       pub fn title(mut self, title: impl Into<String>) -> Self {
           let mut opts = self.root_window.take().unwrap_or_default();
           opts.title = title.into();
           self.root_window = Some(opts);
           self
       }
       
       /// Set window size (convenience)
       pub fn size(mut self, width: u32, height: u32) -> Self {
           let mut opts = self.root_window.take().unwrap_or_default();
           opts.size = Size::new(width as f32, height as f32);
           self.root_window = Some(opts);
           self
       }
       
       /// Enable debug overlays
       pub fn debug_mode(mut self, enabled: bool) -> Self {
           self.config.debug_mode = enabled;
           self
       }
       
       /// Set logging level
       pub fn log_level(mut self, level: flui_log::Level) -> Self {
           self.config.log_level = level;
           self
       }
       
       /// Callback when app is ready
       pub fn on_ready<F>(mut self, callback: F) -> Self
       where
           F: FnOnce(&AppContext) + Send + 'static,
       {
           self.on_ready = Some(Box::new(callback));
           self
       }
       
       /// Build the application
       pub async fn build(mut self) -> Result<Application, AppError> {
           // Get or create platform
           let platform = self.platform.unwrap_or_else(|| {
               flui_platform::current_platform()
           });
           
           tracing::info!("Using platform: {}", platform.name());
           
           // Create rendering engine
           let engine = Arc::new(
               flui_engine::Engine::from_platform(platform.as_ref())
                   .await
                   .context("Failed to create rendering engine")?
           );
           
           tracing::info!("Rendering engine initialized");
           
           // Create event dispatcher
           let dispatcher = Arc::new(flui_interaction::EventRouter::new(
               Arc::new(flui_interaction::FocusManager::global().clone())
           ));
           
           // Create window manager
           let window_manager = Arc::new(WindowManager::new(
               Arc::clone(&platform),
               Arc::clone(&engine),
               Arc::clone(&dispatcher),
           ));
           
           // Create app context
           let context = Arc::new(AppContext {
               platform: Arc::clone(&platform),
               engine: Arc::clone(&engine),
               dispatcher: Arc::clone(&dispatcher),
               window_manager: Arc::clone(&window_manager),
               lifecycle: Arc::new(RwLock::new(AppLifecycle::new())),
               config: self.config.clone(),
           });
           
           // Store user callbacks in config
           self.config.on_ready = self.on_ready.map(|f| Arc::new(Mutex::new(Some(f))));
           self.config.root_window = self.root_window;
           
           Ok(Application {
               context,
               window_manager,
               lifecycle: Arc::new(RwLock::new(AppLifecycle::new())),
               config: self.config,
           })
       }
   }
   
   /// Application configuration
   #[derive(Clone)]
   pub struct AppConfig {
       pub debug_mode: bool,
       pub enable_logging: bool,
       pub log_level: flui_log::Level,
       pub root_window: Option<WindowOptions>,
       pub on_ready: Option<Arc<Mutex<Option<Box<dyn FnOnce(&AppContext) + Send>>>>>,
   }
   
   impl Default for AppConfig {
       fn default() -> Self {
           Self {
               debug_mode: cfg!(debug_assertions),
               enable_logging: true,
               log_level: if cfg!(debug_assertions) {
                   flui_log::Level::Debug
               } else {
                   flui_log::Level::Info
               },
               root_window: Some(WindowOptions::default()),
               on_ready: None,
           }
       }
   }
   ```

2. **Convenience `run_app()` Function**
   ```rust
   /// Run FLUI application (convenience function)
   ///
   /// # Example
   ///
   /// ```rust,ignore
   /// use flui_app::run_app;
   ///
   /// fn main() {
   ///     run_app(|| {
   ///         println!("App is ready!");
   ///     });
   /// }
   /// ```
   pub fn run_app<F>(ready: F) -> !
   where
       F: FnOnce(&AppContext) + Send + 'static,
   {
       pollster::block_on(async {
           let app = Application::builder()
               .on_ready(ready)
               .build()
               .await
               .expect("Failed to create application");
           
           app.run()
       })
   }
   
   /// Run application with custom configuration
   pub fn run_app_with_config<F>(config: AppConfig, ready: F) -> !
   where
       F: FnOnce(&AppContext) + Send + 'static,
   {
       pollster::block_on(async {
           let mut builder = Application::builder();
           builder.config = config;
           
           let app = builder
               .on_ready(ready)
               .build()
               .await
               .expect("Failed to create application");
           
           app.run()
       })
   }
   ```

**Критерии завершения**:
- [ ] AppBuilder fluent API works
- [ ] Application runs event loop
- [ ] run_app() convenience function works
- [ ] 20+ application tests

---

#### День 2: AppContext & Lifecycle

**Цель**: Dependency container и lifecycle management

**Задачи**:

1. **Создать `app/context.rs`**
   ```rust
   /// Application context (dependency container)
   ///
   /// Shared across all parts of the application.
   /// Provides access to platform, engine, dispatcher, etc.
   pub struct AppContext {
       /// Platform implementation
       pub platform: Arc<dyn Platform>,
       
       /// Rendering engine
       pub engine: Arc<Engine>,
       
       /// Event dispatcher
       pub dispatcher: Arc<EventRouter>,
       
       /// Window manager
       pub window_manager: Arc<WindowManager>,
       
       /// Lifecycle state
       pub lifecycle: Arc<RwLock<AppLifecycle>>,
       
       /// Configuration
       pub config: AppConfig,
   }
   
   impl AppContext {
       /// Create a new window
       pub fn create_window(&self, options: WindowOptions) -> Result<WindowHandle, AppError> {
           self.window_manager.create_window(options)
       }
       
       /// Get all windows
       pub fn windows(&self) -> Vec<WindowHandle> {
           self.window_manager.windows()
       }
       
       /// Request application quit
       pub fn quit(&self) {
           self.platform.quit();
       }
       
       /// Current lifecycle state
       pub fn lifecycle_state(&self) -> LifecycleState {
           self.lifecycle.read().state()
       }
   }
   ```

2. **Lifecycle Management (создать `app/lifecycle.rs`)**
   ```rust
   /// Application lifecycle
   pub struct AppLifecycle {
       state: LifecycleState,
       on_create: Vec<Box<dyn Fn() + Send + Sync>>,
       on_start: Vec<Box<dyn Fn() + Send + Sync>>,
       on_resume: Vec<Box<dyn Fn() + Send + Sync>>,
       on_pause: Vec<Box<dyn Fn() + Send + Sync>>,
       on_stop: Vec<Box<dyn Fn() + Send + Sync>>,
       on_destroy: Vec<Box<dyn Fn() + Send + Sync>>,
   }
   
   /// Lifecycle state (Flutter/Android-inspired)
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum LifecycleState {
       /// Initial state
       Uninitialized,
       
       /// onCreate() called
       Created,
       
       /// onStart() called
       Started,
       
       /// onResume() called (app is active)
       Resumed,
       
       /// onPause() called (app going to background)
       Paused,
       
       /// onStop() called (app not visible)
       Stopped,
       
       /// onDestroy() called (app terminating)
       Destroyed,
   }
   
   impl AppLifecycle {
       pub fn new() -> Self {
           Self {
               state: LifecycleState::Uninitialized,
               on_create: Vec::new(),
               on_start: Vec::new(),
               on_resume: Vec::new(),
               on_pause: Vec::new(),
               on_stop: Vec::new(),
               on_destroy: Vec::new(),
           }
       }
       
       /// Register lifecycle callback
       pub fn on_create<F>(&mut self, callback: F)
       where
           F: Fn() + Send + Sync + 'static,
       {
           self.on_create.push(Box::new(callback));
       }
       
       pub fn on_resume<F>(&mut self, callback: F)
       where
           F: Fn() + Send + Sync + 'static,
       {
           self.on_resume.push(Box::new(callback));
       }
       
       // Similar for other lifecycle events...
       
       /// Transition to new state
       pub fn transition_to(&mut self, new_state: LifecycleState) {
           tracing::info!("Lifecycle transition: {:?} -> {:?}", self.state, new_state);
           
           match new_state {
               LifecycleState::Created => {
                   for callback in &self.on_create {
                       callback();
                   }
               }
               LifecycleState::Started => {
                   for callback in &self.on_start {
                       callback();
                   }
               }
               LifecycleState::Resumed => {
                   for callback in &self.on_resume {
                       callback();
                   }
               }
               LifecycleState::Paused => {
                   for callback in &self.on_pause {
                       callback();
                   }
               }
               LifecycleState::Stopped => {
                   for callback in &self.on_stop {
                       callback();
                   }
               }
               LifecycleState::Destroyed => {
                   for callback in &self.on_destroy {
                       callback();
                   }
               }
               _ => {}
           }
           
           self.state = new_state;
       }
       
       pub fn state(&self) -> LifecycleState {
           self.state
       }
   }
   ```

**Критерии завершения**:
- [ ] AppContext provides dependencies
- [ ] Lifecycle transitions work
- [ ] Lifecycle callbacks triggered
- [ ] 15+ lifecycle tests

---

#### День 3: Window Manager

**Цель**: Multi-window management

**Задачи**:

1. **Создать `app/window_manager.rs`**
   ```rust
   use dashmap::DashMap;
   
   /// Window manager
   ///
   /// Manages all application windows.
   pub struct WindowManager {
       /// Platform reference
       platform: Arc<dyn Platform>,
       
       /// Rendering engine
       engine: Arc<Engine>,
       
       /// Event dispatcher
       dispatcher: Arc<EventRouter>,
       
       /// Active windows
       windows: Arc<DashMap<WindowId, Arc<Window>>>,
   }
   
   impl WindowManager {
       pub fn new(
           platform: Arc<dyn Platform>,
           engine: Arc<Engine>,
           dispatcher: Arc<EventRouter>,
       ) -> Self {
           Self {
               platform,
               engine,
               dispatcher,
               windows: Arc::new(DashMap::new()),
           }
       }
       
       /// Create a new window
       pub fn create_window(&self, options: WindowOptions) -> Result<WindowHandle, AppError> {
           tracing::info!("Creating window: {}", options.title);
           
           // Create platform window
           let platform_window = self.platform.create_window(options.clone())?;
           let window_id = platform_window.id();
           
           // Create engine surface
           self.engine.create_surface(platform_window.as_ref())?;
           
           // Create application window
           let window = Arc::new(Window::new(
               window_id,
               platform_window,
               Arc::clone(&self.engine),
               Arc::clone(&self.dispatcher),
               options,
           ));
           
           // Register window
           self.windows.insert(window_id, Arc::clone(&window));
           
           tracing::info!("Window created: {:?}", window_id);
           
           Ok(WindowHandle { id: window_id })
       }
       
       /// Get window by ID
       pub fn get_window(&self, id: WindowId) -> Option<Arc<Window>> {
           self.windows.get(&id).map(|w| Arc::clone(&*w))
       }
       
       /// Get all windows
       pub fn windows(&self) -> Vec<WindowHandle> {
           self.windows.iter()
               .map(|entry| WindowHandle { id: *entry.key() })
               .collect()
       }
       
       /// Close a window
       pub fn close_window(&self, id: WindowId) -> Result<(), AppError> {
           if let Some((_, window)) = self.windows.remove(&id) {
               window.close();
               tracing::info!("Window closed: {:?}", id);
               Ok(())
           } else {
               Err(AppError::WindowNotFound(id))
           }
       }
       
       /// Close all windows
       pub fn close_all_windows(&self) {
           for entry in self.windows.iter() {
               entry.value().close();
           }
           self.windows.clear();
       }
   }
   ```

2. **Window Struct (создать `app/window.rs`)**
   ```rust
   /// Application window
   pub struct Window {
       /// Window ID
       id: WindowId,
       
       /// Platform window
       platform_window: Arc<dyn PlatformWindow>,
       
       /// Rendering engine
       engine: Arc<Engine>,
       
       /// Event dispatcher
       dispatcher: Arc<EventRouter>,
       
       /// Window options
       options: WindowOptions,
       
       /// Window state
       state: Arc<RwLock<WindowState>>,
   }
   
   struct WindowState {
       visible: bool,
       focused: bool,
       size: Size<f32, PhysicalPixels>,
       position: Point<f32, PhysicalPixels>,
   }
   
   impl Window {
       pub fn new(
           id: WindowId,
           platform_window: Arc<dyn PlatformWindow>,
           engine: Arc<Engine>,
           dispatcher: Arc<EventRouter>,
           options: WindowOptions,
       ) -> Self {
           // Register event handlers
           Self::setup_event_handlers(&platform_window, &dispatcher);
           
           Self {
               id,
               platform_window,
               engine,
               dispatcher,
               options,
               state: Arc::new(RwLock::new(WindowState {
                   visible: true,
                   focused: false,
                   size: options.size.cast_unit(),
                   position: Point::zero(),
               })),
           }
       }
       
       fn setup_event_handlers(
           platform_window: &Arc<dyn PlatformWindow>,
           dispatcher: &Arc<EventRouter>,
       ) {
           let dispatcher = Arc::clone(dispatcher);
           
           // Resize event
           platform_window.on_resize(Box::new(move |size| {
               tracing::debug!("Window resized: {:?}", size);
               // TODO: Trigger repaint
           }));
           
           // Close event
           platform_window.on_close_requested(Box::new(|| {
               tracing::info!("Window close requested");
               true // Allow close
           }));
       }
       
       pub fn id(&self) -> WindowId {
           self.id
       }
       
       pub fn title(&self) -> String {
           self.platform_window.title()
       }
       
       pub fn set_title(&self, title: &str) {
           self.platform_window.set_title(title);
       }
       
       pub fn size(&self) -> Size<f32, PhysicalPixels> {
           self.state.read().size
       }
       
       pub fn set_size(&self, size: Size<f32, PhysicalPixels>) {
           self.platform_window.set_size(size);
           self.state.write().size = size;
       }
       
       pub fn show(&self) {
           self.platform_window.set_visible(true);
           self.state.write().visible = true;
       }
       
       pub fn hide(&self) {
           self.platform_window.set_visible(false);
           self.state.write().visible = false;
       }
       
       pub fn close(&self) {
           self.platform_window.close();
       }
       
       /// Render a scene to this window
       pub fn render(&self, scene: &Scene) -> Result<(), AppError> {
           self.engine.render(self.id, scene)
               .map_err(AppError::from)
       }
   }
   
   /// Safe handle to a window
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct WindowHandle {
       pub id: WindowId,
   }
   
   /// Window configuration options
   #[derive(Clone, Debug)]
   pub struct WindowOptions {
       pub title: String,
       pub size: Size<f32, LogicalPixels>,
       pub position: Option<Point<f32, PhysicalPixels>>,
       pub resizable: bool,
       pub decorations: bool,
       pub transparent: bool,
       pub visible: bool,
   }
   
   impl Default for WindowOptions {
       fn default() -> Self {
           Self {
               title: "FLUI Application".to_string(),
               size: Size::new(800.0, 600.0),
               position: None,
               resizable: true,
               decorations: true,
               transparent: false,
               visible: true,
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] WindowManager creates windows
- [ ] Multi-window support works
- [ ] Window lifecycle events work
- [ ] 25+ window management tests

---

### Этап 4.2: Bindings Integration (Неделя 7-8, Дни 4-7)

#### День 4: WidgetsFlutterBinding

**Цель**: Combine all bindings (Flutter pattern)

**Референсы**:
- Flutter's `WidgetsFlutterBinding`
- Plan `3.5` - flui_app architecture

**Задачи**:

1. **Создать `bindings/widgets_flutter_binding.rs`**
   ```rust
   use once_cell::sync::OnceCell;
   
   /// Main binding that combines all other bindings
   ///
   /// This is the Flutter equivalent of WidgetsFlutterBinding.
   /// It coordinates:
   /// - SchedulerBinding (frame scheduling)
   /// - GestureBinding (input events)
   /// - RendererBinding (rendering)
   /// - WidgetsBinding (widget tree)
   pub struct WidgetsFlutterBinding {
       /// Application context
       context: Arc<AppContext>,
       
       /// Scheduler for frame callbacks
       scheduler: Arc<Scheduler>,
       
       /// Build owner for widget tree
       build_owner: Arc<BuildOwner>,
       
       /// Pipeline owner for render tree
       pipeline_owner: Arc<PipelineOwner>,
   }
   
   impl WidgetsFlutterBinding {
       /// Get or create global instance
       pub fn ensure_initialized(context: Arc<AppContext>) -> Arc<Self> {
           static INSTANCE: OnceCell<Arc<WidgetsFlutterBinding>> = OnceCell::new();
           
           INSTANCE.get_or_init(|| {
               Arc::new(Self::new(context))
           }).clone()
       }
       
       /// Create new binding
       fn new(context: Arc<AppContext>) -> Self {
           let scheduler = Arc::new(Scheduler::new());
           let build_owner = Arc::new(BuildOwner::new());
           let pipeline_owner = Arc::new(PipelineOwner::new());
           
           Self {
               context,
               scheduler,
               build_owner,
               pipeline_owner,
           }
       }
       
       /// Schedule a frame
       pub fn schedule_frame(&self) {
           self.scheduler.schedule_frame();
       }
       
       /// Attach root widget
       pub fn attach_root_widget<V: View>(&self, view: V) {
           let element = self.build_owner.build_scope(|| {
               view.create_element()
           });
           
           self.build_owner.set_root(element);
       }
       
       /// Run a frame (build → layout → paint → composite)
       pub fn draw_frame(&self, window_id: WindowId) -> Result<(), AppError> {
           // 1. Build phase
           self.build_owner.build_dirty_elements();
           
           // 2. Layout phase
           self.pipeline_owner.flush_layout();
           
           // 3. Paint phase
           self.pipeline_owner.flush_paint();
           
           // 4. Composite phase
           let scene = self.pipeline_owner.composite_scene();
           
           // 5. Render to window
           if let Some(window) = self.context.window_manager.get_window(window_id) {
               window.render(&scene)?;
           }
           
           Ok(())
       }
       
       /// Handle input event
       pub fn handle_event(&self, event: Event) {
           self.context.dispatcher.route_event(&event, &root);
       }
   }
   ```

2. **Scheduler Integration (обновить `bindings/scheduler_binding.rs`)**
   ```rust
   use flui_scheduler::*;
   
   /// Scheduler binding
   ///
   /// Manages frame scheduling and callbacks.
   pub struct SchedulerBinding {
       scheduler: Arc<Scheduler>,
       frame_callbacks: Arc<RwLock<Vec<FrameCallback>>>,
   }
   
   type FrameCallback = Box<dyn Fn(Duration) + Send + Sync>;
   
   impl SchedulerBinding {
       pub fn new(scheduler: Arc<Scheduler>) -> Self {
           Self {
               scheduler,
               frame_callbacks: Arc::new(RwLock::new(Vec::new())),
           }
       }
       
       /// Schedule a frame
       pub fn schedule_frame(&self) {
           self.scheduler.schedule_frame();
       }
       
       /// Add frame callback (requestAnimationFrame equivalent)
       pub fn add_post_frame_callback<F>(&self, callback: F)
       where
           F: Fn(Duration) + Send + Sync + 'static,
       {
           self.frame_callbacks.write().push(Box::new(callback));
       }
       
       /// Execute frame callbacks
       pub fn handle_begin_frame(&self, elapsed: Duration) {
           let callbacks = self.frame_callbacks.read();
           
           for callback in callbacks.iter() {
               callback(elapsed);
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] WidgetsFlutterBinding combines bindings
- [ ] Frame scheduling works
- [ ] Build → Layout → Paint pipeline works
- [ ] 20+ binding tests

---

#### День 5: Event Loop Integration

**Цель**: Connect platform events to bindings

**Задачи**:

1. **Event Loop Runner (создать `app/runner.rs`)**
   ```rust
   /// Application runner
   ///
   /// Runs the main event loop and coordinates frame rendering.
   pub struct AppRunner {
       context: Arc<AppContext>,
       binding: Arc<WidgetsFlutterBinding>,
       frame_scheduler: Arc<FrameScheduler>,
   }
   
   impl AppRunner {
       pub fn new(
           context: Arc<AppContext>,
           binding: Arc<WidgetsFlutterBinding>,
       ) -> Self {
           let frame_scheduler = Arc::new(FrameScheduler::new());
           
           Self {
               context,
               binding,
               frame_scheduler,
           }
       }
       
       /// Run the event loop
       pub fn run(self) -> ! {
           let context = Arc::clone(&self.context);
           let binding = Arc::clone(&self.binding);
           let frame_scheduler = Arc::clone(&self.frame_scheduler);
           
           // Platform event loop
           context.platform.run(Box::new(move || {
               tracing::info!("Event loop started");
               
               // Register frame callback
               frame_scheduler.on_frame(move |elapsed| {
                   // Draw all windows
                   for window_handle in context.windows() {
                       if let Err(e) = binding.draw_frame(window_handle.id) {
                           tracing::error!("Frame render failed: {}", e);
                       }
                   }
               });
               
               // Request initial frame
               binding.schedule_frame();
           }));
           
           unreachable!()
       }
   }
   
   /// Frame scheduler
   struct FrameScheduler {
       on_frame: Arc<RwLock<Option<Box<dyn Fn(Duration) + Send + Sync>>>>,
       last_frame_time: Arc<RwLock<Option<Instant>>>,
   }
   
   impl FrameScheduler {
       fn new() -> Self {
           Self {
               on_frame: Arc::new(RwLock::new(None)),
               last_frame_time: Arc::new(RwLock::new(None)),
           }
       }
       
       fn on_frame<F>(&self, callback: F)
       where
           F: Fn(Duration) + Send + Sync + 'static,
       {
           *self.on_frame.write() = Some(Box::new(callback));
       }
       
       fn trigger_frame(&self) {
           let now = Instant::now();
           let elapsed = {
               let mut last = self.last_frame_time.write();
               let elapsed = last.map(|t| now.duration_since(t))
                   .unwrap_or(Duration::ZERO);
               *last = Some(now);
               elapsed
           };
           
           if let Some(callback) = &*self.on_frame.read() {
               callback(elapsed);
           }
       }
   }
   ```

2. **Platform Event Handling**
   ```rust
   impl AppRunner {
       fn setup_event_handlers(&self) {
           let binding = Arc::clone(&self.binding);
           
           // Handle platform events
           self.context.platform.on_event(Box::new(move |event| {
               match event {
                   PlatformEvent::WindowResized { window_id, size } => {
                       // Request repaint
                       binding.schedule_frame();
                   }
                   
                   PlatformEvent::PointerEvent(pointer_event) => {
                       binding.handle_event(Event::Pointer(pointer_event));
                   }
                   
                   PlatformEvent::KeyboardEvent(keyboard_event) => {
                       binding.handle_event(Event::Keyboard(keyboard_event));
                   }
                   
                   _ => {}
               }
           }));
       }
   }
   ```

**Критерии завершения**:
- [ ] Event loop runs
- [ ] Platform events routed to bindings
- [ ] Frame rendering triggered correctly
- [ ] 15+ event loop tests

---

#### День 6: Frame Callbacks & Animation Support

**Цель**: requestAnimationFrame equivalent

**Задачи**:

1. **Frame Callback System**
   ```rust
   impl WidgetsFlutterBinding {
       /// Add persistent frame callback (like requestAnimationFrame)
       pub fn add_persistent_frame_callback<F>(&self, callback: F) -> CallbackHandle
       where
           F: Fn(Duration) + Send + Sync + 'static,
       {
           let id = CallbackHandle::new();
           self.persistent_callbacks.write().insert(id, Box::new(callback));
           id
       }
       
       /// Remove persistent callback
       pub fn remove_persistent_frame_callback(&self, handle: CallbackHandle) {
           self.persistent_callbacks.write().remove(&handle);
       }
       
       /// Add one-shot callback (fires once)
       pub fn add_post_frame_callback<F>(&self, callback: F)
       where
           F: FnOnce(Duration) + Send + 'static,
       {
           self.post_frame_callbacks.write().push(Box::new(callback));
       }
   }
   
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct CallbackHandle(u64);
   
   impl CallbackHandle {
       fn new() -> Self {
           use std::sync::atomic::{AtomicU64, Ordering};
           static NEXT_ID: AtomicU64 = AtomicU64::new(1);
           Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
       }
   }
   ```

2. **Animation Ticker (bonus)**
   ```rust
   /// Animation ticker (fires on every frame)
   pub struct Ticker {
       binding: Arc<WidgetsFlutterBinding>,
       callback_handle: Option<CallbackHandle>,
       on_tick: Arc<RwLock<Option<Box<dyn Fn(Duration) + Send + Sync>>>>,
   }
   
   impl Ticker {
       pub fn new(binding: Arc<WidgetsFlutterBinding>) -> Self {
           Self {
               binding,
               callback_handle: None,
               on_tick: Arc::new(RwLock::new(None)),
           }
       }
       
       pub fn start<F>(&mut self, on_tick: F)
       where
           F: Fn(Duration) + Send + Sync + 'static,
       {
           *self.on_tick.write() = Some(Box::new(on_tick));
           
           let on_tick = Arc::clone(&self.on_tick);
           self.callback_handle = Some(
               self.binding.add_persistent_frame_callback(move |elapsed| {
                   if let Some(callback) = &*on_tick.read() {
                       callback(elapsed);
                   }
               })
           );
       }
       
       pub fn stop(&mut self) {
           if let Some(handle) = self.callback_handle.take() {
               self.binding.remove_persistent_frame_callback(handle);
           }
       }
   }
   
   impl Drop for Ticker {
       fn drop(&mut self) {
           self.stop();
       }
   }
   ```

**Критерии завершения**:
- [ ] Frame callbacks work
- [ ] Persistent callbacks work
- [ ] One-shot callbacks work
- [ ] Ticker for animations
- [ ] 20+ callback tests

---

#### День 7: Error Handling & Logging

**Цель**: Comprehensive error handling

**Задачи**:

1. **Error Types (создать `app/error.rs`)**
   ```rust
   use thiserror::Error;
   
   /// Application error
   #[derive(Debug, Error)]
   pub enum AppError {
       #[error("Platform error: {0}")]
       Platform(#[from] flui_platform::PlatformError),
       
       #[error("Rendering error: {0}")]
       Rendering(#[from] flui_engine::RenderError),
       
       #[error("Window not found: {0:?}")]
       WindowNotFound(WindowId),
       
       #[error("Invalid configuration: {0}")]
       InvalidConfig(String),
       
       #[error("Lifecycle error: {0}")]
       Lifecycle(String),
   }
   
   pub type AppResult<T> = Result<T, AppError>;
   ```

2. **Logging Setup**
   ```rust
   impl Application {
       fn setup_logging(config: &AppConfig) {
           if !config.enable_logging {
               return;
           }
           
           tracing_subscriber::fmt()
               .with_max_level(match config.log_level {
                   flui_log::Level::Trace => tracing::Level::TRACE,
                   flui_log::Level::Debug => tracing::Level::DEBUG,
                   flui_log::Level::Info => tracing::Level::INFO,
                   flui_log::Level::Warn => tracing::Level::WARN,
                   flui_log::Level::Error => tracing::Level::ERROR,
               })
               .with_target(false)
               .with_thread_ids(true)
               .with_file(true)
               .with_line_number(true)
               .init();
           
           tracing::info!("Logging initialized at level: {:?}", config.log_level);
       }
   }
   ```

**Критерии завершения**:
- [ ] Error types comprehensive
- [ ] Error messages helpful
- [ ] Logging works
- [ ] 15+ error handling tests

---

### Этап 4.3: Polish & Examples (Неделя 8, Дни 8-10)

#### День 8: Debug Overlays (Bonus)

**Цель**: Development tools

**Задачи**:

1. **FPS Counter**
   ```rust
   pub struct FpsCounter {
       frame_times: VecDeque<Instant>,
       max_samples: usize,
   }
   
   impl FpsCounter {
       pub fn new() -> Self {
           Self {
               frame_times: VecDeque::new(),
               max_samples: 60,
           }
       }
       
       pub fn record_frame(&mut self) {
           let now = Instant::now();
           self.frame_times.push_back(now);
           
           if self.frame_times.len() > self.max_samples {
               self.frame_times.pop_front();
           }
       }
       
       pub fn fps(&self) -> f32 {
           if self.frame_times.len() < 2 {
               return 0.0;
           }
           
           let first = self.frame_times.front().unwrap();
           let last = self.frame_times.back().unwrap();
           let duration = last.duration_since(*first);
           
           if duration.is_zero() {
               return 0.0;
           }
           
           (self.frame_times.len() - 1) as f32 / duration.as_secs_f32()
       }
   }
   ```

2. **Debug Overlay**
   ```rust
   pub struct DebugOverlay {
       fps_counter: FpsCounter,
       visible: bool,
   }
   
   impl DebugOverlay {
       pub fn render(&self, scene_builder: &mut SceneBuilder) {
           if !self.visible {
               return;
           }
           
           let fps = self.fps_counter.fps();
           
           scene_builder.push_layer()
               .add_text(
                   format!("FPS: {:.1}", fps),
                   Point::new(10.0, 10.0),
                   TextStyle {
                       font_size: 14.0,
                       color: Color::WHITE,
                       ..Default::default()
                   },
               );
       }
   }
   ```

**Критерии завершения**:
- [ ] FPS counter works
- [ ] Debug overlay renders
- [ ] Toggle debug mode works

---

#### День 9: Example Applications

**Цель**: Production examples

**Задачи**:

1. **Hello World Example**
   ```rust
   // examples/hello_world.rs
   use flui_app::prelude::*;
   
   fn main() {
       run_app(|ctx| {
           println!("Hello, FLUI!");
           
           // Create a simple window
           let window = ctx.create_window(WindowOptions {
               title: "Hello World".to_string(),
               size: Size::new(400.0, 300.0),
               ..Default::default()
           }).unwrap();
           
           println!("Window created: {:?}", window.id);
       });
   }
   ```

2. **Counter Example**
   ```rust
   // examples/counter.rs
   use flui_app::prelude::*;
   
   fn main() {
       run_app(|ctx| {
           let window = ctx.create_window(WindowOptions::default()).unwrap();
           
           // TODO: Attach counter widget when flui_widgets is ready
           println!("Counter app running");
       });
   }
   ```

3. **Multi-Window Example**
   ```rust
   // examples/multi_window.rs
   use flui_app::prelude::*;
   
   fn main() {
       run_app(|ctx| {
           // Create multiple windows
           for i in 0..3 {
               ctx.create_window(WindowOptions {
                   title: format!("Window {}", i + 1),
                   size: Size::new(300.0, 200.0),
                   position: Some(Point::new(100.0 + i as f32 * 320.0, 100.0)),
                   ..Default::default()
               }).unwrap();
           }
           
           println!("Created 3 windows");
       });
   }
   ```

**Критерии завершения**:
- [ ] Examples compile
- [ ] Examples run
- [ ] Examples demonstrate features

---

#### День 10: Integration Testing & Documentation

**Цель**: Production readiness

**Задачи**:

1. **Integration Tests**
   ```rust
   #[test]
   fn test_full_application_lifecycle() {
       std::env::set_var("FLUI_HEADLESS", "1");
       
       pollster::block_on(async {
           let app = Application::builder()
               .platform(flui_platform::headless_platform())
               .build()
               .await
               .unwrap();
           
           // Test lifecycle
           assert_eq!(app.lifecycle.read().state(), LifecycleState::Uninitialized);
           
           // Create window
           let window = app.context.create_window(WindowOptions::default()).unwrap();
           assert_eq!(app.context.windows().len(), 1);
           
           // Close window
           app.context.window_manager.close_window(window.id).unwrap();
           assert_eq!(app.context.windows().len(), 0);
       });
   }
   ```

2. **Documentation**
   - README.md для flui_app
   - Architecture diagram
   - API docs для всех public items
   - Examples documentation

**Критерии завершения**:
- [ ] All tests pass
- [ ] cargo doc builds
- [ ] README complete
- [ ] Architecture documented

---

## Критерии Завершения Phase 4

### Обязательные Требования

- [ ] **flui_app 0.1.0**
  - [ ] Application builder pattern works
  - [ ] run_app() convenience function
  - [ ] AppContext dependency injection
  - [ ] Full lifecycle management (onCreate → onDestroy)
  - [ ] Multi-window support
  - [ ] WidgetsFlutterBinding combines all bindings
  - [ ] Event loop integration
  - [ ] Frame callbacks (requestAnimationFrame)
  - [ ] 150+ application tests
  - [ ] All examples run successfully

### Бонусные Цели

- [ ] Hot reload support (development mode)
- [ ] Debug overlays (FPS, memory usage)
- [ ] Performance profiling hooks
- [ ] Crash reporting

---

## Примеры Использования

### Example 1: Hello World

```rust
use flui_app::run_app;

fn main() {
    run_app(|ctx| {
        println!("App ready!");
    });
}
```

### Example 2: Custom Configuration

```rust
use flui_app::*;

fn main() {
    pollster::block_on(async {
        let app = Application::builder()
            .title("My App")
            .size(1024, 768)
            .debug_mode(true)
            .log_level(flui_log::Level::Debug)
            .on_ready(|ctx| {
                println!("App is ready!");
            })
            .build()
            .await
            .unwrap();
        
        app.run()
    })
}
```

### Example 3: Multi-Window

```rust
use flui_app::*;

fn main() {
    run_app(|ctx| {
        // Main window
        ctx.create_window(WindowOptions {
            title: "Main".to_string(),
            size: Size::new(800.0, 600.0),
            ..Default::default()
        }).unwrap();
        
        // Settings window
        ctx.create_window(WindowOptions {
            title: "Settings".to_string(),
            size: Size::new(400.0, 300.0),
            ..Default::default()
        }).unwrap();
    });
}
```

---

## Troubleshooting Guide

### Issue: Application не запускается

**Solution**:
```rust
// Check platform initialization
std::env::set_var("FLUI_HEADLESS", "1"); // For testing

// Check async runtime
pollster::block_on(async {
    let app = Application::builder().build().await?;
});
```

### Issue: Window не появляется

**Solution**:
```rust
// Ensure window is visible
let options = WindowOptions {
    visible: true,
    ..Default::default()
};
```

### Issue: Events не обрабатываются

**Solution**:
```rust
// Check event loop is running
// Platform.run() должен быть вызван
app.run(); // Never returns
```

---

## Следующие Шаги (Production)

После завершения Phase 4:

1. **flui_widgets** - Widget library (Button, Text, Container, etc.)
2. **Production Apps** - Real applications
3. **Performance Optimization** - Profiling и optimization
4. **Documentation** - User guides, tutorials
5. **Publishing** - Crate releases на crates.io

---

**Статус**: 🟡 Ready for Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базируется на**: All previous phase plans + original architecture design

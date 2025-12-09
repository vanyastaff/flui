//! Flui Application Framework
//!
//! This crate provides the application framework for FLUI, featuring a Flutter-inspired
//! architecture with pure wgpu rendering.
//!
//! # Architecture
//!
//! ```text
//! User App
//!    ↓
//! runApp(root_widget)
//!    ↓
//! AppBinding::ensure_initialized()
//!    ├─ EventRouter (shared with platform layer)
//!    ├─ SchedulerBinding (wraps flui-scheduler)
//!    │   ├─ TaskQueue (priority-based task execution)
//!    │   ├─ FrameBudget (60fps timing, phase statistics)
//!    │   └─ VSync coordination
//!    ├─ RendererBinding (rendering)
//!    └─ PipelineOwner (element tree management)
//!    ↓
//! DesktopEmbedder::new() (flui-platform)
//!    ├─ Create winit window
//!    ├─ Initialize GpuRenderer (encapsulates ALL wgpu resources)
//!    ├─ EmbedderCore (shared cross-platform logic)
//!    │   └─ GestureBinding (type-safe hit testing)
//!    └─ Setup event routing
//!    ↓
//! Event Loop (winit)
//!    ├─ Window events → EmbedderCore → GestureBinding → EventRouter
//!    ├─ VSync → begin_frame() → FrameCallbacks
//!    ├─ Build → Layout → Paint → Scene (flui_engine)
//!    ├─ Render → GpuRenderer → GPU
//!    └─ end_frame() → PostFrameCallbacks
//! ```
//!
//! # Performance Monitoring
//!
//! Access production-ready frame statistics via `AppBinding`:
//!
//! ```rust,ignore
//! let binding = AppBinding::ensure_initialized();
//!
//! // Access frame budget and statistics
//! let budget = binding.frame_budget();
//! let stats = budget.lock().phase_stats();
//!
//! println!("Build: {:.2}ms", stats.build_ms);
//! println!("Layout: {:.2}ms", stats.layout_ms);
//! println!("Paint: {:.2}ms", stats.paint_ms);
//! println!("Average frame: {:.2}ms", budget.lock().average_frame_time());
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::run_app;
//! use flui_core::view::View;
//! use flui_widgets::*;
//!
//! #[derive(Debug)]
//! struct Counter {
//!     initial: i32,
//! }
//!
//! impl View for Counter {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         let count = use_signal(ctx, self.initial);
//!
//!         Column::new()
//!             .children(vec![
//!                 Text::new(format!("Count: {}", count.get())).into(),
//!                 Button::new("Increment")
//!                     .on_pressed(move || count.update(|n| n + 1))
//!                     .into(),
//!             ])
//!     }
//! }
//!
//! fn main() {
//!     run_app(Counter { initial: 0 });
//! }
//! ```
//!
//! # Modules
//!
//! - **binding**: Framework bindings (gesture, scheduler, renderer, widgets)
//! - **embedder**: Platform integration (wgpu, winit)

pub mod binding;
pub mod embedder;

// Re-exports for convenience
pub use binding::AppBinding;
pub use embedder::WgpuEmbedder;

// Re-export commonly used types from flui_core
pub use flui_core::{
    // Element system
    element::Element,

    // View system (new API)
    view::{BuildContext, StatelessView},
    // Foundation types (re-exported directly from flui_foundation)
    ElementId,
    Key,
    Slot,
};

use winit::event_loop::EventLoop;

/// Run a FLUI app
///
/// This is the main entry point for FLUI applications.
/// It initializes the framework bindings, creates a window, and starts the event loop.
///
/// # Parameters
///
/// - `app`: The root widget (typically an App or MaterialApp)
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::run_app;
/// use flui_widgets::*;
///
/// #[derive(Debug)]
/// struct MyApp;
///
/// impl View for MyApp {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Hello, FLUI!")
///     }
/// }
///
/// fn main() {
///     run_app(MyApp);
/// }
/// ```
///
/// # Panics
///
/// Panics if:
/// - Window creation fails
/// - GPU initialization fails
/// - Root widget has already been attached
#[cfg(not(target_os = "android"))]
pub fn run_app<V>(app: V) -> !
where
    V: flui_core::view::StatelessView + Clone + Sync,
{
    use crate::embedder::DesktopEmbedder;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::ActiveEventLoop;
    use winit::window::WindowId;

    // Initialize cross-platform logging
    // Use RUST_LOG env var or default filter
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,wgpu=warn,flui_core=debug,flui_app=debug,flui_platform=debug".to_string()
    });
    let level = if filter.contains("trace") {
        tracing::Level::TRACE
    } else {
        tracing::Level::DEBUG
    };
    flui_log::Logger::default()
        .with_filter(&filter)
        .with_level(level)
        .init();

    tracing::info!("FLUI application starting");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();
    tracing::debug!("Framework bindings initialized");

    // 2. Attach root widget
    binding.attach_root_widget(app);

    // Application state for winit 0.30+ ApplicationHandler
    struct AppState {
        binding: std::sync::Arc<AppBinding>,
        embedder: Option<DesktopEmbedder>,
    }

    impl ApplicationHandler for AppState {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.embedder.is_none() {
                let embedder = pollster::block_on(DesktopEmbedder::new(
                    self.binding.pipeline(),
                    self.binding.needs_redraw_flag(),
                    self.binding.scheduler.scheduler_arc(),
                    self.binding.event_router(),
                    event_loop,
                ))
                .expect("Failed to create desktop embedder");
                self.embedder = Some(embedder);
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            // On-demand rendering: only redraw when dirty or has pending rebuilds
            if let Some(ref emb) = self.embedder {
                // Check if needs_redraw OR if there are pending signal updates
                let has_pending = {
                    let pipeline = self.binding.pipeline();
                    let owner = pipeline.read();
                    owner.has_pending_rebuilds()
                };

                if self.binding.needs_redraw() || has_pending {
                    emb.winit_window().request_redraw();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            if let Some(ref mut emb) = self.embedder {
                match event {
                    WindowEvent::RedrawRequested => {
                        emb.render_frame();
                        self.binding.mark_rendered();
                    }
                    WindowEvent::CloseRequested => {
                        event_loop.exit();
                    }
                    other => {
                        emb.handle_window_event(other, event_loop);
                    }
                }
            }
        }
    }

    // 3. Create event loop
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    // 4. Create app state and run
    let mut app_state = AppState {
        binding,
        embedder: None,
    };

    event_loop
        .run_app(&mut app_state)
        .expect("Event loop error");

    // Unreachable, but needed to satisfy return type !
    std::process::exit(0)
}

/// Run a FLUI application with an IntoElement root.
///
/// This is useful for render-only views that implement IntoElement
/// but not StatelessView (like Text, Container, etc.).
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::run_app_element;
/// use flui_widgets::Text;
///
/// fn main() {
///     run_app_element(Text::headline("Hello, FLUI!"));
/// }
/// ```
#[cfg(not(target_os = "android"))]
pub fn run_app_element<E>(element: E) -> !
where
    E: flui_core::IntoElement,
{
    use crate::embedder::DesktopEmbedder;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::ActiveEventLoop;
    use winit::window::WindowId;

    // Initialize cross-platform logging
    // Use RUST_LOG env var or default filter
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,wgpu=warn,flui_core=debug,flui_app=debug,flui_platform=debug".to_string()
    });
    let level = if filter.contains("trace") {
        tracing::Level::TRACE
    } else {
        tracing::Level::DEBUG
    };
    flui_log::Logger::default()
        .with_filter(&filter)
        .with_level(level)
        .init();

    tracing::info!("FLUI application starting");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();
    tracing::debug!("Framework bindings initialized");

    // 2. Attach root element
    binding.attach_root_element(element);

    // Verify tree initialization
    {
        let pipeline = binding.pipeline();
        let owner = pipeline.read();
        if owner.root_element_id().is_none() {
            tracing::warn!("No root element found after attach!");
        }
    }

    // Application state for winit 0.30+ ApplicationHandler
    struct AppState {
        binding: std::sync::Arc<AppBinding>,
        embedder: Option<DesktopEmbedder>,
    }

    impl ApplicationHandler for AppState {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.embedder.is_none() {
                let embedder = pollster::block_on(DesktopEmbedder::new(
                    self.binding.pipeline(),
                    self.binding.needs_redraw_flag(),
                    self.binding.scheduler.scheduler_arc(),
                    self.binding.event_router(),
                    event_loop,
                ))
                .expect("Failed to create desktop embedder");
                self.embedder = Some(embedder);
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(ref emb) = self.embedder {
                let has_pending = {
                    let pipeline = self.binding.pipeline();
                    let owner = pipeline.read();
                    owner.has_pending_rebuilds()
                };

                if self.binding.needs_redraw() || has_pending {
                    emb.winit_window().request_redraw();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            if let Some(ref mut emb) = self.embedder {
                match event {
                    WindowEvent::RedrawRequested => {
                        emb.render_frame();
                        self.binding.mark_rendered();
                    }
                    WindowEvent::CloseRequested => {
                        event_loop.exit();
                    }
                    other => {
                        emb.handle_window_event(other, event_loop);
                    }
                }
            }
        }
    }

    // 3. Create event loop
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    // 4. Create app state and run
    let mut app_state = AppState {
        binding,
        embedder: None,
    };

    event_loop
        .run_app(&mut app_state)
        .expect("Event loop error");

    std::process::exit(0)
}

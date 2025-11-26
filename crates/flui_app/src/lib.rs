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
//!    ├─ GestureBinding (EventRouter integration)
//!    ├─ SchedulerBinding (wraps flui-scheduler)
//!    │   ├─ TaskQueue (priority-based task execution)
//!    │   ├─ FrameBudget (60fps timing, phase statistics)
//!    │   └─ VSync coordination
//!    ├─ RendererBinding (rendering)
//!    └─ PipelineBinding (pipeline and element tree management)
//!    ↓
//! WgpuEmbedder::new()
//!    ├─ Create winit window
//!    ├─ Initialize GpuRenderer (encapsulates ALL wgpu resources)
//!    └─ Setup event routing
//!    ↓
//! Event Loop (winit)
//!    ├─ Window events → GestureBinding → EventRouter
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

// Supporting modules for window event handling
pub mod event_callbacks;
pub mod window_state;

// Platform providers
pub mod providers;

// Error handling
pub mod error_handling;

// Application widgets
mod root_widget;
pub mod widgets_app;

// Re-export public app widgets
pub use widgets_app::{TextDirection, WidgetsApp};

// Re-export platform providers
pub use providers::{MediaQueryData, MediaQueryProvider};

// Re-export error handling types
pub use error_handling::{ErrorBoundary, ErrorInfo, ErrorWidget};

// Re-exports for convenience
pub use binding::AppBinding;
pub use embedder::WgpuEmbedder;

// Re-export commonly used types from flui_core
pub use flui_core::{
    // View system (now at top level)
    BuildContext,
    // Element system (now at top level)
    Element,

    // Foundation types (now at top level)
    ElementId,
    Key,
    Slot,
};

// StatelessView is in flui-view
pub use flui_view::StatelessView;

use winit::event_loop::EventLoop;

/// Run a FLUI app
///
/// This is the main entry point for FLUI applications.
/// It initializes the framework bindings, wraps your app in RootWidget,
/// creates a window, and starts the event loop.
///
/// # Parameters
///
/// - `app`: The root widget (typically WidgetsApp or MaterialApp)
///
/// # Recommended Usage
///
/// Use `WidgetsApp` to get framework services (MediaQuery, Navigator, etc.):
///
/// ```rust,ignore
/// use flui_app::{run_app, WidgetsApp};
/// use flui_widgets::*;
///
/// #[derive(Debug, Clone)]
/// struct Home;
///
/// impl StatelessView for Home {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Hello, FLUI!")
///     }
/// }
///
/// fn main() {
///     run_app(WidgetsApp::new(Home).title("My App"));
/// }
/// ```
///
/// # Direct Usage (without WidgetsApp)
///
/// You can also pass any StatelessView directly:
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct SimpleApp;
///
/// impl StatelessView for SimpleApp {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Minimal app")
///     }
/// }
///
/// fn main() {
///     run_app(SimpleApp);
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
    V: flui_view::StatelessView + Clone + Sync,
{
    use crate::embedder::DesktopEmbedder;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::ActiveEventLoop;
    use winit::window::WindowId;

    // Initialize cross-platform logging
    flui_log::Logger::default()
        .with_filter("info,wgpu=warn,flui_core=debug,flui_app=info,counter=debug")
        .init();

    let _app_span = tracing::info_span!("flui_app").entered();
    tracing::info!("Starting FLUI app");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();

    // 2. Wrap user app in RootWidget (internal framework wrapper)
    let root = root_widget::RootWidget::new(app);

    // 3. Attach root widget to pipeline
    binding.attach_root_widget(root);

    tracing::info!("Entering event loop");

    // Application state for winit 0.30+ ApplicationHandler
    struct AppState {
        binding: std::sync::Arc<AppBinding>,
        embedder: Option<DesktopEmbedder>,
    }

    impl ApplicationHandler for AppState {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.embedder.is_none() {
                let _span = tracing::info_span!("create_embedder").entered();
                // Safe to create window and surface now
                self.embedder = Some(pollster::block_on(DesktopEmbedder::new(
                    self.binding.clone(),
                    event_loop,
                )));
                tracing::info!("Desktop embedder ready");
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
                    emb.window().request_redraw();
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
                        tracing::trace!("RedrawRequested event, rendering frame");
                        emb.render_frame();
                        // Clear dirty flag after rendering
                        self.binding.mark_rendered();
                    }
                    WindowEvent::CloseRequested => {
                        tracing::info!("Window close requested");
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

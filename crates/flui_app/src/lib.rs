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

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// Re-exports for convenience
pub use binding::AppBinding;
pub use embedder::WgpuEmbedder;

// Re-export commonly used types from flui_core
pub use flui_core::{
    // Element system
    element::{ComponentElement, Element, ProviderElement, RenderElement, SliverElement},

    // Foundation types
    foundation::{ElementId, Key, Slot},

    // View system (new API)
    view::{AnyView, BuildContext, View, ViewElement},
};

use flui_core::view::IntoElement;
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
pub fn run_app<V>(app: V) -> !
where
    V: View + 'static,
{
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .with_line_number(true)
        .init();

    tracing::info!("Starting FLUI app");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();

    // 2. Attach root widget
    binding.pipeline.attach_root_widget(app);

    // 3. Create event loop
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    // 4. Create wgpu embedder (async init)
    let embedder = pollster::block_on(WgpuEmbedder::new(binding, &event_loop));

    tracing::info!("FLUI app initialized, entering event loop");

    // 5. Run event loop (blocks)
    embedder.run(event_loop)
}

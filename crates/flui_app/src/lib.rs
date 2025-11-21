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
    element::{ComponentElement, Element, ProviderElement, RenderElement},

    // Foundation types
    foundation::{ElementId, Key, Slot},

    // View system (new API)
    view::{BuildContext, View, ViewElement},
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
    V: View + Clone + Send + Sync + 'static,
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

    tracing::info!("Starting FLUI app");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();

    // 2. Attach root widget
    binding.attach_root_widget(app);

    // Application state for winit 0.30+ ApplicationHandler
    struct AppState {
        binding: std::sync::Arc<AppBinding>,
        embedder: Option<DesktopEmbedder>,
    }

    impl ApplicationHandler for AppState {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            tracing::info!("Resumed event received");
            if self.embedder.is_none() {
                tracing::info!("Creating DesktopEmbedder after Resumed event");
                // Safe to create window and surface now
                self.embedder = Some(pollster::block_on(DesktopEmbedder::new(
                    self.binding.clone(),
                    event_loop,
                )));
                tracing::info!("DesktopEmbedder created successfully");
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            // Request redraw every frame (for animations)
            if let Some(ref emb) = self.embedder {
                emb.window().request_redraw();
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

    tracing::info!("FLUI app initialized, entering event loop");

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

// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn android_main(app: android_activity::AndroidApp) {
    use flui_core::view::{BuildContext, IntoElement, View};
    use winit::event::{Event, WindowEvent};
    use winit::event_loop::ControlFlow;

    // Initialize cross-platform logging (uses Android logcat)
    flui_log::Logger::default()
        .with_filter("info,wgpu=warn,flui_core=debug,flui_app=info")
        .init();

    use winit::platform::android::EventLoopBuilderExtAndroid;

    // Android Demo - Vulkan backend + embedded fonts
    #[derive(Debug, Clone)]
    struct AndroidDemo;

    impl View for AndroidDemo {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment};
            use flui_types::Color;
            use flui_widgets::{Column, Container, Text};

            // Vulkan rendering + embedded Roboto font
            Container::builder()
                .color(Color::rgb(100, 200, 100))
                .padding(flui_types::EdgeInsets::all(40.0))
                .child(
                    Column::builder()
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .child(
                            Text::builder()
                                .data("FLUI на Android")
                                .size(32.0)
                                .color(Color::rgb(255, 255, 255))
                                .build(),
                        )
                        .child(
                            Text::builder()
                                .data("Vulkan + Embedded Fonts")
                                .size(18.0)
                                .color(Color::rgb(255, 255, 255))
                                .build(),
                        )
                        .build(),
                )
                .build()
        }
    }

    tracing::info!("Starting FLUI Android Demo");

    // 1. Initialize bindings
    let binding = AppBinding::ensure_initialized();

    // 2. Attach root widget
    binding.attach_root_widget(AndroidDemo);

    // 3. Create event loop with Android app
    let mut event_loop_builder = EventLoop::builder();
    event_loop_builder.with_android_app(app);
    let event_loop = event_loop_builder
        .build()
        .expect("Failed to create event loop");

    tracing::info!("Event loop created, waiting for Resumed event");

    // 4. Wait for Resumed event before creating embedder
    // On Android, window/surface creation must happen AFTER Resumed event
    let mut embedder: Option<crate::embedder::AndroidEmbedder> = None;

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::Resumed => {
                    tracing::info!("Resumed event received");
                    if embedder.is_none() {
                        tracing::info!("Creating AndroidEmbedder after Resumed event");
                        // Safe to create window and surface now
                        embedder = Some(pollster::block_on(crate::embedder::AndroidEmbedder::new(
                            binding.clone(),
                            elwt,
                        )));
                        tracing::info!("AndroidEmbedder created successfully");
                    } else {
                        // Resume existing embedder
                        if let Some(ref mut emb) = embedder {
                            emb.resume();
                            tracing::info!("AndroidEmbedder resumed");
                        }
                    }
                }

                Event::Suspended => {
                    tracing::info!("Suspended event received");
                    // Mark as suspended (stops rendering)
                    if let Some(ref mut emb) = embedder {
                        emb.suspend();
                        tracing::info!("AndroidEmbedder suspended (rendering stopped)");
                    }
                    // Drop embedder to release GPU resources
                    embedder = None;
                    tracing::info!("AndroidEmbedder dropped (GPU resources released)");
                }

                Event::AboutToWait => {
                    // Request redraw every frame (for animations)
                    if let Some(ref emb) = embedder {
                        emb.window().request_redraw();
                    }
                }

                Event::WindowEvent { event, .. } => {
                    if let Some(ref mut emb) = embedder {
                        match event {
                            WindowEvent::RedrawRequested => {
                                tracing::trace!("RedrawRequested event, rendering frame");
                                emb.render_frame();
                            }
                            WindowEvent::CloseRequested => {
                                tracing::info!("Window close requested");
                                elwt.exit();
                            }
                            other => {
                                emb.handle_event(other, elwt);
                            }
                        }
                    }
                }

                _ => {}
            }
        })
        .expect("Event loop error");
}

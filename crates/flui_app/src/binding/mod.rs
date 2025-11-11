//! Binding module - Flutter-style framework bindings
//!
//! This module provides the binding layer between platform events and framework components.
//!
//! # Architecture
//!
//! ```text
//! Platform (winit) → Bindings → Framework (flui_core, flui_rendering)
//! ```
//!
//! # Binding Types
//!
//! - **BindingBase**: Base trait for all bindings
//! - **GestureBinding**: Routes platform events to EventRouter
//! - **SchedulerBinding**: Manages frame callbacks and timing
//! - **RendererBinding**: Coordinates rendering pipeline (build/layout/paint)
//! - **WidgetsBinding**: Manages widget tree lifecycle
//! - **WidgetsFlutterBinding**: Combined singleton binding
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_app::binding::WidgetsFlutterBinding;
//!
//! let binding = WidgetsFlutterBinding::ensure_initialized();
//! binding.widgets.attach_root_widget(MyApp::new());
//! ```

mod base;
mod gesture;
mod renderer;
mod scheduler;
mod widgets;
mod widgets_flutter_binding;

// Re-exports
pub use base::BindingBase;
pub use gesture::GestureBinding;
pub use renderer::RendererBinding;
pub use scheduler::{FrameCallback, SchedulerBinding};
pub use widgets::WidgetsBinding;
pub use widgets_flutter_binding::WidgetsFlutterBinding;

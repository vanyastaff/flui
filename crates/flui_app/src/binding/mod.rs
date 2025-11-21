//! Binding module - Framework bindings
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
//! - **SchedulerBinding**: Wraps flui-scheduler for framework integration
//! - **RendererBinding**: Coordinates rendering
//! - **AppBinding**: Combined singleton binding with pipeline ownership
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_app::binding::AppBinding;
//!
//! let binding = AppBinding::ensure_initialized();
//! binding.attach_root_widget(MyApp::new());
//! ```

mod app_binding;
mod base;
mod gesture;
mod renderer;
mod scheduler;

// Re-exports
pub use app_binding::AppBinding;
pub use base::BindingBase;
pub use gesture::GestureBinding;
pub use renderer::RendererBinding;
pub use scheduler::SchedulerBinding;

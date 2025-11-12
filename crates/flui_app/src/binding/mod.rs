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
//! - **PipelineBinding**: Manages pipeline and widget tree lifecycle
//! - **AppBinding**: Combined singleton binding
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_app::binding::AppBinding;
//!
//! let binding = AppBinding::ensure_initialized();
//! binding.pipeline.attach_root_widget(MyApp::new());
//! ```

mod app_binding;
mod base;
mod gesture;
mod pipeline;
mod renderer;
mod scheduler;

// Re-exports
pub use app_binding::AppBinding;
pub use base::BindingBase;
pub use gesture::GestureBinding;
pub use pipeline::PipelineBinding;
pub use renderer::RendererBinding;
pub use scheduler::SchedulerBinding;

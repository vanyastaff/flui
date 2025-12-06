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
//! - **SchedulerBinding**: Wraps flui-scheduler for framework integration
//! - **RendererBinding**: Coordinates rendering
//! - **AppBinding**: Combined singleton binding with pipeline ownership and EventRouter
//!
//! Note: GestureBinding is now part of flui-platform's EmbedderCore.
//! AppBinding manages the EventRouter directly and shares it with the platform layer.
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
mod renderer;
mod scheduler;

// Re-exports
pub use app_binding::AppBinding;
pub use base::BindingBase;
pub use renderer::RendererBinding;
pub use scheduler::SchedulerBinding;

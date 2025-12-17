//! Application bindings - Flutter-like binding architecture.
//!
//! This module provides the binding system that connects all parts of FLUI:
//!
//! - [`Binding`] - Base trait for all bindings
//! - [`RendererBindingBehavior`] - Trait for renderer bindings
//! - [`RendererBinding`] - Manages render tree and pipeline (layout/paint)
//! - [`WidgetsBinding`] - Manages element tree and build phase (from flui-view)
//! - [`GestureBinding`] - Manages hit testing and gestures (from flui-interaction)
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's binding system:
//! - `flutter/lib/src/foundation/binding.dart` → Binding trait (BindingBase)
//! - `flutter/lib/src/rendering/binding.dart` → RendererBinding
//! - `flutter/lib/src/widgets/binding.dart` → WidgetsBinding
//! - `flutter/lib/src/gestures/binding.dart` → GestureBinding
//! - `flutter/lib/src/scheduler/binding.dart` → SchedulerBindingBehavior
//!
//! # Architecture
//!
//! ```text
//! Binding (base trait - like BindingBase)
//!   ├── init_instances()
//!   ├── init_service_extensions()
//!   └── is_initialized()
//!
//! RendererBindingBehavior : Binding
//!   ├── root_pipeline_owner()
//!   ├── render_views()
//!   ├── add_render_view() / remove_render_view()
//!   └── draw_frame()
//!
//! WidgetsBindingBehavior : RendererBindingBehavior
//!   ├── build_owner()
//!   ├── attach_root_widget()
//!   └── build_scope()
//!
//! AppBinding (singleton, combines all bindings)
//!   ├── renderer: RendererBinding
//!   ├── widgets: WidgetsBinding (from flui-view)
//!   ├── gestures: GestureBinding (from flui-interaction)
//!   └── scheduler: Scheduler
//! ```

mod renderer_binding;
mod traits;

// Export traits
pub use traits::{
    Binding, GestureBindingBehavior, RendererBindingBehavior, SchedulerBindingBehavior,
    WidgetsBindingBehavior,
};

// Export concrete implementations
pub use renderer_binding::{RenderView, RendererBinding};

// Re-export from other crates
pub use flui_interaction::binding::GestureBinding;
pub use flui_view::WidgetsBinding;

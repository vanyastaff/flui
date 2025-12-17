//! Application bindings - re-exports from specialized crates.
//!
//! FLUI uses composition instead of Flutter's mixin pattern.
//! Each binding is a separate crate with focused responsibility:
//!
//! - [`WidgetsBinding`] - Element tree and build phase (from flui-view)
//! - [`GestureBinding`] - Hit testing and gestures (from flui-interaction)
//! - [`PipelineOwner`] - Render tree and layout/paint (from flui_rendering)
//! - [`Scheduler`] - Frame scheduling (from flui-scheduler)
//! - [`SemanticsBinding`] - Accessibility (from flui-semantics)
//! - [`RenderingFlutterBinding`] - Rendering integration (local)
//!
//! # Flutter Equivalence
//!
//! Flutter's mixin-based binding system:
//! ```dart
//! class WidgetsFlutterBinding extends BindingBase
//!     with GestureBinding, SchedulerBinding, ServicesBinding,
//!          SemanticsBinding, PaintingBinding, RendererBinding,
//!          WidgetsBinding { }
//! ```
//!
//! FLUI equivalent using composition:
//! ```rust,ignore
//! pub struct WidgetsFlutterBinding {
//!     widgets: WidgetsBinding,
//!     gestures: GestureBinding,
//!     pipeline_owner: PipelineOwner,
//!     scheduler: Scheduler,
//!     renderer: RenderingFlutterBinding,
//! }
//! ```

mod renderer_binding;
mod traits;

// Re-export bindings from their respective crates
pub use flui_interaction::binding::GestureBinding;
pub use flui_painting::PaintingBinding;
pub use flui_rendering::binding::RendererBinding;
pub use flui_rendering::pipeline::PipelineOwner;
pub use flui_scheduler::Scheduler;
pub use flui_semantics::SemanticsBinding;
pub use flui_view::WidgetsBinding;

// Re-export local bindings and traits
pub use renderer_binding::RenderingFlutterBinding;
pub use traits::{Binding, RendererBindingBehavior};

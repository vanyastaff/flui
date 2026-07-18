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
//! Flutter composes these responsibilities into one class via mixins:
//! ```dart
//! class WidgetsFlutterBinding extends BindingBase
//!     with GestureBinding, SchedulerBinding, ServicesBinding,
//!          SemanticsBinding, PaintingBinding, RendererBinding,
//!          WidgetsBinding { }
//! ```
//!
//! FLUI does not compose a matching struct. `flui_app::WidgetsFlutterBinding`
//! is a type alias for [`AppBinding`](crate::AppBinding) — the transitional
//! process-scoped service host that owns the frame loop, render pipeline, and
//! input dispatch directly. The element/build side lives in `UiRealm`
//! (owner-affine, one per window), not as a field on this module's bindings.

mod renderer_binding;

// Re-export bindings from their respective crates
pub use flui_interaction::binding::GestureBinding;
pub use flui_painting::PaintingBinding;
pub use flui_rendering::{binding::RendererBinding, pipeline::PipelineOwner};
pub use flui_scheduler::Scheduler;
pub use flui_semantics::SemanticsBinding;
pub use flui_view::WidgetsBinding;
// Re-export the local binding
pub use renderer_binding::RenderingFlutterBinding;

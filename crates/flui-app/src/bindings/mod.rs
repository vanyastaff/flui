//! Application bindings - re-exports from specialized crates.
//!
//! FLUI uses composition instead of Flutter's mixin pattern.
//! Each binding is a separate crate with focused responsibility:
//!
//! - [`WidgetsBinding`] - Element tree and build phase (from flui-view)
//! - [`GestureBinding`] - Hit testing and gestures (from flui-interaction)
//! - [`PipelineOwner`] - Render tree and layout/paint (from flui_rendering)
//! - [`PipelineCell`] - Owner-local, closure-scoped handle to a
//!   `PipelineOwner` (`!Send + !Sync`, from flui_rendering); the shape every
//!   binding here actually stores and clones, `PipelineOwner` being the
//!   value it wraps
//! - [`UpdateScheduler`] - Frame scheduling (from flui-scheduler)
//! - [`RenderingFlutterBinding`] - Rendering integration (local); per-window
//!   semantics enablement/announce/event delivery lives on `SemanticsHost`
//!   (`crate::app::semantics_host`), not on a process-wide accessibility
//!   binding
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
//! FLUI does not compose a matching struct. The frame loop, render pipeline,
//! and input dispatch live directly on `UiRealm` (`crate::app::ui_realm`,
//! owner-affine, one per window) — there is no separate process-scoped
//! service host; the retired `AppBinding` and its combined-binding type
//! alias dissolved into `UiRealm` and the loop-scoped `AppRuntime`
//! (`crate::app::runtime`).

mod renderer_binding;

// Re-export bindings from their respective crates
pub use flui_interaction::binding::GestureBinding;
pub use flui_painting::PaintingBinding;
pub use flui_rendering::{
    binding::RendererBinding,
    pipeline::{PipelineCell, PipelineOwner},
};
pub use flui_scheduler::UpdateScheduler;
pub use flui_view::WidgetsBinding;
// Re-export the local binding
pub use renderer_binding::RenderingFlutterBinding;
pub(crate) use renderer_binding::redirty_pipeline_root;

//! The phases of a frame, as type markers of [`FramePass`].
//!
//! A frame lends its [`PipelineOwner`] to one phase at a time: layout,
//! compositing, paint, semantics (ADR-0399). Each phase's `run_*` method is a
//! method of its own pass only, so running a phase out of order is a compile
//! error, not a runtime assert, and while any pass is alive the owner's own
//! `&mut self` operations are a second borrow the compiler refuses.
//!
//! # Compile-time enforcement examples
//!
//! `tests/compile_fail.rs` pins the exact diagnostics for the invalid phase
//! calls below and for a downstream phase implementation, alongside a passing
//! constructor and complete phase sequence.
//!
//! An owner has no phase methods of its own:
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
//! owner.run_paint();                       // error[E0599]: run_paint is on the paint pass only
//! ```
//!
//! A pass runs its own phase and no other:
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
//! let mut layout = owner.layout_pass();
//! layout.run_paint();                      // error[E0599]: run_paint is on the paint pass, not layout
//! ```
//!
//! And while a pass borrows the owner, no frame runs on it:
//!
//! ```compile_fail
//! use flui_layer::SceneOwner;
//! use flui_rendering::pipeline::PipelineOwner;
//! let mut scene = SceneOwner::new();
//! let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
//! let layout = owner.layout_pass();
//! let _ = owner.run_frame(&mut scene.writer()); // error[E0499]: the layout pass still borrows it
//! drop(layout);
//! ```
//!
//! The phase markers are sealed in this module; downstream crates cannot
//! invent a new `PipelinePhase` impl:
//!
//! ```compile_fail
//! struct CustomPhase;
//! impl flui_rendering::pipeline::PipelinePhase for CustomPhase {
//!     const NAME: &'static str = "Custom";   // error[E0277]: not Sealed
//! }
//! ```
//!
//! The legitimate sequence type-checks and runs. `run_layout`, `run_paint` and
//! `run_semantics` return [`crate::error::RenderResult<()>`] (compositing
//! cannot fail); `?` or `expect` propagates panics-turned-errors from
//! third-party render objects:
//!
//! ```
//! use flui_layer::SceneOwner;
//! use flui_rendering::pipeline::PipelineOwner;
//! let mut scene = SceneOwner::new();
//! let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
//! let mut layout = owner.layout_pass();
//! layout.run_layout().unwrap();
//! let mut compositing = layout.into_compositing();
//! compositing.run_compositing();
//! let mut paint = compositing.into_paint();
//! paint.run_paint(&mut scene.writer()).unwrap();
//! let mut semantics = paint.into_semantics();
//! semantics.run_semantics().unwrap();
//! drop(semantics);                         // the owner is free again
//! ```
//!
//! And the orchestrator. `run_frame` buffers the frame's patch in the writer
//! it is handed and answers whether it painted:
//!
//! ```
//! use flui_layer::SceneOwner;
//! use flui_rendering::pipeline::PipelineOwner;
//! let mut scene = SceneOwner::new();
//! let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
//! let _painted: bool = owner.run_frame(&mut scene.writer()).unwrap();
//! ```
//!
//! ## Passes
//!
//! ```text
//!   PipelineOwner ─layout_pass()──▶ Layout
//!     ▲                               │ into_compositing()
//!     │ the last pass is dropped      ▼
//!     │                             Compositing
//!     │                               │ into_paint()
//!     │                               ▼
//!     │                             Paint
//!     │                               │ into_semantics()
//!     │                               ▼
//!     └────────────────────────── Semantics
//! ```
//!
//! [`FramePass`]: crate::pipeline::FramePass
//! [`PipelineOwner`]: crate::pipeline::PipelineOwner

// ---------------------------------------------------------------------------
// Sealed marker trait
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// Phase marker trait, sealed so only this crate can name phases.
/// Implementors are zero-sized marker structs ([`Layout`], [`Compositing`],
/// [`PaintPhase`], [`Semantics`]). They never appear in runtime values: they
/// live only as the `PhantomData` of a [`FramePass`](crate::pipeline::FramePass).
pub trait PipelinePhase: sealed::Sealed + 'static {
    /// Static name of the phase, for debug printing.
    const NAME: &'static str;
}

// ---------------------------------------------------------------------------
// Phase markers
// ---------------------------------------------------------------------------
/// Layout phase: `run_layout` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Layout;

/// Compositing-bits phase: `run_compositing` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Compositing;

/// Paint phase: `run_paint` may execute.
///
/// Named with a `Phase` suffix to avoid a collision with
/// `flui_painting::paint::Paint` (the canvas paint style type) that is
/// re-exported from `flui_rendering::pipeline::*`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PaintPhase;

/// Semantics phase: `run_semantics` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Semantics;

impl sealed::Sealed for Layout {}
impl sealed::Sealed for Compositing {}
impl sealed::Sealed for PaintPhase {}
impl sealed::Sealed for Semantics {}

impl PipelinePhase for Layout {
    const NAME: &'static str = "Layout";
}
impl PipelinePhase for Compositing {
    const NAME: &'static str = "Compositing";
}
impl PipelinePhase for PaintPhase {
    const NAME: &'static str = "Paint";
}
impl PipelinePhase for Semantics {
    const NAME: &'static str = "Semantics";
}

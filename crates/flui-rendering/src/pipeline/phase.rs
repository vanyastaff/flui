//! Pipeline phase typestate markers.
//!
//! Per docs/designs/2026-05-20-mythos-flui-rendering-redesign.md, the
//! `PipelineOwner` carries a phantom type parameter `Phase: PipelinePhase`
//! that lifts the runtime "what frame phase am I in" question into the
//! type system. Mythos Step 7 (2026-05-20) finalized the design: each
//! `run_*` method now lives only on its phase's impl block, so calling
//! `run_paint` on `<Idle>` or `run_layout` on `<Compositing>` is a
//! compile error, not a runtime assert.
//!
//! # Compile-time enforcement examples (Mythos Step 13 + Step 7)
//!
//! `run_layout`, `run_compositing`, `run_paint`, and `run_semantics`
//! each live on the matching phase's impl block. Calling them on the
//! wrong phase (or on `<Idle>`) fails at compile time:
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! owner.run_paint();                       // error[E0599]: run_paint is on <PaintPhase> only
//! ```
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! let mut owner = owner.into_layout();     // <Layout>
//! owner.run_paint();                       // error[E0599]: run_paint is on <PaintPhase>, not <Layout>
//! ```
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! let owner = owner.into_layout();         // <Layout>
//! let (_owner, _layer) = owner.run_frame(); // error[E0599]: run_frame is on <Idle> only
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
//! The legitimate transition sequence type-checks and runs. Each `run_*`
//! returns [`crate::error::RenderResult<()>`] (Mythos Step 12); `?` or
//! `expect` propagates panics-turned-errors from third-party render
//! objects:
//!
//! ```
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! let mut owner = owner.into_layout();     // <Layout>
//! owner.run_layout().unwrap();
//! let mut owner = owner.into_compositing();// <Compositing>
//! owner.run_compositing().unwrap();
//! let mut owner = owner.into_paint();      // <PaintPhase>
//! owner.run_paint().unwrap();
//! let mut owner = owner.into_semantics();  // <Semantics>
//! owner.run_semantics().unwrap();
//! let _owner = owner.finish();             // back to <Idle>
//! ```
//!
//! And the convenience orchestrator. `run_frame` always returns the
//! owner at [`Idle`] alongside a [`crate::error::RenderResult`] for
//! the layer tree:
//!
//! ```
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();
//! let (_owner, result) = owner.run_frame();
//! let _layer_tree = result.unwrap();
//! ```
//!
//! ## States
//!
//! ```text
//!   Idle ──into_layout()──▶ Layout
//!     ▲                       │ into_compositing()
//!     │ finish()               ▼
//!     │                     Compositing
//!     │                       │ into_paint()
//!     │                        ▼
//!     │                      Paint
//!     │                       │ into_semantics()
//!     │                        ▼
//!     └─────────────────── Semantics
//! ```
//!
//! Transitions consume `self`, so the borrow checker (not a runtime
//! flag) enforces that you cannot call `run_paint` before `into_paint`.

use core::marker::PhantomData;

// ---------------------------------------------------------------------------
// Sealed marker trait
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// Phase marker trait, sealed so only this crate can name phases.
///
/// Implementors are zero-sized marker structs ([`Idle`], [`Layout`],
/// [`Compositing`], [`Paint`], [`Semantics`]). They never appear in
/// runtime values -- they live only as `PhantomData<Phase>` on
/// `PipelineOwner<Phase>`.
pub trait PipelinePhase: sealed::Sealed + 'static {
    /// Static name of the phase, for debug printing.
    const NAME: &'static str;
}

// ---------------------------------------------------------------------------
// Phase markers
// ---------------------------------------------------------------------------

/// Default phase: no frame work in progress.
///
/// Insertion / removal / mark-dirty operations are valid in this phase
/// via the phase-agnostic accessors on `impl<Phase: PipelinePhase>
/// PipelineOwner<Phase>`. The phase-specific work (`run_layout`,
/// `run_compositing`, `run_paint`, `run_semantics`) lives elsewhere; on
/// `<Idle>` only the constructors and `run_frame` are reachable.
#[derive(Debug, Default, Clone, Copy)]
pub struct Idle;

/// Layout phase: `run_layout` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Layout;

/// Compositing-bits phase: `run_compositing` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Compositing;

/// Paint phase: `run_paint` may execute.
///
/// Named with a `Phase` suffix to avoid a collision with
/// `flui_types::painting::Paint` (the canvas paint style type) that is
/// re-exported from `flui_rendering::pipeline::*`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PaintPhase;

/// Semantics phase: `run_semantics` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Semantics;

impl sealed::Sealed for Idle {}
impl sealed::Sealed for Layout {}
impl sealed::Sealed for Compositing {}
impl sealed::Sealed for PaintPhase {}
impl sealed::Sealed for Semantics {}

impl PipelinePhase for Idle {
    const NAME: &'static str = "Idle";
}
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

// ---------------------------------------------------------------------------
// PhaseMarker helper
// ---------------------------------------------------------------------------

/// `PhantomData<Phase>` helper used by `PipelineOwner<Phase>`.
///
/// Always `Copy`/`Default`; carrying it on a struct adds no runtime
/// footprint.
pub type PhaseMarker<P> = PhantomData<P>;

//! Pipeline phase typestate markers.
//!
//! Per docs/designs/2026-05-20-mythos-flui-rendering-redesign.md, the
//! `PipelineOwner` carries a phantom type parameter `Phase: PipelinePhase`
//! that lifts the runtime "what frame phase am I in" question into the
//! type system. The four runtime `debug_doing_*` bool fields on
//! `PipelineOwner` will be retired in Mythos Step 7 once consuming
//! phase transitions land; until then they coexist as a runtime safety
//! net.
//!
//! This file lands the markers and the sealed trait so the rest of the
//! crate can begin annotating phase-specific impls. Step 1 deliberately
//! does **not** make transitions consume `self` -- that comes in Step 7
//! once every caller has been updated.
//!
//! # Compile-time enforcement examples (Mythos Step 13)
//!
//! `flush_layout`, `flush_compositing_bits`, `flush_paint`,
//! `flush_semantics`, and `run_frame` live on `PipelineOwner<Idle>`.
//! Calling them on a transitioned phase fails at compile time:
//!
//! ```compile_fail
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! let owner = owner.into_layout();         // <Layout>
//! owner.flush_layout();                    // error[E0599]: method not found in <Layout>
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
//! The legitimate transition sequence type-checks and runs:
//!
//! ```
//! use flui_rendering::pipeline::PipelineOwner;
//! let owner = PipelineOwner::new();        // <Idle>
//! let owner = owner.into_layout();         // <Layout>
//! let owner = owner.into_compositing();    // <Compositing>
//! let owner = owner.into_paint();          // <PaintPhase>
//! let owner = owner.into_semantics();      // <Semantics>
//! let _owner = owner.finish();             // back to <Idle>
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
//! Transitions are stateless renames; they consume nothing yet, only
//! retag the phantom parameter. When Mythos Step 7 promotes them to
//! `self`-consuming, the borrow checker will refuse to call `run_paint`
//! before `into_paint`.

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
/// once the consuming transitions land (Step 7). Today, every existing
/// `PipelineOwner` call is on `<Idle>` by virtue of the default type
/// parameter.
#[derive(Debug, Default, Clone, Copy)]
pub struct Idle;

/// Layout phase: `flush_layout` / `run_layout` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Layout;

/// Compositing-bits phase: `flush_compositing_bits` may execute.
#[derive(Debug, Default, Clone, Copy)]
pub struct Compositing;

/// Paint phase: `flush_paint` / `run_paint` may execute.
///
/// Named with a `Phase` suffix to avoid a collision with
/// `flui_types::painting::Paint` (the canvas paint style type) that is
/// re-exported from `flui_rendering::pipeline::*`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PaintPhase;

/// Semantics phase: `flush_semantics` may execute.
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

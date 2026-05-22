//! Hit test result accumulation -- canonical type re-exported from
//! `flui_interaction::routing`.
//!
//! # Cycle 4 R-7/U-4 migration
//!
//! Pre-cycle this file owned a `HitTestResult` struct with
//! `Arc<dyn HitTestTarget>`-based entries (`Vec<HitTestEntry>` +
//! a `Vec<MatrixTransformPart>` transform stack +
//! `add_with_paint_offset` / `add_with_raw_transform` helpers). It
//! competed with `flui_interaction::routing::HitTestResult`, which
//! carries data-attached entries (`RenderId target` + handler
//! closures + cursor) and Flutter-parity lazy transform globalization.
//! `flui-app`'s pointer-dispatch path had a literal `// TODO: Convert
//! rendering HitTestEntry targets to interaction targets` bridge
//! between the two types because conversion was never wired.
//!
//! Cycle 4 audit R-7 flagged this as a parallel-type smell. The
//! interaction-side wins because:
//!   - only its entries carry runtime-dispatch data (handler closure
//!     + cursor),
//!   - Flutter's canonical `HitTestResult` lives in `gestures/`,
//!     which `flui-interaction` is the workspace equivalent of
//!     (per PR #84 framework-spine work),
//!   - the rendering-side `HitTestTarget` trait had ONE production
//!     impl (`RenderView`) and TWO file-private `DummyTarget` stubs --
//!     vestigial dyn-dispatch surface, not a system the workspace
//!     actually uses.
//!
//! The struct, its `Debug`/`Default`/`IntoIterator` impls, the
//! file-private `DummyTarget`, and the 6 unit tests that exercised
//! them were deleted. The module now re-exports
//! `flui_interaction::routing::HitTestResult` so existing
//! `use crate::hit_testing::HitTestResult` consumers compile
//! unchanged with the canonical type.
//!
//! See `docs/research/2026-05-22-cycle4-wave2-design.md` for the
//! full migration plan.

pub use flui_interaction::routing::HitTestResult;

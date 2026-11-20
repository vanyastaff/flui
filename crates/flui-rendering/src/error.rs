//! The error a render operation returns instead of panicking.

use flui_foundation::RenderId;
use thiserror::Error;

/// The pipeline phase whose `catch_unwind` wrapper caught a render object's
/// (or its own effect-layer build's) panic, carried by
/// [`RenderError::Poisoned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PoisonPhase {
    /// `perform_layout_raw` panicked
    /// ([`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only),
    /// and the non-leaf box/sliver layout walks in
    /// `pipeline::owner::subtree_arena`).
    Layout,
    /// `paint_raw`, a node's own `paint_effects`, or the walk's resolution
    /// of a `PaintClip::PathTarget` it reports panicked during an ordinary
    /// paint (`PipelineOwner::<PaintPhase>::paint_subtree_impl`).
    Paint,
    /// A retained boundary's own effect layers panicked while being
    /// rebuilt for a composited-layer-update patch
    /// (`PipelineOwner::<PaintPhase>::layer_patches_for`).
    LayerUpdate,
}

impl core::fmt::Display for PoisonPhase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Layout => "layout",
            Self::Paint => "paint",
            Self::LayerUpdate => "layer-update",
        })
    }
}

/// What the semantics pass found wrong in the tree it assembled. The frame's
/// semantics are not published; layout and paint have already committed.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SemanticsError {
    /// A render object authored a sort order that is not finite.
    #[error("render object {host:?} authored a non-finite semantics sort order")]
    InvalidSortKey {
        /// The render object whose configuration holds the sort key.
        host: RenderId,
    },
    /// A render object listed one region key twice in one contribution list.
    #[error(
        "render object {host:?} authored duplicate semantics region key {key} in one contribution list"
    )]
    DuplicateRegion {
        /// The render object that authored the list.
        host: RenderId,
        /// The repeated key.
        key: u64,
    },
    /// No identity is left for a synthetic region node.
    #[error("synthetic semantics identity space exhausted")]
    RegionIdentityExhausted,
    /// The semantics root names a render object the tree does not hold.
    #[error("semantics root render object {root:?} is missing from the render tree")]
    MissingRootRenderObject {
        /// The missing root.
        root: RenderId,
    },
    /// The root's assembly produced other than one formed node.
    #[error(
        "semantics root assembly produced {actual} fragments; exactly one formed root is required"
    )]
    InvalidRootFragmentCount {
        /// How many fragments it produced.
        actual: usize,
    },
    /// The root stayed a pending fragment instead of forming a node.
    #[error("semantics root {root:?} remained pending instead of forming a node")]
    PendingRoot {
        /// The root.
        root: RenderId,
    },
}

/// Errors that can occur during rendering operations.
///
/// Errors are terminal values that travel through `?`, so the type is not
/// `Clone`; a caller that fans one out wraps it in an `Arc`.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RenderError {
    /// Render node not found in tree.
    #[error("render node not found: {0:?}")]
    NodeNotFound(RenderId),

    /// A render object cannot lay out under the constraints it was given,
    /// though it could under others: the pipeline retries it up to the
    /// poison budget instead of poisoning it on the first failure.
    #[error("invalid constraints for {render_object}: {reason}")]
    InvalidConstraints {
        /// Static debug name of the render object that refused them.
        render_object: &'static str,
        /// What the render object needs that the constraints do not give.
        reason: &'static str,
    },

    /// Another independently held loan is using the runtime's text resource.
    /// The current operation is refused without poisoning or native preparation
    /// debt; it can be repeated after the competing loan ends.
    #[error("the runtime's text context is already lent")]
    TextContextBusy,

    /// Text preparation or shaping refused the current layout request.
    /// This is an ordinary input/geometry error, not a caught render panic.
    #[error(transparent)]
    TextLayout(#[from] flui_painting::TextLayoutError),
    /// Numeric sizing answers are unavailable for this captured preference.
    #[error(transparent)]
    TextPreparationPending(#[from] flui_painting::TextPreparationPending),

    /// A scroll input changed while its layout proposal was being measured.
    /// Retry against the accepted input; the proposal has not been published.
    #[error("viewport offset changed during layout")]
    ViewportOffsetChanged,

    /// The paint phase began with layout work still pending: the
    /// scheduler's layout queue was non-empty when
    /// [`PipelineOwner::run_paint`](crate::pipeline::PipelineOwner::run_paint)
    /// was entered.
    ///
    /// Phase order is a type (each `run_*` lives on its phase's
    /// `PipelineOwner`), but a caller may drive the phases by hand
    /// (`into_layout().into_compositing().into_paint()`) and skip `run_layout`
    /// with work queued, so completeness stays a run-time check. With the
    /// queue empty that chain is allowed and returns `Ok`. The error aborts
    /// the frame and names no node.
    #[error("paint phase began with layout work still pending")]
    PaintBeforeLayout,

    /// The semantics pass refused the tree it assembled.
    #[error(transparent)]
    Semantics(#[from] SemanticsError),

    /// Geometry returned from a render object's `perform_layout` is
    /// structurally invalid (NaN, negative dimensions, larger than
    /// `f64::MAX / 2`, etc.). The frame is dropped; the previous
    /// geometry remains valid.
    #[error("invalid geometry from {render_object}: {reason}")]
    InvalidGeometry {
        /// Static debug name of the offending render object.
        render_object: &'static str,
        /// Reason the geometry failed validation.
        reason: &'static str,
    },

    /// A render object received an unbounded constraint where it
    /// expected bounded input. The parent must provide bounds (e.g.
    /// wrap the child in a `SizedBox` or `Container`).
    #[error("unbounded constraint at {render_object}; parent must provide bounds")]
    UnboundedConstraint {
        /// Static debug name of the render object that needed bounds.
        render_object: &'static str,
    },

    /// A render object's `perform_layout_raw` panicked
    /// ([`PoisonPhase::Layout`]); or a panic occurred while painting it —
    /// in `paint_raw`, in its own `paint_effects`, or in the walk's
    /// resolution of a `PaintClip::PathTarget` it reports
    /// ([`PoisonPhase::Paint`]); or while rebuilding a retained boundary's
    /// own effect layers for a composited-layer-update patch
    /// ([`PoisonPhase::LayerUpdate`]). The pipeline catches the panic,
    /// drops the in-flight frame, and surfaces this variant so the caller
    /// can decide (drop the node, retry next frame, abort).
    ///
    /// [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only)
    /// holds the layout wrapper and `PipelineOwner::<PaintPhase>` the paint
    /// and layer-update wrappers.
    #[error("render object {render_object} panicked during {phase}")]
    Poisoned {
        /// Static debug name of the offending render object.
        render_object: &'static str,
        /// Phase during which the panic occurred.
        phase: PoisonPhase,
    },

    /// A pipeline walk reached a node whose protocol does not match the
    /// protocol the walk expects. Returned by the layout walk (a `Box(_)`
    /// constraint handed to a `Sliver` node or vice versa) and by the paint
    /// walk (a sliver node reached by the box paint walk). Indicates a bug in
    /// the pipeline-dispatch caller — the message is phase-agnostic because
    /// more than one walk produces it.
    #[error("protocol mismatch: node is {node_protocol}, walk expected {constraints_protocol}")]
    ProtocolMismatch {
        /// Protocol of the node that received the call.
        node_protocol: &'static str,
        /// Protocol the walk expected (e.g. `"Box"` for the box layout/paint
        /// walks).
        constraints_protocol: &'static str,
    },

    /// A layout recursed (through `LayoutContext::layout_child`) back into a
    /// node whose layout was still in flight on the same frame. The layout
    /// walk's `LayoutFrame` marks each slot in flight while its frame
    /// runs and refuses a second entry with this variant.
    #[error("layout cycle detected: node {0:?} re-entered while its own layout was in flight")]
    LayoutCycle(RenderId),

    /// A render object violated a named layout contract, which the bridge
    /// or the pipeline detected and reports by name.
    ///
    /// A panic in user code is [`Poisoned`](Self::Poisoned) instead; this
    /// variant reaches the caller through `Result` (`perform_layout_raw`
    /// returns `RenderResult`), not through an unwind.
    #[error("contract violation in {render_object}: {what}")]
    ContractViolation {
        /// Static debug name of the render object that violated the contract.
        render_object: &'static str,
        /// The contract that was violated.
        what: &'static str,
    },
}

/// Result type alias for render operations.
pub type RenderResult<T> = Result<T, RenderError>;

impl From<flui_painting::TextMeasurementError> for RenderError {
    fn from(error: flui_painting::TextMeasurementError) -> Self {
        match error {
            flui_painting::TextMeasurementError::Layout(error) => Self::TextLayout(error),
            flui_painting::TextMeasurementError::Pending(pending) => {
                Self::TextPreparationPending(pending)
            }
        }
    }
}

impl RenderError {
    /// The node this error names, when it carries one: the node a lookup
    /// missed or a layout cycle re-entered.
    ///
    /// Errors that name no node (a panic caught in a render object, an
    /// invalid constraint) answer `None`; the pipeline owner attaches the
    /// node when it knows it.
    #[must_use]
    pub fn node(&self) -> Option<RenderId> {
        match self {
            Self::NodeNotFound(id) | Self::LayoutCycle(id) => Some(*id),
            _ => None,
        }
    }

    /// An ordinary layout refusal must propagate without a poisoned stand-in.
    pub(crate) fn is_recoverable_layout_error(&self) -> bool {
        matches!(
            self,
            Self::TextContextBusy
                | Self::TextLayout(_)
                | Self::TextPreparationPending(_)
                | Self::ViewportOffsetChanged
        )
    }

    /// Creates an [`InvalidConstraints`](Self::InvalidConstraints) error.
    pub fn invalid_constraints(render_object: &'static str, reason: &'static str) -> Self {
        Self::InvalidConstraints {
            render_object,
            reason,
        }
    }

    /// Creates an [`InvalidGeometry`](Self::InvalidGeometry) error.
    pub fn invalid_geometry(render_object: &'static str, reason: &'static str) -> Self {
        Self::InvalidGeometry {
            render_object,
            reason,
        }
    }

    /// Creates an [`UnboundedConstraint`](Self::UnboundedConstraint) error.
    pub fn unbounded_constraint(render_object: &'static str) -> Self {
        Self::UnboundedConstraint { render_object }
    }

    /// Creates a [`ContractViolation`](Self::ContractViolation) error.
    pub fn contract_violation(render_object: &'static str, what: &'static str) -> Self {
        Self::ContractViolation {
            render_object,
            what,
        }
    }

    /// Creates a [`Poisoned`](Self::Poisoned) error.
    pub fn poisoned(render_object: &'static str, phase: PoisonPhase) -> Self {
        Self::Poisoned {
            render_object,
            phase,
        }
    }

    /// Creates a [`LayoutCycle`](Self::LayoutCycle) error.
    pub fn layout_cycle(id: RenderId) -> Self {
        Self::LayoutCycle(id)
    }
}

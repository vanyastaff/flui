//! Heterogeneous-children trait — [`ViewSeq`] — and the macro-
//! generated tuple impls + `Vec`-of-View / `Vec<BoxedView>` impls
//! that drive the two C2 authoring paths.
//!
//! See `docs/FOUNDATIONS.md` §C2 ("heterogeneous children") and
//! `specs/004-view-element-core/spec.md` FR-012–FR-018. The two
//! load-bearing paths are:
//!
//! - **Static tuple path** — `(A, B, C, …): ViewSeq` for tuple
//!   arities `0..=16`, expanded by the `impl_view_seq_for_tuple!`
//!   macro in `tuple_impls.rs`. Each tuple position keeps its
//!   concrete `View` type to the boundary; the per-position
//!   callback in [`ViewSeq::for_each`] pays exactly one `&dyn View`
//!   `dyn`-call per child — `SC-007` dispatch-cost model.
//! - **Dynamic `Vec` path** — `Vec<V: View>: ViewSeq` for the
//!   homogeneous case and `Vec<BoxedView>: ViewSeq` for the
//!   heterogeneous case, the canonical shape for every scrolling
//!   widget in the catalog (`ListView`, `GridView`,
//!   `CustomScrollView`, `DataTable`). The dynamic path pays one
//!   `dyn`-call per child via the boxed `BoxedView` — equivalent
//!   to the tuple path's `&dyn View` boundary, so the per-child
//!   cost matches.
//!
//! Both paths share the same keyed reconciler algorithm (FR-016);
//! the difference is the per-position monomorphism the tuple path
//! retains at the outer `match self.kind { … }` dispatch in the
//! element-storage layer.
//!
//! The `column!` / `row!` macros (`crates/flui-view/src/macros/mod.rs`)
//! expand to the tuple form and emit the friendly FR-034
//! `compile_error!` at >16 children.

mod tuple_impls;
mod vec_impls;

use crate::view::{BoxedView, View};

/// Heterogeneous-children trait for multi-child widget configuration.
///
/// Implemented for:
///
/// - Tuples of arities `0..=16` whose elements all implement
///   [`View`] — see the crate's `seq::tuple_impls` module. The
///   tuple path keeps each position's concrete type to the
///   element boundary.
/// - `Vec<V: View>` (homogeneous dynamic case) — see the crate's
///   `seq::vec_impls` module.
/// - `Vec<BoxedView>` (heterogeneous dynamic case) — covered by
///   the same `Vec<V: View>` blanket since `BoxedView: View`.
///
/// Multi-child widgets bind `C: ViewSeq` (`struct Column<C: ViewSeq>
/// { children: C }`) rather than specialize over `Vec<BoxedView>`
/// directly (FR-018) — this is what makes the tuple-static-path
/// monomorphism benefits actually land.
///
/// # Object safety
///
/// `ViewSeq` is **not** object-safe: [`for_each`](Self::for_each)
/// takes a generic `F: FnMut(usize, &dyn View)`. Multi-child widgets
/// bind `C: ViewSeq` as a type parameter (or land on `Vec<BoxedView>`
/// directly); no `dyn ViewSeq` use exists or is needed.
pub trait ViewSeq {
    /// Number of children in this sequence.
    fn len(&self) -> usize;

    /// `true` when the sequence carries zero children.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Per-position iteration with a type-erased `&dyn View` handle.
    ///
    /// The callback receives `(index, &dyn View)` for each child in
    /// declaration order. Tuple-static impls expand to an explicit
    /// sequence of `f(i, &self.i)` calls — each call site is
    /// monomorphic per position, the closure inlines, and the tuple
    /// element is concrete (`SC-007`).
    ///
    /// The `&dyn View` parameter pays one `dyn`-call per child at
    /// the closure boundary; this is the per-child cost `SC-007`
    /// acknowledges and is sanctioned by FR-029 point 1 (the
    /// element-storage enum's inner `Box<dyn …>`).
    fn for_each<F: FnMut(usize, &dyn View)>(&self, f: F);

    /// Consume into the dynamic-path representation.
    ///
    /// Used by call sites that need ID-based reconciler input — the
    /// keyed reconciler entry point operates on a `Vec<BoxedView>`
    /// slice. The call site allocates exactly one `Vec<BoxedView>`
    /// per `Variable`-arity parent rebuild; this is the linear cost
    /// the `SC-006` linearity bench measures (and it is dominated
    /// by the per-child reconcile work for any non-trivial child
    /// count).
    fn into_boxed_vec(self) -> Vec<BoxedView>;
}

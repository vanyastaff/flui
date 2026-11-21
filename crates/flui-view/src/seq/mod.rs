//! Heterogeneous-children trait — [`ViewSeq`] — and the macro-
//! generated tuple impls + `Vec`-of-View / `Vec<BoxedView>` impls
//! that drive the two authoring paths for heterogeneous children.
//!
//! The two load-bearing paths are:
//!
//! - **Static tuple path** — `(A, B, C, …): ViewSeq` for tuple
//!   arities `0..=16`, expanded by the `impl_view_seq_for_tuple!`
//!   macro in `tuple_impls.rs`. Each tuple position keeps its
//!   concrete `View` type in the widget; [`ViewSeq::for_each`] erases
//!   each position into a `BoxedView` of its own clone, the form the
//!   reconciler takes.
//! - **Dynamic `Vec` path** — `Vec<V: View>: ViewSeq` for the
//!   homogeneous case and `Vec<BoxedView>: ViewSeq` for the
//!   heterogeneous case, the canonical shape for every scrolling
//!   widget in the catalog (`ListView`, `GridView`,
//!   `CustomScrollView`, `DataTable`). A `Vec<BoxedView>` hands each
//!   child on shared, so a child the parent hands back unchanged keeps
//!   its identity and is not built again.
//!
//! Both paths share the same keyed reconciler algorithm (FR-016);
//! the difference is the per-position monomorphism the tuple path
//! retains at the outer `match self.kind { … }` dispatch in the
//! element-storage layer.
//!
//! A widget takes its children as one of these values directly:
//! `Column::new((a, b, c))`, `Row::new([a, b])`, `Stack::new(children)`
//! with a `Vec`. A one-child tuple keeps its comma: `(a,)`.

mod tuple_impls;
mod vec_impls;

use crate::view::BoxedView;

/// Heterogeneous-children trait for multi-child widget configuration.
///
/// Implemented for:
///
/// - Tuples of arities `0..=16` whose elements all implement
///   [`View`](crate::View) — see the crate's `seq::tuple_impls` module. The
///   tuple path keeps each position's concrete type to the
///   element boundary.
/// - `Vec<V: View>` (homogeneous dynamic case) — see the crate's
///   `seq::vec_impls` module.
/// - `[V; N]` with `V: View` (a fixed list of one type).
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
/// takes a generic `F: FnMut(usize, &BoxedView)`. Multi-child widgets
/// bind `C: ViewSeq` as a type parameter (or land on `Vec<BoxedView>`
/// directly); no `dyn ViewSeq` use exists or is needed.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a list of children",
    label = "children are a tuple of up to 16 views, an array or a `Vec`",
    note = "write `(a, b, c)`, `(a,)` for one child, `[a, b]` for one type, or `vec![a.boxed(), ..]` past 16 children"
)]
pub trait ViewSeq {
    /// Number of children in this sequence.
    fn len(&self) -> usize;

    /// `true` when the sequence carries zero children.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Per-position iteration, each child handed as a [`BoxedView`].
    ///
    /// The callback receives `(index, &BoxedView)` for each child in
    /// declaration order, which the render element hands on to the
    /// reconciler as it is. A child already held as a `BoxedView` is
    /// handed shared, so its element keeps that allocation and a child
    /// handed back unchanged is not built again; any other child is erased
    /// into a `BoxedView` of its own clone.
    fn for_each<F: FnMut(usize, &BoxedView)>(&self, f: F);

    /// Consume into the dynamic-path representation.
    ///
    /// Used by call sites that need ID-based reconciler input — the
    /// keyed reconciler entry point operates on a `Vec<BoxedView>`
    /// slice. The call site allocates exactly one `Vec<BoxedView>`
    /// per `Variable`-arity parent rebuild; this is a linear cost
    /// (dominated by the per-child reconcile work for any
    /// non-trivial child count).
    fn into_boxed_vec(self) -> Vec<BoxedView>;
}

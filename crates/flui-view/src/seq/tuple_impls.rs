//! Macro-generated tuple [`ViewSeq`] impls for arities `0..=16`.
//!
//! The arity cap matches Rust stdlib's standard tuple-trait cap
//! (every trait that ships impls for tuples — `Debug`, `Hash`,
//! `PartialEq`, etc. — stops at 12 or 16, with 16 being the larger
//! cap). Authors who need >16 statically-known heterogeneous
//! children fall back to the `Vec<BoxedView>` dynamic path with one
//! `.boxed()` per child; the `column!` / `row!` macros (Phase 3
//! §U26) emit a friendly `compile_error!` per FR-034 directing
//! the author to this fallback.
//!
//! Tuple impls are macro-generated so the 17 arities (`()` through
//! `(A,B,…,P)`) share a single source of truth — bumping the cap
//! later is a one-line `impl_view_seq_for_tuple!` change rather
//! than a 17-line copy-paste.

use super::ViewSeq;
use crate::view::{BoxedView, View, ViewExt};

// Special-cased — the macro arms below all assume at least one
// generic parameter, so the 0-arity case lives outside the macro
// invocation.
impl ViewSeq for () {
    #[inline]
    fn len(&self) -> usize {
        0
    }

    #[inline]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline]
    fn for_each<F: FnMut(usize, &dyn View)>(&self, _f: F) {}

    #[inline]
    fn into_boxed_vec(self) -> Vec<BoxedView> {
        Vec::new()
    }
}

/// Macro: emit `impl ViewSeq for (A_1, A_2, …, A_N)` for one arity.
///
/// `$count` is the arity (compile-time integer). `$($idx:tt)+` is
/// the sequence of tuple-position indices (e.g. `0 1 2` for arity
/// 3). `$($name:ident)+` is the matching sequence of generic type
/// names (e.g. `A B C`). The expansion produces:
///
/// ```ignore
/// impl<A: View, B: View, C: View> ViewSeq for (A, B, C) {
///     fn len(&self) -> usize { 3 }
///     fn for_each<F: FnMut(usize, &dyn View)>(&self, mut f: F) {
///         f(0, &self.0);
///         f(1, &self.1);
///         f(2, &self.2);
///     }
///     fn into_boxed_vec(self) -> Vec<BoxedView> {
///         vec![self.0.boxed(), self.1.boxed(), self.2.boxed()]
///     }
/// }
/// ```
macro_rules! impl_view_seq_for_tuple {
    ($count:literal; $( ($idx:tt, $name:ident) ),+ $(,)?) => {
        impl<$($name: View),+> ViewSeq for ($($name,)+) {
            #[inline]
            fn len(&self) -> usize {
                $count
            }

            // `__Visitor` rather than the natural `F` to avoid a
            // clash with the tuple's own `F` generic at arity 6+
            // (the tuple letters run A..=P, and `F` is the sixth).
            #[inline]
            fn for_each<__Visitor: FnMut(usize, &dyn View)>(&self, mut visitor: __Visitor) {
                $( visitor($idx, &self.$idx); )+
            }

            #[inline]
            fn into_boxed_vec(self) -> Vec<BoxedView> {
                vec![ $( self.$idx.boxed() ),+ ]
            }
        }
    };
}

impl_view_seq_for_tuple!(1; (0, A));
impl_view_seq_for_tuple!(2; (0, A), (1, B));
impl_view_seq_for_tuple!(3; (0, A), (1, B), (2, C));
impl_view_seq_for_tuple!(4; (0, A), (1, B), (2, C), (3, D));
impl_view_seq_for_tuple!(5; (0, A), (1, B), (2, C), (3, D), (4, E));
impl_view_seq_for_tuple!(6; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
impl_view_seq_for_tuple!(7; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
impl_view_seq_for_tuple!(8; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
impl_view_seq_for_tuple!(9; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
impl_view_seq_for_tuple!(10; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
impl_view_seq_for_tuple!(11; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K));
impl_view_seq_for_tuple!(12; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K), (11, L));
impl_view_seq_for_tuple!(13; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K), (11, L), (12, M));
impl_view_seq_for_tuple!(14; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K), (11, L), (12, M), (13, N));
impl_view_seq_for_tuple!(15; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K), (11, L), (12, M), (13, N), (14, O));
impl_view_seq_for_tuple!(16; (0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J), (10, K), (11, L), (12, M), (13, N), (14, O), (15, P));

#[cfg(test)]
mod tests {
    //! Smoke coverage for tuple arities 0, 1, 3, and 16 — the
    //! shape boundaries (empty / singleton / mid / cap).

    use super::*;

    #[derive(Clone)]
    struct Leaf(u32);

    impl View for Leaf {
        fn create_element(&self) -> crate::element::ElementKind {
            // The fixture never participates in a real mount; we
            // return a stub element so the trait bound is satisfied.
            crate::element::ElementKind::stateless(self)
        }
    }

    impl crate::view::StatelessView for Leaf {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl crate::view::IntoView {
            Leaf(self.0)
        }
    }

    #[test]
    fn unit_arity_is_empty() {
        let s: () = ();
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        let v = s.into_boxed_vec();
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn arity_one_holds_single_child() {
        let s = (Leaf(7),);
        assert_eq!(s.len(), 1);
        assert!(!s.is_empty());

        let mut visited = vec![];
        s.for_each(|i, _v| visited.push(i));
        assert_eq!(visited, vec![0]);

        let v = s.into_boxed_vec();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn arity_three_iterates_in_order() {
        let s = (Leaf(1), Leaf(2), Leaf(3));
        assert_eq!(s.len(), 3);

        let mut visited = vec![];
        s.for_each(|i, _v| visited.push(i));
        assert_eq!(visited, vec![0, 1, 2]);

        let v = s.into_boxed_vec();
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn arity_sixteen_is_the_cap() {
        // Build a 16-tuple of Leaf values.
        let s = (
            Leaf(0),
            Leaf(1),
            Leaf(2),
            Leaf(3),
            Leaf(4),
            Leaf(5),
            Leaf(6),
            Leaf(7),
            Leaf(8),
            Leaf(9),
            Leaf(10),
            Leaf(11),
            Leaf(12),
            Leaf(13),
            Leaf(14),
            Leaf(15),
        );
        assert_eq!(s.len(), 16);

        let v = s.into_boxed_vec();
        assert_eq!(v.len(), 16);
    }
}

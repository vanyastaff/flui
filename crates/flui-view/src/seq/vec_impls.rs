//! `Vec`-shaped [`ViewSeq`] impls — the dynamic-path side of C2.
//!
//! - `Vec<V: View>` — homogeneous dynamic; every scrolling widget
//!   in the catalog that builds a list from runtime data (a
//!   `ListView::builder` with one item type) sits here.
//! - `Vec<BoxedView>` — heterogeneous dynamic; the more general
//!   shape (a `ListView` of items whose types vary by row, a
//!   conditional `column!` that escapes >16 children to the
//!   dynamic-fallback path per FR-013's cap).
//!
//! Per-child `dyn`-dispatch cost is the same as the tuple path's
//! `&dyn View` callback boundary — both pay one `dyn`-call per
//! child (`SC-007` model). The tuple path's monomorphism advantage
//! is per-*position* (inlined callback bodies, no nested arity
//! discriminant), not per-child.

use super::ViewSeq;
use crate::view::{BoxedView, View, ViewExt};

impl<V: View> ViewSeq for Vec<V> {
    #[inline]
    fn len(&self) -> usize {
        Vec::len(self)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    #[inline]
    fn for_each<F: FnMut(usize, &dyn View)>(&self, mut f: F) {
        for (i, v) in self.iter().enumerate() {
            f(i, v);
        }
    }

    #[inline]
    fn into_boxed_vec(self) -> Vec<BoxedView> {
        self.into_iter().map(ViewExt::boxed).collect()
    }
}

// `Vec<BoxedView>` is covered by the `impl<V: View> ViewSeq for Vec<V>`
// blanket above (`BoxedView: View`). No standalone impl is needed —
// FR-015 lists `Vec<BoxedView>` explicitly because it is the
// canonical heterogeneous-dynamic shape (every catalog scrollable
// widget sits on it), but the trait machinery treats it as a
// special case of the blanket. The `into_boxed_vec` path is the
// less-efficient general path (one round-trip per element via
// `ViewExt::boxed()` returning a fresh `BoxedView` per item) rather
// than an identity; if profiling shows this dominates, a
// `specialization`-style override (or a marker-trait variant of
// `ViewSeq`) can short-circuit the round-trip in a follow-up
// without breaking authoring code.

#[cfg(test)]
mod tests {
    //! Smoke coverage for the two dynamic-path impls.

    use super::*;
    use crate::view::ViewExt;

    #[derive(Clone)]
    struct Leaf(u32);

    impl View for Leaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    impl crate::view::StatelessView for Leaf {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl crate::view::IntoView {
            Leaf(self.0)
        }
    }

    #[derive(Clone)]
    struct OtherLeaf(&'static str);

    impl View for OtherLeaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    impl crate::view::StatelessView for OtherLeaf {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl crate::view::IntoView {
            OtherLeaf(self.0)
        }
    }

    #[test]
    fn vec_of_homogeneous_views_implements_view_seq() {
        let s: Vec<Leaf> = vec![Leaf(1), Leaf(2), Leaf(3)];
        assert_eq!(<Vec<Leaf> as ViewSeq>::len(&s), 3);

        let mut visited = vec![];
        ViewSeq::for_each(&s, |i, _v| visited.push(i));
        assert_eq!(visited, vec![0, 1, 2]);

        let boxed = s.into_boxed_vec();
        assert_eq!(boxed.len(), 3);
    }

    #[test]
    fn vec_of_boxed_views_supports_heterogeneous_children() {
        let s: Vec<BoxedView> = vec![Leaf(1).boxed(), OtherLeaf("two").boxed(), Leaf(3).boxed()];
        assert_eq!(<Vec<BoxedView> as ViewSeq>::len(&s), 3);

        let mut visited = vec![];
        ViewSeq::for_each(&s, |i, _v| visited.push(i));
        assert_eq!(visited, vec![0, 1, 2]);

        let boxed = s.into_boxed_vec();
        assert_eq!(boxed.len(), 3);
    }

    #[test]
    fn empty_vec_reports_zero_length() {
        let s: Vec<Leaf> = Vec::new();
        assert_eq!(<Vec<Leaf> as ViewSeq>::len(&s), 0);
        assert!(<Vec<Leaf> as ViewSeq>::is_empty(&s));
    }
}

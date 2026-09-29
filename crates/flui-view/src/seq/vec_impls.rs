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
//! child. The tuple path's monomorphism advantage
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

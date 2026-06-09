//! `Memo<V>` — a proxy-family memoization combinator.
//!
//! Wraps any `View + PartialEq` and overrides
//! [`View::should_skip_rebuild`] to bail out of a rebuild when the
//! inner view compares equal to the previous version.
//!
//! # The `PartialEq` bound is intentionally narrow
//!
//! The bound lives **only** on `Memo<V>`, not on the base `View` trait.
//! That placement is a Constitution C1 hard requirement — "the Druid
//! trap": a blanket `View: PartialEq` bound means every view type must
//! implement `PartialEq`, which is impossible for views that carry
//! callbacks or `Arc<dyn Fn>` fields (closures are not `PartialEq`).
//! Druid added this bound and it poisoned its entire ecosystem of view
//! types. `Memo<V>` is the opt-in surface; non-`PartialEq` views use the
//! safe default ([`View::should_skip_rebuild`] returns `false` = always
//! rebuild). `Clone` is *not* part of the trap — the whole view universe
//! is already `Clone`/`DynClone` (`View: DynClone`, `ProxyView: Clone`),
//! so `Memo<V>` requires `V: Clone` like any other proxy wrapper.
//!
//! # Warning — unsafe for callback views
//!
//! `Memo<V>` is **unsound for views that carry a callback, `Box<dyn Fn>`,
//! or `Arc<dyn Fn>` field.** `PartialEq` cannot compare closures; two
//! views may compare *equal by data* while the handler has been silently
//! replaced. The stale handler is then kept alive because the rebuild is
//! skipped, and the UI stops responding to the new closure.
//!
//! **Rule of thumb:** only use `Memo<V>` on purely data-driven views
//! whose `PartialEq` covers every field that affects output. If in doubt,
//! do not use `Memo<V>` — the default `should_skip_rebuild = false` is
//! always safe.

use super::view::{ElementBase, View};
use crate::{ProxyElement, element::ProxyBehavior};

/// Memoization wrapper that skips rebuilds when the inner view is
/// [`PartialEq`]-equal to the previous version.
///
/// # Usage
///
/// ```rust,ignore
/// // Only rebuilds MyView when its data actually changes.
/// Memo::new(MyView { label: "hello".into(), count: 42 })
/// ```
///
/// # Warning — unsafe for callback-carrying views
///
/// `Memo<V>` **must not** be used with views that carry a callback,
/// `Box<dyn Fn>`, or `Arc<dyn Fn>` field. `PartialEq` cannot compare
/// closures: if the closure is replaced between builds but the data
/// fields are unchanged, `PartialEq` returns `true`, the rebuild is
/// skipped, and the **stale handler is silently kept** — the UI stops
/// responding to the new closure. This is a known, irremediable
/// limitation of `PartialEq`-based memoization. See the module
/// documentation for the full rationale.
///
/// # Element kind
///
/// `Memo<V>` is a **proxy-family** wrapper: it reuses
/// [`ProxyElement`]/[`ProxyBehavior`] and does not introduce a new
/// `ElementKind` variant.
///
/// # Constitution compliance
///
/// - **C1** — `PartialEq` bound lives only here, not on `View`; the
///   default `should_skip_rebuild` is `false`.
/// - **C4** — `should_skip_rebuild` carries `where Self: Sized`, keeping
///   `View` object-safe.
/// - **C9** — the equality check runs with both concrete `V` values
///   coexisting; zero `dyn` at the comparison site.
#[derive(Clone, Debug)]
pub struct Memo<V> {
    inner: V,
}

impl<V: View + PartialEq + Clone> Memo<V> {
    /// Wrap `inner` in a memoization combinator.
    ///
    /// Subsequent builds are skipped whenever the new `Memo<V>` compares
    /// equal to the one currently stored in the element — i.e. when
    /// `new_memo.inner == prev_memo.inner`.
    #[inline]
    #[must_use]
    pub fn new(inner: V) -> Self {
        Self { inner }
    }

    /// Borrow the wrapped inner view.
    #[inline]
    pub fn inner(&self) -> &V {
        &self.inner
    }
}

impl<V: View + PartialEq + Clone> View for Memo<V> {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(ProxyElement::new(self, ProxyBehavior))
    }

    /// Skip the rebuild when `self.inner == prev.inner`.
    ///
    /// This is the only site in the codebase that uses a `PartialEq`
    /// bound to drive the skip decision — the bound is scoped here, not on
    /// the base `View` trait (C1). Both `self` and `prev` are concrete
    /// `Memo<V>` values; no `dyn` dispatch occurs at the comparison (C9).
    fn should_skip_rebuild(&self, prev: &Self) -> bool
    where
        Self: Sized,
    {
        self.inner == prev.inner
    }
}

/// Proxy the child view: `Memo<V>` forwards `child()` to the inner view so
/// `ProxyBehavior` can locate the single child element during build.
impl<V: View + PartialEq + Clone> crate::ProxyView for Memo<V> {
    fn child(&self) -> &dyn View {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Single;
    use crate::element::dispatch::dispatch_view_update;
    use crate::element::generic::ElementCore;
    use crate::{BuildContext, IntoView, StatelessView, ViewExt};

    // A trivial data-only view. `build` is never executed by these tests
    // (they exercise `should_skip_rebuild` and the config-update dispatch
    // path, not the build pass), but must compile; it returns a boxed
    // clone purely to satisfy the `impl IntoView` return.
    #[derive(Clone, PartialEq, Debug)]
    struct ProbeView {
        data: u32,
    }

    impl StatelessView for ProbeView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for ProbeView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::StatelessElement;
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }
    }

    fn assert_view_is_object_safe(_: &dyn View) {}

    #[test]
    fn memo_object_safety_holds() {
        // Constructing `Box<dyn View>` from a `Memo` must compile — proves
        // the `where Self: Sized` clause kept `View` object-safe (C4).
        let m: Box<dyn View> = Box::new(Memo::new(ProbeView { data: 1 }));
        assert_view_is_object_safe(m.as_ref());
    }

    #[test]
    fn should_skip_rebuild_true_when_equal() {
        let a = Memo::new(ProbeView { data: 42 });
        let b = Memo::new(ProbeView { data: 42 });
        assert!(
            a.should_skip_rebuild(&b),
            "Memo::should_skip_rebuild must be true for equal inner views"
        );
    }

    #[test]
    fn should_skip_rebuild_false_when_different() {
        let a = Memo::new(ProbeView { data: 1 });
        let b = Memo::new(ProbeView { data: 2 });
        assert!(
            !a.should_skip_rebuild(&b),
            "Memo::should_skip_rebuild must be false for unequal inner views"
        );
    }

    #[test]
    fn default_should_skip_rebuild_is_false() {
        // ProbeView does NOT wrap Memo; its `should_skip_rebuild` is the
        // default `false`. Even with equal data it must not skip — the C1
        // safe-default (Flutter parity, no accidental skip).
        let a = ProbeView { data: 7 };
        let b = ProbeView { data: 7 };
        assert!(
            !a.should_skip_rebuild(&b),
            "default should_skip_rebuild must be false (C1 safe-default)"
        );
    }

    // The dispatch equality-bail: on an equal Memo update, dispatch must
    // return true (element reused) AND leave the element NOT dirty (the
    // mark_dirty was skipped). build_scope only rebuilds dirty elements
    // (should_build() gates on the dirty flag), so "not dirty" == "subtree
    // rebuild skipped". On an unequal update the element is marked dirty.
    #[test]
    fn dispatch_skips_on_equal_memo() {
        let mut core: ElementCore<Memo<ProbeView>, Single> =
            ElementCore::new(Memo::new(ProbeView { data: 5 }));
        core.clear_dirty(); // ElementCore::new starts dirty; clear to observe the skip.

        let equal: &dyn View = &Memo::new(ProbeView { data: 5 });
        let reused = dispatch_view_update(&mut core, equal);

        assert!(
            reused,
            "dispatch must return true (reuse) for an equal Memo"
        );
        assert!(
            !core.is_dirty(),
            "element must NOT be dirty after an equality-bail skip"
        );
    }

    #[test]
    fn dispatch_rebuilds_on_different_memo() {
        let mut core: ElementCore<Memo<ProbeView>, Single> =
            ElementCore::new(Memo::new(ProbeView { data: 5 }));
        core.clear_dirty();

        let different: &dyn View = &Memo::new(ProbeView { data: 99 });
        let reused = dispatch_view_update(&mut core, different);

        assert!(
            reused,
            "dispatch must succeed (return true) for an unequal Memo"
        );
        assert!(
            core.is_dirty(),
            "element MUST be dirty after a non-skip update"
        );
    }

    // Stale-closure tripwire — documents the known limitation. A
    // `Memo<ViewWithCallback>` whose closure changed but whose data is
    // PartialEq-equal returns `true` from should_skip_rebuild, silently
    // keeping the stale handler. This ASSERTS that stale behavior so any
    // change which accidentally alters the semantics fails the test and
    // forces a doc update — rather than a silent contract change.
    #[derive(Clone)]
    struct ViewWithCallback {
        data: u32,
        // The callback cannot implement `PartialEq`; equality is computed
        // only on `data`, silently ignoring the handler — the trap.
        #[allow(dead_code)] // held to model a real callback-carrying view
        handler: std::sync::Arc<dyn Fn() + Send + Sync>,
    }

    impl PartialEq for ViewWithCallback {
        fn eq(&self, other: &Self) -> bool {
            self.data == other.data // handler intentionally ignored — the trap
        }
    }

    impl StatelessView for ViewWithCallback {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for ViewWithCallback {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::StatelessElement;
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }
    }

    #[test]
    fn stale_closure_tripwire_documents_known_limitation() {
        let a = Memo::new(ViewWithCallback {
            data: 1,
            handler: std::sync::Arc::new(|| {}),
        });
        let b = Memo::new(ViewWithCallback {
            data: 1, // same data, different handler
            handler: std::sync::Arc::new(|| {}),
        });

        // KNOWN LIMITATION: data equal → skip fires → stale handler kept.
        // This documents the broken invariant, not a desired behavior.
        assert!(
            a.should_skip_rebuild(&b),
            "TRIPWIRE: Memo skips when data is equal even though the handler \
             changed — documents the known stale-closure limitation of \
             PartialEq memoization for callback views. If this fails, the \
             behavior changed; update the rustdoc."
        );
    }
}

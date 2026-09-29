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

use super::view::View;

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
/// [`crate::element::ProxyElement`]/[`crate::element::ProxyBehavior`] and does
/// not introduce a new `ElementKind` variant.
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
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::proxy(self)
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

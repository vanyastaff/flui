//! Object-safe accessor protocol for InheritedElement.
//!
//! [`BuildContext::depend_on_inherited`](crate::BuildContext::depend_on_inherited) walks the element tree, finds
//! the nearest matching `InheritedElement<V>`, and needs to (a) read the
//! view as `&dyn Any` so the caller's downcast can succeed, and
//! (b) record the caller in the inherited element's dependent map.
//!
//! Because `BuildContext` is parameter-free at the trait surface
//! (object-safe `&dyn BuildContext`), it can't name `V`. The retained
//! element side, however, is parametric: each `InheritedElement<V>` is
//! a distinct concrete type at `V`-instantiation time. The bridge is a
//! small object-safe trait that exposes the two operations
//! `BuildContext` needs without leaking `V` into the trait surface.
//!
//! Flutter parity: `framework.dart:5081`
//! `dependOnInheritedWidgetOfExactType<T>` resolves the ancestor via
//! `_inheritedElements` lookup then invokes
//! `inheritedElement.updateDependencies(self, null)` — same shape.

use flui_foundation::ElementId;

/// Object-safe view of an `InheritedElement<V>` exposed to
/// `BuildContext` so the dependency-injection machinery can record
/// dependents and read the inherited view as `&dyn Any` without
/// naming the concrete `V`.
///
/// Implemented by `Element<V, Single, InheritedBehavior<V>>` (see
/// [`Element`](crate::element::Element)) in
/// `crates/flui-view/src/element/unified.rs`. The default `ElementBase`
/// hooks ([`ElementBase::as_inherited`] / [`as_inherited_mut`]) return
/// `None` for every other element type.
///
/// [`ElementBase::as_inherited`]: crate::view::ElementBase::as_inherited
/// [`as_inherited_mut`]: crate::view::ElementBase::as_inherited_mut
pub trait InheritedElementAccess {
    /// Borrow the inherited view as `&dyn Any` so the caller can
    /// downcast to the concrete `V` (the `InheritedView` type).
    ///
    /// This is the typed payload Flutter's `InheritedElement.widget`
    /// returns to the dependent's `BuildContext`.
    fn view_as_any(&self) -> &dyn std::any::Any;

    /// How many elements currently depend on this inherited element.
    ///
    /// **Diagnostics only, and deliberately not part of the stable surface** —
    /// `#[doc(hidden)]`, and classified as such in `docs/runtime-contract.toml`
    /// rather than left to grow unnoticed. It exists because whether a widget
    /// took an inherited dependency is a real behavioural property with no
    /// other observer: a widget depending on a `Directionality` it cannot use
    /// rebuilds on every direction change, and rebuild counting CANNOT see the
    /// difference, because changing an inherited value rebuilds the whole
    /// subtree regardless of who depends on what.
    ///
    /// It is the read half of the
    /// [`record_dependent`](Self::record_dependent) /
    /// [`remove_dependent`](Self::remove_dependent) pair this trait already
    /// carries, so it observes state the protocol already owns rather than
    /// adding new state. Adding it is not a practical break: `InheritedBehavior`
    /// is the only implementor, and an out-of-crate impl cannot be wired in —
    /// `as_inherited()` is produced by this crate's own element types.
    #[doc(hidden)]
    fn dependent_count(&self) -> usize;

    /// Register a dependent element with this `InheritedElement`.
    ///
    /// `depth` is the dependent's depth in the element tree, threaded
    /// through so a later
    /// [`InheritedBehavior::on_view_updated`](crate::element::InheritedBehavior)
    /// can call `ElementOwner::schedule_build_for` with
    /// [`RebuildReason::DependencyChange`](crate::RebuildReason::DependencyChange)
    /// without an extra tree traversal.
    ///
    /// Idempotent: re-registering the same id overwrites its depth
    /// (HashMap keyed by id) so reconciliation-driven depth changes are
    /// captured without leaving stale entries.
    fn record_dependent(&mut self, dependent: ElementId, depth: usize);

    /// Release a dependent during deactivate or unmount.
    ///
    /// The reverse ownership index supplies the exact provider ids, so
    /// lifecycle cleanup never scans the tree or waits for a later
    /// notification to prune stale entries. This mirrors Flutter's
    /// `InheritedElement.removeDependent`, invoked from `Element.deactivate`.
    /// No-op when the id is not registered.
    fn remove_dependent(&mut self, dependent: ElementId);
}

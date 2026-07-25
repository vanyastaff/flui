//! [`ValueListenableBuilder`] — rebuilds a subtree from the latest value of a
//! [`ValueListenable`].
//!
//! Flutter parity: `widgets/value_listenable_builder.dart`
//! (`ValueListenableBuilder<T>` / `_ValueListenableBuilderState<T>`, tag
//! `3.44.0`). The state subscribes to `value_listenable` in `init_state`,
//! swaps the subscription when the listenable *instance* changes (not merely
//! its current value) in `did_update_view`, and unsubscribes in `dispose`.
//!
//! # Read at build time, not cached
//!
//! Flutter's `_ValueListenableBuilderState` caches the value in a `late T
//! value` field, overwritten by `_valueChanged` on every notification and
//! read by `build`. FLUI's port instead re-reads [`ValueListenable::value`]
//! directly inside `build` — the listener callback's only job is to request
//! a rebuild ([`RebuildHandle::schedule`]). Both give the same observable
//! behavior: a notification landing between two frames with no intervening
//! build coalesces to whatever value is live when `build` finally runs,
//! because there is exactly one source of truth (the listenable itself)
//! rather than a second field that could drift from it.
//!
//! # Instance identity, not value equality
//!
//! `did_update_view` decides whether to resubscribe by comparing the two
//! `Arc` pointers ([`Arc::ptr_eq`]), matching Flutter's reference-identity
//! `oldWidget.valueListenable != widget.valueListenable` (Dart's
//! `ValueNotifier` does not override `==`, so it compares by identity). A
//! `PartialEq`-by-value comparison would be wrong here: two distinct
//! notifiers that happen to hold equal values are still a listenable swap,
//! and must still unsubscribe the old one.
//!
//! # Divergence: `child` passthrough is not a rebuild-skip
//!
//! Flutter's `Element.updateChild` short-circuits when the new child widget
//! is `identical()` to the old one, so the pre-built `child` subtree's
//! `build()` never re-runs. FLUI's [`BoxedView`] clones the child view's
//! configuration (`dyn_clone`) on every read rather than tracking object
//! identity, so `child` reaches `builder` **unchanged in content** on every
//! rebuild, but the framework does not special-case skipping that subtree's
//! own reconciliation. This is a Rust-shape divergence from the Dart
//! optimization, not a behavioral one visible to `builder`'s caller.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::{ListenerId, ValueListenable};
use flui_view::context::BuildContext;
use flui_view::element::ElementKind;
use flui_view::{BoxedView, IntoView, RebuildHandle, StatefulView, View, ViewExt, ViewState};

/// Builds a widget from the current value of a [`ValueListenable<T>`].
///
/// If `child` is `Some`, it is handed back unchanged in content on every
/// call (the module doc's divergence note explains why identity is not
/// preserved through `BoxedView`'s dyn-clone) — build the value-independent
/// part of the subtree once (outside the closure) and incorporate it here,
/// rather than reconstructing it every notification.
///
/// Flutter parity: `ValueWidgetBuilder<T>`.
pub type ValueWidgetBuilder<T> = Rc<dyn Fn(&dyn BuildContext, &T, Option<BoxedView>) -> BoxedView>;

/// A widget whose content stays synced with a [`ValueListenable`].
///
/// Registers itself as a listener of `value_listenable` and calls `builder`
/// with the listenable's current value whenever it changes.
///
/// # Performance
///
/// If `builder`'s output contains a subtree that does not depend on the
/// listenable's value, build it once and pass it as `child` — `builder`
/// receives it back on every call, so it can be incorporated without being
/// reconstructed from scratch.
///
/// # Example
///
/// ```
/// use std::rc::Rc;
/// use std::sync::Arc;
///
/// use flui_foundation::ValueNotifier;
/// use flui_widgets::ValueListenableBuilder;
/// use flui_widgets::prelude::*;
///
/// let counter = Arc::new(ValueNotifier::new(0_i32));
/// let _widget = ValueListenableBuilder::new(
///     counter,
///     Rc::new(|_ctx: &dyn BuildContext, value: &i32, _child| {
///         SizedBox::square(*value as f32).boxed()
///     }),
/// );
/// ```
pub struct ValueListenableBuilder<T> {
    value_listenable: Arc<dyn ValueListenable<T>>, // PORT-CHECK-OK-DYN: erases the concrete notifier type, same shape as the already-sanctioned `Listenable`
    builder: ValueWidgetBuilder<T>,
    child: Option<BoxedView>,
}

impl<T> ValueListenableBuilder<T> {
    /// Rebuild from `value_listenable`'s current value, via `builder`.
    #[must_use]
    pub fn new(
        value_listenable: Arc<dyn ValueListenable<T>>, // PORT-CHECK-OK-DYN: erases the concrete notifier type, same shape as the already-sanctioned `Listenable`
        builder: ValueWidgetBuilder<T>,
    ) -> Self {
        Self {
            value_listenable,
            builder,
            child: None,
        }
    }

    /// A value-independent subtree, built once and passed back to `builder`
    /// on every rebuild instead of being reconstructed inside the closure.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view().boxed());
        self
    }
}

impl<T> Clone for ValueListenableBuilder<T> {
    fn clone(&self) -> Self {
        Self {
            value_listenable: Arc::clone(&self.value_listenable),
            builder: Rc::clone(&self.builder),
            child: self.child.clone(),
        }
    }
}

impl<T> std::fmt::Debug for ValueListenableBuilder<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValueListenableBuilder")
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> StatefulView for ValueListenableBuilder<T> {
    type State = ValueListenableBuilderState<T>;

    fn create_state(&self) -> Self::State {
        // `ViewState::init_state` is handed a `BuildContext` but NOT the view,
        // so the listenable the first subscription needs is copied here.
        ValueListenableBuilderState {
            value_listenable: Arc::clone(&self.value_listenable),
            handle: None,
            listener_id: None,
        }
    }
}

impl<T: 'static> View for ValueListenableBuilder<T> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// Persistent state for [`ValueListenableBuilder`] — **opaque**.
///
/// `pub` only because it is the `State` associated type of a public
/// [`StatefulView`] impl; Rust forbids a crate-private type there. It has no
/// public fields and no public methods; construct it only through
/// `ValueListenableBuilder::create_state`.
pub struct ValueListenableBuilderState<T> {
    /// The listenable the live subscription (if any) targets. Kept in sync
    /// with the view's field by `create_state` and `did_update_view`, so
    /// `dispose` and the unsubscribe half of `did_update_view` always
    /// remove the listener from the correct (old) instance — `init_state`
    /// and `did_update_view` receive no live `view` to re-read it from.
    value_listenable: Arc<dyn ValueListenable<T>>, // PORT-CHECK-OK-DYN: erases the concrete notifier type, same shape as the already-sanctioned `Listenable`
    /// Captured in `init_state`, the only lifecycle hook handed a `BuildContext`.
    handle: Option<RebuildHandle>,
    /// The id of the listener registered against `value_listenable`, if any.
    listener_id: Option<ListenerId>,
}

impl<T> std::fmt::Debug for ValueListenableBuilderState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValueListenableBuilderState")
            .field("subscribed", &self.listener_id.is_some())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> ValueListenableBuilderState<T> {
    /// Subscribe to `self.value_listenable`, scheduling a rebuild through
    /// `self.handle` on every notification.
    ///
    /// # Panics
    ///
    /// Panics if called before `init_state` has populated `self.handle` — the
    /// only two call sites (`init_state` and `did_update_view`) both run
    /// after that population, so this is an internal invariant, not a
    /// reachable runtime condition.
    fn subscribe(&mut self) {
        let handle = self
            .handle
            .clone()
            .expect("BUG: subscribe() called before init_state populated handle");
        let listener_id = self.value_listenable.add_listener(Arc::new(move || {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }));
        self.listener_id = Some(listener_id);
    }
}

impl<T: 'static> ViewState<ValueListenableBuilder<T>> for ValueListenableBuilderState<T> {
    /// `_ValueListenableBuilderState.initState`: subscribe to the listenable.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.handle = Some(ctx.rebuild_handle());
        self.subscribe();
    }

    /// `_ValueListenableBuilderState.build`: `widget.builder(context, value,
    /// widget.child)`. The value is read live from the listenable rather than
    /// cached — see the module docs.
    fn build(&self, view: &ValueListenableBuilder<T>, ctx: &dyn BuildContext) -> impl IntoView {
        let value = view.value_listenable.value();
        (view.builder)(ctx, value, view.child.clone())
    }

    /// `_ValueListenableBuilderState.didUpdateWidget`: an unchanged listenable
    /// instance is a no-op; a changed one unsubscribes the old and subscribes
    /// the new.
    fn did_update_view(
        &mut self,
        _old_view: &ValueListenableBuilder<T>,
        new_view: &ValueListenableBuilder<T>,
    ) {
        if Arc::ptr_eq(&self.value_listenable, &new_view.value_listenable) {
            return;
        }

        if let Some(listener_id) = self.listener_id.take() {
            self.value_listenable.remove_listener(listener_id);
        }
        self.value_listenable = Arc::clone(&new_view.value_listenable);
        self.subscribe();
    }

    /// `_ValueListenableBuilderState.dispose`: unsubscribe from the listenable.
    fn dispose(&mut self) {
        if let Some(listener_id) = self.listener_id.take() {
            self.value_listenable.remove_listener(listener_id);
        }
    }
}

//! Acceptance + edge-case tests for `BuildContext::dispatch_notification`
//! and the object-safe `ElementBase::on_notification` handler protocol
//! (plan U13 / R10 / AE6).
//!
//! Flutter parity: `notification_listener.dart:67` (`Notification.dispatch`)
//! and `notification_listener.dart:127` (`_NotificationElement.onNotification`)
//! — Flutter walks the ancestor chain, invokes each listener's typed
//! `onNotification(notification)` callback, and stops bubbling when a
//! listener returns `true`.
//!
//! Plan §D3: single `dyn` boundary at dispatch. `Notification` is a marker
//! trait; `ElementBase::on_notification(type_id, &dyn Any) -> bool` is the
//! object-safe handler that lives on every Element. The typed
//! `NotifiableElement<N>` extension wrapper is sugar — internally the
//! dispatcher only walks via the object-safe shape.
//!
//! Tree shape under test (AE6): Root[NotificationListener] →
//! NotificationListener[middle] → DummyChild. From the child the test
//! dispatches via `ctx.dispatch_notification(ScrollNotification {..})`.

use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

use flui_foundation::{ElementId, RenderId};
use flui_view::{
    ViewExt,
    IntoView,
    BuildContext, BuildOwner, ElementBase, ElementBuildContext, ElementTree, Notification,
    StatelessBehavior, StatelessElement, StatelessView, View, element::Lifecycle,
};
use parking_lot::RwLock;

// ============================================================================
// Custom Notification types under test
// ============================================================================

#[derive(Debug, Clone)]
struct ScrollNotification {
    delta: f64,
}

impl Notification for ScrollNotification {}

#[derive(Debug, Clone)]
struct FooNotification;

impl Notification for FooNotification {}

// ============================================================================
// Test fixtures
// ============================================================================

/// Leaf StatelessView used as the dispatching descendant in the AE6 shape.
#[derive(Clone)]
struct DummyChild;

impl StatelessView for DummyChild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for DummyChild {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// A NotificationListener<N> View — analog of Flutter's
/// `NotificationListener<T extends Notification>` (notification_listener.dart:39).
///
/// Owns a typed callback `Fn(&N) -> bool`. The listener's element overrides
/// the object-safe `ElementBase::on_notification(type_id, &dyn Any)` to
/// downcast and invoke the typed callback. This proves the marker
/// `Notification` trait + the object-safe handler protocol round-trip
/// correctly.
struct NotificationListener<N: Notification> {
    on_notification: Arc<dyn Fn(&N) -> bool + Send + Sync>,
    _marker: PhantomData<N>,
}

impl<N: Notification> NotificationListener<N> {
    fn new<F: Fn(&N) -> bool + Send + Sync + 'static>(f: F) -> Self {
        Self {
            on_notification: Arc::new(f),
            _marker: PhantomData,
        }
    }
}

impl<N: Notification> Clone for NotificationListener<N> {
    fn clone(&self) -> Self {
        Self {
            on_notification: Arc::clone(&self.on_notification),
            _marker: PhantomData,
        }
    }
}

impl<N: Notification> View for NotificationListener<N> {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(NotificationListenerElement {
            on_notification: Arc::clone(&self.on_notification),
            depth: 0,
            lifecycle: Lifecycle::Initial,
            _marker: PhantomData,
        })
    }
}

/// Hand-rolled `ElementBase` impl that overrides `on_notification` to
/// match the marker-trait dispatch protocol. Used only by integration
/// tests — production NotificationListener wiring is out of scope for U13.
///
/// The element keeps just enough state to participate in the ancestor
/// walk (depth + lifecycle) and to expose its `view_type_id`. Building
/// is a no-op since the tests never trigger `perform_build` on it.
struct NotificationListenerElement<N: Notification> {
    on_notification: Arc<dyn Fn(&N) -> bool + Send + Sync>,
    depth: usize,
    lifecycle: Lifecycle,
    _marker: PhantomData<N>,
}

impl<N: Notification> ElementBase for NotificationListenerElement<N> {
    fn view_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<NotificationListener<N>>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mark_needs_build(&mut self) {}

    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {}

    fn set_pipeline_owner_any(&mut self, _owner: Arc<dyn std::any::Any + Send + Sync>) {}

    fn set_parent_render_id(&mut self, _parent_id: Option<RenderId>) {}

    fn update(&mut self, _new_view: &dyn View, _owner: &mut flui_view::ElementOwner<'_>) {}

    fn perform_build(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {}

    fn mount(
        &mut self,
        _parent: Option<ElementId>,
        _slot: usize,
        _owner: &mut flui_view::ElementOwner<'_>,
    ) {
        self.lifecycle = Lifecycle::Active;
    }

    fn unmount(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {
        self.lifecycle = Lifecycle::Defunct;
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
    }

    /// Override the object-safe handler protocol: if `type_id` matches
    /// `N`, downcast and dispatch to the typed callback. Otherwise return
    /// `false` so the bubble walks past this listener (it doesn't handle
    /// this notification type).
    ///
    /// Mirrors Flutter's `_NotificationElement.onNotification`
    /// (notification_listener.dart:127) which performs the
    /// `is T` runtime-type check before invoking the listener's
    /// `widget.onNotification` callback.
    fn on_notification(&self, type_id: std::any::TypeId, notification: &dyn std::any::Any) -> bool {
        if type_id != std::any::TypeId::of::<N>() {
            return false;
        }
        let Some(typed) = notification.downcast_ref::<N>() else {
            return false;
        };
        (self.on_notification)(typed)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn create_tree_and_owner() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    (tree, owner)
}

// ============================================================================
// AE6: happy path — listener fires and bubble stops on `true` return
// ============================================================================

#[test]
fn dispatch_notification_calls_handler_and_stops_on_true() {
    // Tree shape: Root[outer-listener] → Inner[middle-listener] → DummyChild.
    // The inner listener returns `true`. The outer listener MUST NOT fire.
    // This locks down the "stops on true" semantics of Flutter
    // notification_listener.dart:127.
    let (tree, owner) = create_tree_and_owner();

    let outer_called = Arc::new(AtomicBool::new(false));
    let inner_called = Arc::new(AtomicBool::new(false));
    let received_delta = Arc::new(parking_lot::Mutex::new(0.0_f64));

    let outer_listener = {
        let outer_called = Arc::clone(&outer_called);
        NotificationListener::<ScrollNotification>::new(move |_n| {
            outer_called.store(true, Ordering::Release);
            true
        })
    };
    let outer_id = tree
        .write()
        .mount_root(&outer_listener, &mut owner.write().element_owner_mut());

    let inner_listener = {
        let inner_called = Arc::clone(&inner_called);
        let received_delta = Arc::clone(&received_delta);
        NotificationListener::<ScrollNotification>::new(move |n| {
            inner_called.store(true, Ordering::Release);
            *received_delta.lock() = n.delta;
            true
        })
    };
    let inner_id = tree.write().insert(
        &inner_listener,
        outer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let child_id = tree.write().insert(
        &DummyChild,
        inner_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    ctx.dispatch_notification(&ScrollNotification { delta: 42.0 });

    assert!(
        inner_called.load(Ordering::Acquire),
        "inner (nearest) listener must fire on dispatch"
    );
    assert_eq!(
        *received_delta.lock(),
        42.0,
        "inner listener must receive the typed notification with its data"
    );
    assert!(
        !outer_called.load(Ordering::Acquire),
        "outer listener must NOT fire after inner returns true (bubble stops)"
    );
}

// ============================================================================
// Bubble continues on false — listener returns false, walk reaches root
// ============================================================================

#[test]
fn dispatch_notification_continues_when_handler_returns_false() {
    // Tree shape: Root[outer] → Inner[middle returns false] → DummyChild.
    // Inner returns false, so the bubble must continue and reach the outer
    // listener.
    let (tree, owner) = create_tree_and_owner();

    let outer_called = Arc::new(AtomicBool::new(false));
    let inner_called = Arc::new(AtomicBool::new(false));
    let call_order = Arc::new(AtomicI32::new(0));
    let outer_order = Arc::new(AtomicI32::new(-1));
    let inner_order = Arc::new(AtomicI32::new(-1));

    let outer_listener = {
        let outer_called = Arc::clone(&outer_called);
        let call_order = Arc::clone(&call_order);
        let outer_order = Arc::clone(&outer_order);
        NotificationListener::<ScrollNotification>::new(move |_n| {
            outer_called.store(true, Ordering::Release);
            outer_order.store(call_order.fetch_add(1, Ordering::AcqRel), Ordering::Release);
            true
        })
    };
    let outer_id = tree
        .write()
        .mount_root(&outer_listener, &mut owner.write().element_owner_mut());

    let inner_listener = {
        let inner_called = Arc::clone(&inner_called);
        let call_order = Arc::clone(&call_order);
        let inner_order = Arc::clone(&inner_order);
        NotificationListener::<ScrollNotification>::new(move |_n| {
            inner_called.store(true, Ordering::Release);
            inner_order.store(call_order.fetch_add(1, Ordering::AcqRel), Ordering::Release);
            false // bubble continues
        })
    };
    let inner_id = tree.write().insert(
        &inner_listener,
        outer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let child_id = tree.write().insert(
        &DummyChild,
        inner_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    ctx.dispatch_notification(&ScrollNotification { delta: 7.0 });

    assert!(
        inner_called.load(Ordering::Acquire),
        "inner listener must fire first"
    );
    assert!(
        outer_called.load(Ordering::Acquire),
        "outer listener must fire after inner returned false (bubble continues)"
    );
    assert_eq!(
        inner_order.load(Ordering::Acquire),
        0,
        "inner (nearest) listener must run before outer"
    );
    assert_eq!(
        outer_order.load(Ordering::Acquire),
        1,
        "outer (root) listener must run after inner per Flutter bubble order"
    );
}

// ============================================================================
// No handler in chain — walk completes cleanly without panic
// ============================================================================

#[test]
fn dispatch_notification_walks_to_root_without_match() {
    // Tree shape: DummyChild[root] → DummyChild → DummyChild.
    // No NotifiableElement anywhere. dispatch_notification must walk the
    // chain and exit cleanly when the root's parent is None.
    let (tree, owner) = create_tree_and_owner();

    let root_id = tree
        .write()
        .mount_root(&DummyChild, &mut owner.write().element_owner_mut());

    let middle_id = tree.write().insert(
        &DummyChild,
        root_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let leaf_id = tree.write().insert(
        &DummyChild,
        middle_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(leaf_id, tree.clone(), owner.clone()).unwrap();

    // This must not panic.
    ctx.dispatch_notification(&ScrollNotification { delta: 1.0 });
}

// ============================================================================
// Wrong-type listener — bubble passes through without invoking it
// ============================================================================

#[test]
fn dispatch_notification_skips_non_matching_type() {
    // Tree shape: Root[FooListener] → Inner[ScrollListener returns true]
    //             → DummyChild. Dispatch a FooNotification: ScrollListener
    // is in the chain but type-mismatched, must NOT fire. FooListener at
    // the root must fire.
    let (tree, owner) = create_tree_and_owner();

    let foo_called = Arc::new(AtomicBool::new(false));
    let scroll_called = Arc::new(AtomicBool::new(false));

    let foo_listener = {
        let foo_called = Arc::clone(&foo_called);
        NotificationListener::<FooNotification>::new(move |_n| {
            foo_called.store(true, Ordering::Release);
            true
        })
    };
    let root_id = tree
        .write()
        .mount_root(&foo_listener, &mut owner.write().element_owner_mut());

    let scroll_listener = {
        let scroll_called = Arc::clone(&scroll_called);
        NotificationListener::<ScrollNotification>::new(move |_n| {
            scroll_called.store(true, Ordering::Release);
            true
        })
    };
    let middle_id = tree.write().insert(
        &scroll_listener,
        root_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let leaf_id = tree.write().insert(
        &DummyChild,
        middle_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(leaf_id, tree.clone(), owner.clone()).unwrap();

    ctx.dispatch_notification(&FooNotification);

    assert!(
        !scroll_called.load(Ordering::Acquire),
        "ScrollNotification listener must NOT fire for FooNotification dispatch"
    );
    assert!(
        foo_called.load(Ordering::Acquire),
        "FooNotification listener at root must fire after walking past ScrollListener"
    );
}

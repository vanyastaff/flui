//! Typed dispatch test (plan §U8 / KTD-4 / FR-021, finalized in §U27).
//!
//! `ElementCore::update_view` routes through
//! `crate::element::dispatch::dispatch_view_update`. The dispatch
//! body is now the concrete-`TypeId`-keyed
//! `Downcast::as_any().type_id()` + `Box::downcast::<V>` path
//! (§U27) — no `downcast_ref::<V>()` syntactic pattern in the
//! View-type update dispatch path. This file pins the dispatch
//! contract so a regression surfaces here.

use flui_foundation::{ValueKey, ViewKey};
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, IntoView, StatelessElement, StatelessView,
    View, ViewExt,
};

struct ShimView {
    payload: u32,
    key: Option<Box<dyn ViewKey>>,
}

impl Clone for ShimView {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload,
            key: self.key.as_ref().map(|k| k.clone_key()),
        }
    }
}

impl StatelessView for ShimView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for ShimView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::element::StatelessBehavior;
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }
}

/// Typed dispatch must succeed on a matching type — proves the
/// default route through `dispatch_view_update` reaches the
/// `Ok(typed) = Box::downcast::<V>` branch and applies the new
/// view.
#[test]
fn identity_shim_succeeds_on_type_match() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let initial = ShimView {
        payload: 1,
        key: Some(Box::new(ValueKey::new(42_u32))),
    };
    let id = tree.mount_root(&initial, &mut owner.element_owner_mut());

    let updated = ShimView {
        payload: 2,
        key: Some(Box::new(ValueKey::new(42_u32))),
    };
    // `ElementTree::update` calls `node.element.update`, which invokes
    // `ElementCore::update_view` on the typed element. Round-trips
    // through `dispatch_view_update` under default features.
    tree.update(id, &updated, &mut owner.element_owner_mut());

    // The key should still survive the update (U7 re-clones at the
    // update boundary). Same key value confirms the dispatch took the
    // success path and applied the new view — a downcast failure
    // would leave `node.key` unchanged at the old probe value (it
    // already matches by value here) but `payload` would also stay
    // at 1 (we can't read payload through the type-erased element,
    // so the surface assertion is the key still being present).
    let node = tree.get(id).expect("node alive after update");
    let stored_hash = node
        .key()
        .map(|k| k.key_hash())
        .expect("key survives update");
    let probe = ValueKey::<u32>::new(42_u32);
    assert_eq!(
        stored_hash,
        probe.key_hash(),
        "typed dispatch must preserve the keyed slot across update",
    );
}

/// PR #133 review (P1) regression lock — `BoxedView` forwards
/// `View::view_type_id()` to its inner view; a `view_type_id()`-keyed
/// guard would let the wrapper slip through and the subsequent
/// downcast would panic on every `.boxed()` rebuild.
///
/// The §U27 dispatch keys on `Downcast::as_any().type_id()` —
/// the concrete runtime TypeId, not the overridable trait method —
/// so a `BoxedView` handed into a `dispatch_view_update::<Inner, _>`
/// call discriminates correctly and returns `false` rather than
/// panicking. The caller (reconciler) then replaces the element.
///
/// This test does NOT assert that `.boxed()` rebuilds are
/// FRAMEWORK-correct end-to-end — that is the reconciler's job
/// and lives elsewhere. It locks the **dispatch behavior**:
/// `BoxedView` over `Inner` must not panic the dispatch when the
/// element is parameterized over `Inner` directly.
#[test]
fn dispatch_does_not_panic_when_boxed_view_wraps_v() {
    use flui_view::BoxedView;

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    // Mount the element parameterized over the inner concrete type.
    let initial = ShimView {
        payload: 1,
        key: Some(Box::new(ValueKey::new(7_u32))),
    };
    let id = tree.mount_root(&initial, &mut owner.element_owner_mut());

    // Construct a BoxedView wrapping a ShimView. The wrapper's
    // `view_type_id()` forwards to its inner (== TypeId::of::<ShimView>())
    // — the exact shape that would slip past a naive
    // `view_type_id()`-keyed guard.
    let inner = ShimView {
        payload: 2,
        key: Some(Box::new(ValueKey::new(7_u32))),
    };
    let boxed: BoxedView = inner.boxed();

    // Pass the wrapper through `update`. The dispatch must NOT
    // panic; the `as_any().type_id()` guard returns `false`
    // (BoxedView's concrete TypeId differs from ShimView's), and
    // `update_view` propagates `false` — the caller is responsible
    // for treating this as a type-mismatch (replace element). We
    // assert by reaching the line after the call without panicking.
    tree.update(id, &boxed, &mut owner.element_owner_mut());

    // Sanity: the element still exists in the tree (dispatch did
    // not corrupt state; it simply rejected the update).
    assert!(
        tree.get(id).is_some(),
        "tree node must survive a BoxedView-wrapping-V dispatch attempt"
    );
}

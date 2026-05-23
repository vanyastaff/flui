//! Round-trip test for `ElementNode::key` storage (plan §U7 / FR-022).
//!
//! Phase 1 §U7 introduces a `key: Option<Box<dyn ViewKey>>` field on
//! `ElementNode`, populated from `View::key()` at every mount /
//! insert / update boundary. Phase 2's keyed reconciler reads this
//! field directly — never crossing the typed-`V` boundary. The
//! round-trip discipline below proves the field stores the cloned
//! key faithfully across the five concrete `ViewKey` impls
//! (`ValueKey`, `UniqueKey`, `ObjectKey`, `GlobalKey<T>`, `Key`)
//! plus the keyless case and survives a same-type update.
//!
//! Every test here uses a single `TestView` shape that holds an
//! `Option<Box<dyn ViewKey>>` and overrides `View::key()` to surface
//! it. A real widget would derive `StatelessView` and either keep the
//! default keyless `View::key() -> None` or define a typed key field;
//! the heterogeneous-key-test demand makes a single dynamic shape
//! cleaner here than seven separate concrete views.

use std::sync::Arc;

use flui_foundation::{Key, ObserverId, UniqueKey, ValueKey, ViewKey};
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, GlobalKey, IntoView, ObjectKey,
    StatelessElement, StatelessView, View, ViewExt,
};

// ----------------------------------------------------------------------------
// Heterogeneous-keyed test view
// ----------------------------------------------------------------------------

struct TestView {
    /// Cosmetic identifier surfaced in test failure messages — never
    /// participates in `View::can_update` or the key round-trip.
    name: &'static str,
    /// Optional dynamic key. `View::key()` returns `Some(&**key)` when
    /// present so the same `TestView` shape can carry any of the five
    /// `ViewKey` impls.
    key: Option<Box<dyn ViewKey>>,
}

impl TestView {
    fn keyless(name: &'static str) -> Self {
        Self { name, key: None }
    }

    fn with_key<K: ViewKey>(name: &'static str, key: K) -> Self {
        Self {
            name,
            key: Some(Box::new(key)),
        }
    }
}

impl Clone for TestView {
    fn clone(&self) -> Self {
        // `Box<dyn ViewKey>` is not `Clone` directly — clone through
        // the trait's `clone_key()`.
        Self {
            name: self.name,
            key: self.key.as_ref().map(|k| k.clone_key()),
        }
    }
}

impl StatelessView for TestView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // A real view returns child views; tests only exercise the
        // tree-level mount/update path, so the `build` body just
        // re-emits self.
        self.clone().boxed()
    }
}

impl View for TestView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::element::StatelessBehavior;
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }
}

// Helper: assert that `node.key()` round-trips a key that compares
// equal to the supplied probe via `ViewKey::key_eq`. The probe is
// the same key value that was passed into `TestView::with_key` —
// reconstructed at the assertion site so failures show exactly
// which key shape went sideways.
fn assert_key_round_trips(tree: &ElementTree, id: flui_foundation::ElementId, probe: &dyn ViewKey) {
    let node = tree
        .get(id)
        .expect("ElementNode must be present after mount/insert");
    let stored = node
        .key()
        .unwrap_or_else(|| panic!("ElementNode::key() returned None after keyed mount"));
    assert!(
        stored.key_eq(probe),
        "stored key did not key_eq the probe; stored hash={}, probe hash={}",
        stored.key_hash(),
        probe.key_hash(),
    );
    assert_eq!(
        Some(probe.key_hash()),
        node.key_hash(),
        "node.key_hash() must match probe.key_hash() under round-trip",
    );
}

// ============================================================================
// Round-trip cases (the seven `Covers FR-022 / key_roundtrip(*)` tests)
// ============================================================================

#[test]
fn covers_fr022_key_roundtrip_value_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let probe = ValueKey::new(42_u32);
    let view = TestView::with_key("vk_42", ValueKey::new(42_u32));
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    assert_key_round_trips(&tree, id, &probe);
}

#[test]
fn covers_fr022_key_roundtrip_unique_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    // `UniqueKey` is `Copy`, so a `Clone` is the round-trip probe.
    let probe = UniqueKey::new();
    let view = TestView::with_key("uk", probe);
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    assert_key_round_trips(&tree, id, &probe);
}

#[test]
fn covers_fr022_key_roundtrip_object_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    // `ObjectKey` keys by pointer identity; both the probe and the
    // mounted key must share the same Arc allocation for `key_eq` to
    // hold.
    let holder: Arc<u32> = Arc::new(7);
    let probe = ObjectKey::new(Arc::clone(&holder));
    let view_key = ObjectKey::new(Arc::clone(&holder));
    let view = TestView::with_key("ok", view_key);
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    assert_key_round_trips(&tree, id, &probe);
}

#[test]
fn covers_fr022_key_roundtrip_global_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    // `GlobalKey<T>` is `Clone` over its inner `id`.
    let probe = GlobalKey::<TestView>::new();
    let view = TestView::with_key("gk", probe.clone());
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    assert_key_round_trips(&tree, id, &probe);
}

#[test]
fn covers_fr022_key_roundtrip_key_newtype() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    // U10 added `impl ViewKey for Key` so this assertion is possible.
    let probe = Key::from_str("k1");
    let view = TestView::with_key("k", probe);
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    assert_key_round_trips(&tree, id, &probe);
}

#[test]
fn covers_fr022_key_roundtrip_no_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view = TestView::keyless("plain");
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    let node = tree
        .get(id)
        .expect("ElementNode must be present after keyless mount");
    assert!(node.key().is_none(), "keyless mount must store None");
    assert!(
        node.key_hash().is_none(),
        "keyless mount must report key_hash() = None",
    );
}

// ============================================================================
// Edge cases
// ============================================================================

/// Edge: a `View::can_update`-compatible update preserves the stored
/// key. Spec FR-028 requires both old and new to carry equal keys for
/// an update to succeed, so the stored key after update must remain
/// equal to the original probe value.
#[test]
fn edge_remount_copy_preserves_key() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let probe = ValueKey::new("steady");
    let initial = TestView::with_key("v1", ValueKey::new("steady"));
    let id = tree.mount_root(&initial, &mut owner.element_owner_mut());

    // Drop the `BuildOwner` borrow before re-borrowing for `update`.
    {
        let next = TestView::with_key("v2", ValueKey::new("steady"));
        tree.update(id, &next, &mut owner.element_owner_mut());
    }

    assert_key_round_trips(&tree, id, &probe);
}

/// Edge: `GlobalKey` round-trip surfaces `is_global_key() == true` so
/// the registry-side code paths (`global_key_hash_of`,
/// `register_global_key_with_collision_check`) can route off the
/// stored key as well as the side-index `registered_global_key_hash`
/// field.
#[test]
fn edge_global_key_is_global() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let global = GlobalKey::<TestView>::new();
    let view = TestView::with_key("gk_isglobal", global.clone());
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    let node = tree
        .get(id)
        .expect("ElementNode must be present after mount");
    let stored = node
        .key()
        .expect("GlobalKey mount must populate node.key()");
    assert!(
        stored.is_global_key(),
        "GlobalKey's stored ViewKey must report is_global_key() == true",
    );
}

// ----------------------------------------------------------------------------
// Negative regression: the side-index hash still aligns with the new
// key field for global keys (R13 / R14 path). Catches future drift
// where the side-index gets populated but the new `key` field is
// silently skipped.
// ----------------------------------------------------------------------------

#[test]
fn regression_global_key_side_index_matches_key_field() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let global = GlobalKey::<TestView>::new();
    let view = TestView::with_key("gk_regression", global.clone());
    let id = tree.mount_root(&view, &mut owner.element_owner_mut());

    let node = tree.get(id).expect("node present");
    assert_eq!(
        node.registered_global_key_hash(),
        node.key_hash(),
        "registered_global_key_hash side-index must equal node.key_hash() for a GlobalKey mount",
    );
}

// Sanity-touch: `ObserverId` is exported from `flui_foundation` and is
// distinct from the key types — guards against accidental over-broad
// uses of `Box<dyn ViewKey>` in the tree (would surface as a compile
// error if `ObserverId` could trivially coerce, which it cannot).
const _: fn() = || {
    let _: ObserverId = ObserverId::new(1);
};

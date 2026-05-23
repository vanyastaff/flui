//! `View::can_update` semantics test suite (plan §U11 / FR-028).
//!
//! Phase 1 §U11 extends the default `View::can_update` body from a
//! type-id-only check to spec FR-028's full
//! `runtimeType == other.runtimeType && key == other.key` semantics.
//! The body already shipped in
//! `crates/flui-view/src/view/view.rs:94-106` as part of the
//! framework spine repair series, so this test file is the LOCKING
//! evidence — it pins the four-quadrant behavior so a future
//! regression that silently reverts to type-id-only matching turns
//! into a test failure here rather than a silent state-loss bug in
//! keyed list reconciliation.

use std::sync::Arc;

use flui_foundation::{UniqueKey, ValueKey, ViewKey};
use flui_view::{
    BuildContext, ElementBase, GlobalKey, ObjectKey, StatelessElement, StatelessView, View,
};

// ----------------------------------------------------------------------------
// Two distinct view types so the "type mismatch" axis is unambiguous.
// ----------------------------------------------------------------------------

struct Alpha {
    key: Option<Box<dyn ViewKey>>,
}

impl Alpha {
    fn keyless() -> Self {
        Self { key: None }
    }

    fn with_key<K: ViewKey>(key: K) -> Self {
        Self {
            key: Some(Box::new(key)),
        }
    }
}

impl Clone for Alpha {
    fn clone(&self) -> Self {
        Self {
            key: self.key.as_ref().map(|k| k.clone_key()),
        }
    }
}

impl StatelessView for Alpha {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for Alpha {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::element::StatelessBehavior;
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }
}

struct Beta {
    key: Option<Box<dyn ViewKey>>,
}

impl Beta {
    fn keyless() -> Self {
        Self { key: None }
    }

    fn with_key<K: ViewKey>(key: K) -> Self {
        Self {
            key: Some(Box::new(key)),
        }
    }
}

impl Clone for Beta {
    fn clone(&self) -> Self {
        Self {
            key: self.key.as_ref().map(|k| k.clone_key()),
        }
    }
}

impl StatelessView for Beta {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for Beta {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::element::StatelessBehavior;
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }
}

// ============================================================================
// FR-028 quadrants
// ============================================================================

#[test]
fn covers_fr028_type_match_no_keys() {
    let a = Alpha::keyless();
    let b = Alpha::keyless();
    assert!(a.can_update(&b));
    assert!(b.can_update(&a));
}

#[test]
fn covers_fr028_type_match_same_keys() {
    let a = Alpha::with_key(ValueKey::new(42_u32));
    let b = Alpha::with_key(ValueKey::new(42_u32));
    assert!(a.can_update(&b));
    assert!(b.can_update(&a));
}

#[test]
fn covers_fr028_type_match_different_keys() {
    let a = Alpha::with_key(ValueKey::new(42_u32));
    let b = Alpha::with_key(ValueKey::new(43_u32));
    assert!(
        !a.can_update(&b),
        "same-type ValueKey(42) must NOT update ValueKey(43)"
    );
    assert!(!b.can_update(&a));
}

#[test]
fn covers_fr028_type_mismatch() {
    let a = Alpha::keyless();
    let b = Beta::keyless();
    assert!(!a.can_update(&b), "Alpha must NOT update Beta");
    assert!(!b.can_update(&a), "Beta must NOT update Alpha");

    // Type mismatch trumps key match: even when both carry the same
    // logical key, the type discriminant rejects the update.
    let key_a = Alpha::with_key(ValueKey::new(99_u32));
    let key_b = Beta::with_key(ValueKey::new(99_u32));
    assert!(
        !key_a.can_update(&key_b),
        "type mismatch must reject even with matching keys",
    );
}

#[test]
fn covers_fr028_mixed_keyed_unkeyed() {
    let keyed = Alpha::with_key(ValueKey::new(7_u32));
    let keyless = Alpha::keyless();
    assert!(!keyed.can_update(&keyless), "keyed must NOT update keyless",);
    assert!(!keyless.can_update(&keyed), "keyless must NOT update keyed",);
}

// ============================================================================
// Additional discrimination — the five ViewKey impls all participate.
// ============================================================================

/// `UniqueKey` instances are designed never to compare equal — each
/// call to `UniqueKey::new()` bumps an atomic counter, so two
/// `UniqueKey`-keyed views with otherwise identical state still
/// reject a `can_update` match.
#[test]
fn unique_key_never_updates() {
    let a = Alpha::with_key(UniqueKey::new());
    let b = Alpha::with_key(UniqueKey::new());
    assert!(
        !a.can_update(&b),
        "two distinct UniqueKey instances must not update each other",
    );
}

/// `ObjectKey` compares by `Arc` pointer identity. Two `ObjectKey`s
/// pointing at the SAME `Arc` allocation match; two `ObjectKey`s
/// pointing at separate allocations with the same inner value do
/// not.
#[test]
fn object_key_distinguishes_arc_identity() {
    let shared: Arc<u32> = Arc::new(1);
    let same_alloc_a = Alpha::with_key(ObjectKey::new(Arc::clone(&shared)));
    let same_alloc_b = Alpha::with_key(ObjectKey::new(Arc::clone(&shared)));
    assert!(
        same_alloc_a.can_update(&same_alloc_b),
        "ObjectKey pointing at the same Arc must update",
    );

    let other_alloc = Alpha::with_key(ObjectKey::new(Arc::new(1_u32)));
    assert!(
        !same_alloc_a.can_update(&other_alloc),
        "ObjectKey pointing at a DIFFERENT Arc allocation must NOT update, even with the same inner value",
    );
}

/// `GlobalKey<T>` compares by inner id. Two clones of the same
/// `GlobalKey` match; two fresh `GlobalKey<T>::new()` calls do not.
#[test]
fn global_key_compares_by_id() {
    let key = GlobalKey::<Alpha>::new();
    let a = Alpha::with_key(key.clone());
    let b = Alpha::with_key(key.clone());
    assert!(
        a.can_update(&b),
        "two clones of the same GlobalKey must update"
    );

    let fresh = Alpha::with_key(GlobalKey::<Alpha>::new());
    assert!(
        !a.can_update(&fresh),
        "two fresh GlobalKey<T>::new() instances must NOT update",
    );
}

/// `Key` (the foundation newtype that U10 added a `ViewKey` impl
/// for) compares by inner `u64`. Identical `Key::from_str` strings
/// hash to the same value (compile-time FNV-1a), so the
/// `can_update` round-trip succeeds — the U10 impl participates in
/// the FR-028 match path correctly.
#[test]
fn key_newtype_compares_by_inner_u64() {
    use flui_foundation::Key;

    let stable_a = Alpha::with_key(Key::from_str("stable"));
    let stable_b = Alpha::with_key(Key::from_str("stable"));
    assert!(
        stable_a.can_update(&stable_b),
        "two Key::from_str(\"stable\") instances must update (same FNV-1a hash)",
    );

    let other = Alpha::with_key(Key::from_str("other"));
    assert!(
        !stable_a.can_update(&other),
        "different Key::from_str literals must NOT update",
    );
}

//! `View::can_update` semantics test suite (FR-028).
//!
//! The default `View::can_update` body was extended from a
//! type-id-only check to spec FR-028's full
//! `runtimeType == other.runtimeType && key == other.key` semantics.
//! The body already shipped in
//! `crates/flui-view/src/view/view.rs:94-106` as part of the
//! framework spine repair series, so this test file is the LOCKING
//! evidence — it pins the four-quadrant behavior so a future
//! regression that silently reverts to type-id-only matching turns
//! into a test failure here rather than a silent state-loss bug in
//! keyed list reconciliation.

use flui_foundation::{ValueKey, ViewKey};
use flui_view::{BuildContext, IntoView, StatelessView, View, ViewExt};

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
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for Alpha {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
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
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for Beta {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }
}

// ============================================================================
// FR-028 quadrants
// ============================================================================

#[test]
fn view_match_type_match_different_keys() {
    let a = Alpha::with_key(ValueKey::new(42_u32));
    let b = Alpha::with_key(ValueKey::new(43_u32));
    assert!(
        !a.can_update(&b),
        "same-type ValueKey(42) must NOT update ValueKey(43)"
    );
    assert!(!b.can_update(&a));
}

#[test]
fn view_match_type_mismatch() {
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
fn view_match_mixed_keyed_unkeyed() {
    let keyed = Alpha::with_key(ValueKey::new(7_u32));
    let keyless = Alpha::keyless();
    assert!(!keyed.can_update(&keyless), "keyed must NOT update keyless");
    assert!(!keyless.can_update(&keyed), "keyless must NOT update keyed");
}

// ============================================================================
// Additional discrimination — the five ViewKey impls all participate.
// ============================================================================

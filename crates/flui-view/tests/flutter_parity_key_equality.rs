//! Ported from `.flutter/flutter-master/packages/flutter/test/widgets/key_test.dart`.
//!
//! Flutter's widgets/key_test.dart is a SINGLE `testWidgets('Keys', ...)`
//! block with ~15 `expect()` calls covering the `LocalKey` /
//! `ValueKey<T>` / `UniqueKey` / `ObjectKey` / `GlobalKey` equality
//! contracts. The Dart-specific cases (Flutter's `Key` being an
//! alias for `ValueKey<String>`, generic type variance with
//! `ValueKey<num>` vs `ValueKey<int>`, NaN-not-equal-to-NaN with
//! `ValueKey<double>`) translate differently or not at all into
//! FLUI's Rust idioms:
//!
//! - FLUI's `Key` is a distinct `NonZeroU64` newtype (not a
//!   `ValueKey<String>` alias). FLUI's `ValueKey<T>` mixes
//!   `TypeId::of::<T>()` into the hash to keep cross-generic-
//!   parameter collisions disjoint — that's a stronger contract
//!   than Flutter's Dart-runtime-type check.
//! - `f64::NaN`-as-key is structurally invalid in FLUI: `ValueKey<T>`
//!   requires `T: Eq` and `f64` is not `Eq`. The Flutter case has
//!   no FLUI equivalent.
//! - Flutter's `TestValueKey<String>` subclassing pattern uses Dart's
//!   nominal inheritance; FLUI uses generic monomorphisation, so
//!   `ValueKey<u32>` and `ValueKey<i32>` ARE distinct types via
//!   `TypeId` (already locked in `test_key_view_key_eq_rejects_cross_type`
//!   and `flui-foundation`'s `test_key_view_key_hash_determinism`).
//!
//! What lands here: the EQUALITY contracts that DO translate.

#![cfg(feature = "test-utils")]

use std::sync::Arc;

use flui_foundation::{Key, UniqueKey, ValueKey, ViewKey};
use flui_view::ObjectKey;

// ============================================================================
// Ported: `ValueKey<int>(3) == ValueKey<int>(3)` is true
//         `ValueKey<int>(3) == ValueKey<int>(2)` is false
// ============================================================================

#[test]
fn value_key_same_value_same_type_equals() {
    let a: &dyn ViewKey = &ValueKey::new(3_i32);
    let b: &dyn ViewKey = &ValueKey::new(3_i32);
    assert!(a.key_eq(b));
}

#[test]
fn value_key_different_value_same_type_not_equals() {
    let a: &dyn ViewKey = &ValueKey::new(3_i32);
    let b: &dyn ViewKey = &ValueKey::new(2_i32);
    assert!(!a.key_eq(b));
}

// ============================================================================
// Ported (variant): `ValueKey<num>(3) == ValueKey<int>(3)` is false
//
// Flutter's case uses Dart's generic type variance with `num` as
// the abstract supertype of `int`. FLUI's monomorphic Rust generics
// have no analogue — different generic parameters ARE different
// types via `TypeId`. The translation: `ValueKey<u32>(3) !=
// ValueKey<i32>(3)` even though the numeric values are equal.
// ============================================================================

#[test]
fn value_key_different_generic_parameter_not_equals() {
    let a: &dyn ViewKey = &ValueKey::new(3_u32);
    let b: &dyn ViewKey = &ValueKey::new(3_i32);
    assert!(
        !a.key_eq(b),
        "ValueKey<u32>(3) must NOT equal ValueKey<i32>(3) — TypeId mixing in key_hash",
    );
    assert!(!b.key_eq(a));
}

// ============================================================================
// Ported: `UniqueKey() == UniqueKey()` is false
//         `let k = UniqueKey(); k == k` is true
// ============================================================================

#[test]
fn unique_key_two_distinct_instances_not_equals() {
    let a: &dyn ViewKey = &UniqueKey::new();
    let b: &dyn ViewKey = &UniqueKey::new();
    assert!(!a.key_eq(b));
}

#[test]
fn unique_key_self_equals_self() {
    let k = UniqueKey::new();
    let a: &dyn ViewKey = &k;
    let b: &dyn ViewKey = &k;
    assert!(a.key_eq(b));
}

// ============================================================================
// Ported: `ObjectKey(k) == ObjectKey(k)` is true (when k is the same
// object)
//
// Flutter compares by reference identity; FLUI compares by `Arc`
// pointer identity. Two `ObjectKey`s pointing at the SAME `Arc`
// match; two `ObjectKey`s wrapping the same value in different `Arc`
// allocations do NOT match.
// ============================================================================

#[test]
fn object_key_same_arc_equals() {
    let shared: Arc<u32> = Arc::new(42);
    let a: &dyn ViewKey = &ObjectKey::new(Arc::clone(&shared));
    let b: &dyn ViewKey = &ObjectKey::new(Arc::clone(&shared));
    assert!(
        a.key_eq(b),
        "two ObjectKeys backed by the same Arc must match"
    );
}

#[test]
fn object_key_distinct_arcs_same_inner_not_equals() {
    let a: &dyn ViewKey = &ObjectKey::new(Arc::new(42_u32));
    let b: &dyn ViewKey = &ObjectKey::new(Arc::new(42_u32));
    assert!(
        !a.key_eq(b),
        "two ObjectKeys with DIFFERENT Arc allocations must NOT match, even if inner value is equal",
    );
}

// ============================================================================
// FLUI-specific addition (not in Flutter's key_test.dart): the
// foundation `Key` newtype distinguishes itself from `ValueKey`.
// Flutter aliases `Key` as `ValueKey<String>`; FLUI's `Key` is a
// `NonZeroU64` newtype with its own `ViewKey` impl.
// Cross-impl matches must reject.
// ============================================================================

#[test]
fn flui_key_does_not_equal_value_key_string() {
    let k: &dyn ViewKey = &Key::from_str("a");
    let vk: &dyn ViewKey = &ValueKey::new("a".to_owned());
    assert!(
        !k.key_eq(vk),
        "FLUI's Key newtype is NOT an alias for ValueKey<String>; cross-impl match must reject",
    );
    assert!(!vk.key_eq(k));
}

// ============================================================================
// Ported: keys carry a one-line debug description
//
// Flutter asserts `hasOneLineDescription` on each key family. FLUI
// asserts the `Debug` impl produces a non-empty single-line string.
// ============================================================================

#[test]
fn keys_have_one_line_debug_descriptions() {
    fn assert_one_line<K: ViewKey>(key: K, label: &str) {
        let s = format!("{:?}", &key as &dyn ViewKey);
        assert!(!s.is_empty(), "{label} debug must be non-empty");
        assert!(
            !s.contains('\n'),
            "{label} debug must be one line; got {s:?}",
        );
    }

    assert_one_line(ValueKey::new(true), "ValueKey<bool>");
    assert_one_line(UniqueKey::new(), "UniqueKey");
    assert_one_line(ObjectKey::new(Arc::new(true)), "ObjectKey");
    assert_one_line(Key::from_str("hello"), "Key");
}

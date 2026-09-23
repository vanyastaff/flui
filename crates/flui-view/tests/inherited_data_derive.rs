//! Integration tests for `#[derive(InheritedData)]` (issue #1090).
//!
//! The derive lives in a `proc-macro = true` crate, which cannot use its own
//! derives in unit tests (and resolves the runtime path from the consumer's
//! manifest); this compilation unit exercises the generated code against the
//! real runtime types. `prelude::*` brings both the derive and the trait.

use flui_view::FieldMask;
use flui_view::prelude::*;

#[derive(Clone, PartialEq, InheritedData)]
struct Data {
    size: u32,
    scale: f32,
    // A raw identifier: the constant is `FIELD_TYPE`, not `FIELD_R#TYPE`.
    r#type: u8,
}

#[test]
fn constants_are_declaration_order_bits() {
    assert_eq!(Data::FIELD_SIZE, FieldMask::bit(0));
    assert_eq!(Data::FIELD_SCALE, FieldMask::bit(1));
    assert_eq!(Data::FIELD_TYPE, FieldMask::bit(2));
    assert!(!Data::FIELD_SIZE.intersects(Data::FIELD_SCALE));
    assert!(FieldMask::<Data>::ALL.intersects(Data::FIELD_TYPE));
}

#[test]
fn field_mask_diff_names_exactly_the_changed_fields() {
    let a = Data {
        size: 1,
        scale: 1.0,
        r#type: 0,
    };
    let same = a.clone();
    assert!(a.field_mask_diff(&same).is_empty());

    let b = Data {
        size: 2,
        r#type: 1,
        ..a.clone()
    };
    let changed = a.field_mask_diff(&b);
    assert!(changed.intersects(Data::FIELD_SIZE));
    assert!(changed.intersects(Data::FIELD_TYPE));
    assert!(!changed.intersects(Data::FIELD_SCALE));
    assert_eq!(changed, Data::FIELD_SIZE | Data::FIELD_TYPE);
}

//! The contract's own flag sets: the part of the `bitflags` API the workspace
//! uses, under the same names, with no upstream type in their signatures.

use flui_platform::data_transfer::TransferActions;
use flui_platform::input_capabilities::PointerKindSet;

#[test]
fn debug_names_the_flags_or_prints_the_empty_bits() {
    let cases = [
        (TransferActions::empty(), "TransferActions(0x0)"),
        (TransferActions::COPY, "TransferActions(COPY)"),
        (
            TransferActions::MOVE | TransferActions::COPY,
            "TransferActions(COPY | MOVE)",
        ),
        (
            TransferActions::all(),
            "TransferActions(COPY | MOVE | LINK)",
        ),
    ];
    for (set, expected) in cases {
        assert_eq!(format!("{set:?}"), expected);
    }
    assert_eq!(
        format!("{:?}", PointerKindSet::PEN | PointerKindSet::MOUSE),
        "PointerKindSet(MOUSE | PEN)"
    );
}

#[test]
fn from_bits_refuses_a_bit_no_flag_names() {
    assert_eq!(
        TransferActions::from_bits(0b11),
        Some(TransferActions::COPY | TransferActions::MOVE)
    );
    assert_eq!(
        TransferActions::from_bits(0),
        Some(TransferActions::empty())
    );
    assert_eq!(TransferActions::from_bits(1 << 3), None);
    assert_eq!(
        PointerKindSet::from_bits(PointerKindSet::all().bits()),
        Some(PointerKindSet::all())
    );
    assert_eq!(PointerKindSet::from_bits(1 << 5), None);
}

#[test]
fn all_holds_every_flag_and_nothing_else() {
    assert_eq!(TransferActions::all().bits(), 0b111);
    assert_eq!(PointerKindSet::all().bits(), 0b1_1111);
    assert!(TransferActions::all().contains(TransferActions::LINK));
    assert!(TransferActions::COPY.contains(TransferActions::empty()));
    assert!(!TransferActions::COPY.contains(TransferActions::COPY | TransferActions::MOVE));
}

#[test]
fn set_insert_and_remove_change_only_the_named_flags() {
    let mut kinds = PointerKindSet::MOUSE;
    kinds.insert(PointerKindSet::TOUCH);
    kinds.set(PointerKindSet::PEN, true);
    kinds.set(PointerKindSet::MOUSE, false);
    assert_eq!(kinds, PointerKindSet::TOUCH | PointerKindSet::PEN);
    kinds.remove(PointerKindSet::TOUCH | PointerKindSet::TRACKPAD);
    assert_eq!(kinds, PointerKindSet::PEN);
    kinds |= PointerKindSet::VIRTUAL;
    assert_eq!(kinds & PointerKindSet::VIRTUAL, PointerKindSet::VIRTUAL);
    assert!(!kinds.is_empty());
}

#[test]
fn iter_names_follows_declaration_order() {
    let names: Vec<&str> = (TransferActions::LINK | TransferActions::COPY)
        .iter_names()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["COPY", "LINK"]);
    assert_eq!(TransferActions::empty().iter_names().count(), 0);
}

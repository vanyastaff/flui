//! The ownership table (ADR-0208 §1) holds for touch and mouse contacts.

#[test]
fn ownership_table_holds_for_touch_and_mouse() {
    flui_testing::assert_ownership_table();
}

use crate::common::{lay_out, offset, size, tight};
use flui_geometry::{EdgeInsets, px};
use flui_widgets::{MediaQuery, MediaQueryData, SafeArea, SizedBox};

#[test]
fn nested_safe_area_consumes_selected_padding_once() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(20.0, 8.0, 12.0, 6.0),
        ..Default::default()
    };
    let laid = lay_out(
        MediaQuery::new(
            media,
            SafeArea::new().child(SafeArea::new().child(SizedBox::expand())),
        ),
        tight(200.0, 100.0),
    );
    let inner = laid.only_child(laid.root());
    let leaf = laid.only_child(inner);
    assert_eq!(laid.absolute_offset(leaf), offset(6.0, 20.0));
    assert_eq!(laid.size(leaf), size(186.0, 68.0));
}

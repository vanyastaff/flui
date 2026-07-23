//! Widget-level coverage for the accessibility semantics wrappers.

use crate::common::{lay_out, loose, size};
use flui_widgets::{ExcludeSemantics, MergeSemantics, Semantics, SizedBox};

#[test]
fn semantics_widget_mounts_annotations_render_object() {
    let laid = lay_out(
        Semantics::new()
            .container(true)
            .label("Submit")
            .button(true)
            .enabled(true)
            .child(SizedBox::new(40.0, 20.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderSemanticsAnnotations");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(40.0, 20.0));
    assert_eq!(laid.size(laid.only_child(root)), size(40.0, 20.0));
}

#[test]
fn merge_semantics_widget_mounts_merge_render_object() {
    let laid = lay_out(
        MergeSemantics::new().child(SizedBox::new(30.0, 18.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderMergeSemantics");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(30.0, 18.0));
}

#[test]
fn exclude_semantics_widget_mounts_exclude_render_object() {
    let laid = lay_out(
        ExcludeSemantics::new().child(SizedBox::new(24.0, 16.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderExcludeSemantics");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(24.0, 16.0));
}

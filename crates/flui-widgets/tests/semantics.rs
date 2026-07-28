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

#[test]
fn probe_audit_semantics_tree_assembles_in_widget_harness() {
    use flui_widgets::Column;
    let mut laid = lay_out(
        Column::new((
            Semantics::new()
                .container(true)
                .label("Submit")
                .button(true)
                .child(SizedBox::new(40.0, 20.0)),
            Semantics::new()
                .container(true)
                .label("Cancel")
                .child(SizedBox::new(40.0, 20.0)),
        )),
        loose(200.0),
    );
    laid.probe_enable_semantics();
    let snap = laid.probe_semantics_snapshot();
    let labels: Vec<String> = snap
        .nodes()
        .iter()
        .map(|n| {
            format!(
                "{:?} label={:?} rect={:?} flags={} actions={}",
                n.id(),
                n.label().map(|l| l.as_str().to_string()),
                n.rect(),
                n.flags(),
                n.actions()
            )
        })
        .collect();
    panic!("SEMANTICS SNAPSHOT root={:?} n={} \n{}", snap.root(), snap.nodes().len(), labels.join("\n"));
}

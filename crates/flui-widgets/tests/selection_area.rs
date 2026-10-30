//! Static text inside a [`SelectionArea`] publishes runs and is selected by
//! assistive technology as one document (ADR-0214), read back through
//! `accesskit_consumer` as an adapter reads it.

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::{
    Action, ActionData, ActionRequest, NodeId, Role, TextPosition, TextSelection, TreeId,
};
use flui_painting::typography::TextDirection;
use flui_rendering::semantics::ActionDecodeError;
use flui_testing::InvokeActionError;
use flui_widgets::{
    Column, MergeSemantics, Row, SelectionArea, SelectionContainer, Semantics, SizedBox, Text,
};

use crate::common::{LaidOut, lay_out, loose};

/// An adapter's view of the tree the presentation publishes now.
fn consumer(laid: &LaidOut) -> accesskit_consumer::Tree {
    accesskit_consumer::Tree::new(
        laid.a11y_tree().expect("semantics enabled").raw().clone(),
        true,
    )
}

/// Every node of `role` beneath the root, in tree order.
fn find_all(
    state: &accesskit_consumer::TreeState,
    role: Role,
) -> Vec<accesskit_consumer::NodeRef<'_>> {
    let mut found = Vec::new();
    let mut pending = vec![state.root()];
    while let Some(node) = pending.pop() {
        if node.role() == role {
            found.push(node);
        }
        let mut children: Vec<_> = node.children().collect();
        children.reverse();
        pending.extend(children);
    }
    found
}

/// The text runs a node publishes, in order.
fn runs_of(node: &accesskit_consumer::NodeRef<'_>) -> Vec<NodeId> {
    node.children()
        .filter(|child| child.role() == Role::TextRun)
        .map(|child| child.locate().0)
        .collect()
}

/// The static-text node whose value is `text`.
fn paragraph<'a>(
    state: &'a accesskit_consumer::TreeState,
    text: &str,
) -> accesskit_consumer::NodeRef<'a> {
    find_all(state, Role::Label)
        .into_iter()
        .find(|node| node.data().value() == Some(text))
        .unwrap_or_else(|| panic!("no static text {text:?} is published"))
}

fn selection_request(target: NodeId, anchor: TextPosition, focus: TextPosition) -> ActionRequest {
    ActionRequest {
        action: Action::SetTextSelection,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::SetTextSelection(TextSelection {
            anchor,
            focus,
        })),
    }
}

fn at(run: NodeId, character_index: usize) -> TextPosition {
    TextPosition {
        node: run,
        character_index,
    }
}

/// An area over `child` that records each selected text it reports.
fn recorded_area(
    child: impl flui_view::IntoView,
) -> (SelectionArea, Rc<RefCell<Vec<Option<String>>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    let area = SelectionArea::new(child).on_selection_changed(move |_cx, selected| {
        record
            .borrow_mut()
            .push(selected.map(|selected| selected.text.clone()));
    });
    (area, seen)
}

fn mounted(view: impl flui_view::View) -> LaidOut {
    let mut laid = lay_out(view, loose(4000.0));
    laid.enable_semantics();
    laid.tick();
    laid
}

/// Several paragraphs each publish runs under a document node that reads
/// text ranges; an RTL line publishes RTL runs; a line longer than 256
/// characters is cut into runs linked as one line.
pub(crate) fn selection_area_text_publishes_runs() {
    let long = "a".repeat(300);
    let laid = mounted(SelectionArea::new(Column::new((
        Text::new("Alpha beta"),
        Text::new("שלום עולם").direction(TextDirection::Rtl),
        Text::new(long.clone()),
    ))));
    let tree = consumer(&laid);
    let state = tree.state();
    let documents = find_all(state, Role::Document);
    assert_eq!(documents.len(), 1, "the area is one document node");
    assert!(
        documents[0].supports_text_ranges(),
        "a document over runs reads text ranges"
    );
    for text in ["Alpha beta", "שלום עולם"] {
        let node = paragraph(state, text);
        assert!(node.supports_text_ranges(), "{text:?} reads by itself");
        assert!(!runs_of(&node).is_empty(), "{text:?} publishes its runs");
    }
    let rtl = paragraph(state, "שלום עולם");
    assert!(
        rtl.children()
            .filter(|run| run.role() == Role::TextRun)
            .all(|run| run.data().text_direction() == Some(accesskit::TextDirection::RightToLeft)),
        "an RTL line publishes RTL runs"
    );
    let long_runs: Vec<_> = paragraph(state, &long)
        .children()
        .filter(|run| run.role() == Role::TextRun)
        .collect();
    assert_eq!(
        long_runs.len(),
        2,
        "300 characters on one line are two runs"
    );
    assert_eq!(
        long_runs[0].data().next_on_line(),
        Some(long_runs[1].locate().0),
        "and read as one line"
    );
    assert_eq!(
        documents[0].document_range().text(),
        format!("Alpha betaשלום עולם{long}"),
        "the document reads every text beneath it"
    );
}

/// A screen reader's selection inside one paragraph and across two reaches
/// the area's model, is reported, and is published back on the document.
pub(crate) fn selection_area_accepts_assistive_selection() {
    let (area, seen) = recorded_area(Column::new((
        Text::new("Alpha beta"),
        Text::new("Gamma delta"),
    )));
    let mut laid = mounted(area);
    let (first, second, document) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha beta")),
            runs_of(&paragraph(state, "Gamma delta")),
            find_all(state, Role::Document)[0].locate().0,
        )
    };

    // Sent to a run, as an adapter does for a caret inside one paragraph.
    laid.invoke_semantics_action(selection_request(
        first[0],
        at(first[0], 0),
        at(first[0], 5),
    ))
    .expect("a selection in one paragraph is accepted");
    laid.tick();
    // Sent to the document; the focus index past its run's end reads as the
    // run's end.
    laid.invoke_semantics_action(selection_request(
        document,
        at(first[0], 6),
        at(second[0], 99),
    ))
    .expect("a selection across paragraphs is accepted");
    laid.tick();

    assert_eq!(
        *seen.borrow(),
        vec![
            Some("Alpha".to_owned()),
            Some("beta\nGamma delta".to_owned())
        ],
        "each selection is reported in reading order"
    );
    let tree = consumer(&laid);
    let state = tree.state();
    let document = find_all(state, Role::Document)[0];
    let range = document
        .text_selection()
        .expect("the document publishes the selection");
    assert_eq!(
        range.text(),
        "betaGamma delta",
        "an adapter reads the selection"
    );
}

/// An end in another area's runs, or in a run an edit retired, refuses the
/// whole request before the area sees it.
pub(crate) fn selection_area_refuses_stale_or_foreign_positions() {
    let view = |text: &str| {
        Row::new((
            recorded_area(Text::new(text.to_owned())).0,
            SelectionArea::new(Text::new("Other")),
        ))
    };
    let mut laid = mounted(view("Alpha"));
    let (mine, foreign) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Other")),
        )
    };
    let refused = |result: Result<(), InvokeActionError>| {
        matches!(
            result,
            Err(InvokeActionError::Decode(
                ActionDecodeError::InvalidTextPosition { .. }
            ))
        )
    };
    assert!(
        refused(laid.invoke_semantics_action(selection_request(
            mine[0],
            at(mine[0], 0),
            at(foreign[0], 1)
        ))),
        "an end in another area is refused"
    );

    laid.pump_widget(view("Alpine"));
    let current = runs_of(&paragraph(consumer(&laid).state(), "Alpine"));
    assert_ne!(current, mine, "the edited text has new runs");
    assert!(
        refused(laid.invoke_semantics_action(selection_request(
            current[0],
            at(current[0], 0),
            at(mine[0], 1)
        ))),
        "a current end with a stale one is refused"
    );

    // An area nested in another holds its own texts: the outer document
    // refuses ends in the inner one's runs.
    let nested = mounted(SelectionArea::new(Column::new((
        Text::new("Outer"),
        SelectionArea::new(Text::new("Inner")),
    ))));
    let (outer, inner) = {
        let tree = consumer(&nested);
        let state = tree.state();
        (
            find_all(state, Role::Document)[0].locate().0,
            runs_of(&paragraph(state, "Inner")),
        )
    };
    assert!(
        refused(nested.invoke_semantics_action(selection_request(
            outer,
            at(inner[0], 0),
            at(inner[0], 2)
        ))),
        "an end in a nested area's runs is refused by the outer one"
    );
}

/// Under a merging ancestor the texts keep their runs and still take a
/// selection.
pub(crate) fn selectable_text_under_a_merging_ancestor_keeps_its_runs() {
    let (area, seen) = recorded_area(Column::new((Text::new("Alpha"), Text::new("Beta"))));
    let laid = mounted(MergeSemantics::new().child(area));
    let runs = runs_of(&paragraph(consumer(&laid).state(), "Alpha"));
    assert!(!runs.is_empty(), "the runs survive the outer merge");
    laid.invoke_semantics_action(selection_request(runs[0], at(runs[0], 0), at(runs[0], 3)))
        .expect("and a selection in them is accepted");
    assert_eq!(*seen.borrow(), vec![Some("Alp".to_owned())]);
}

/// Text outside an area, or opted out of one, is a label with no runs.
pub(crate) fn static_text_outside_selection_area_publishes_no_runs() {
    let laid = mounted(Column::new((
        Text::new("Plain"),
        SelectionArea::new(SelectionContainer::disabled(Text::new("Opted out"))),
    )));
    let tree = consumer(&laid);
    let state = tree.state();
    assert!(
        find_all(state, Role::TextRun).is_empty(),
        "no text run is published"
    );
    assert!(!paragraph(state, "Plain").supports_text_ranges());
    assert!(
        laid.a11y_tree()
            .expect("semantics enabled")
            .find_by_label("Opted out")
            .is_ok(),
        "opted-out text is still read, as plain text"
    );
}

/// A text inside a merge inside the area is a nested merger: it keeps its
/// own node and its run nodes rather than joining the merged label.
pub(crate) fn nested_merger_paragraph_keeps_its_run_nodes() {
    let laid = mounted(SelectionArea::new(
        MergeSemantics::new().child(Row::new((Text::new("Alpha"), Text::new("Beta")))),
    ));
    let tree = consumer(&laid);
    let state = tree.state();
    for text in ["Alpha", "Beta"] {
        assert!(
            !runs_of(&paragraph(state, text)).is_empty(),
            "{text:?} keeps its run nodes inside the merge"
        );
    }
}

/// A control inside an area that an outer merger holds keeps its own node:
/// the area stops the merge without merging its descendants.
pub(crate) fn a_control_inside_a_selection_area_under_an_outer_merger_keeps_its_node() {
    let laid = mounted(
        MergeSemantics::new().child(Column::new((
            Text::new("Tile"),
            SelectionArea::new(Column::new((
                Text::new("Alpha"),
                Semantics::new()
                    .container(true)
                    .label("Go")
                    .on_click(|_cx| {})
                    .child(SizedBox::new(20.0, 20.0)),
            ))),
        ))),
    );
    let tree = laid.a11y_tree().expect("semantics enabled");
    let go = tree
        .find_by_label("Go")
        .expect("the control keeps its own node");
    assert!(go.supports_action(Action::Click), "and its action");
    assert!(
        tree.find_by_label("Tile").is_ok(),
        "the outer merge still holds the text outside the area"
    );
}

/// A document selection naming a byte past its text, a byte inside a
/// character, or a text that is not beneath the document is published
/// snapped into the text, or withheld, and is never handed to an adapter as
/// a position its runs lack.
pub(crate) fn a_document_selection_snaps_into_its_texts_or_is_withheld() {
    use flui_foundation::geometry::{Point, Rect, Size};
    use flui_painting::Canvas;
    use flui_rendering::semantics::{
        DocumentPosition, DocumentSelection, SemanticsAnchor, SemanticsConfiguration,
        SemanticsRegion, SemanticsRegionKey, TextContent, TextRun, TextRunParts,
    };
    use flui_widgets::{CustomPaint, CustomPainter, SemanticsRole};

    /// Static text "héllo" declaring `anchor`, one run, each character ten
    /// wide; 'é' is two bytes.
    #[derive(Debug)]
    struct Static(SemanticsAnchor);
    impl CustomPainter for Static {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}
        fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
            false
        }
        fn semantics_regions(&self, size: Size) -> Vec<SemanticsRegion> {
            let mut parts = TextRunParts::new(
                "héllo",
                0,
                0,
                flui_widgets::SemanticsTextDirection::Ltr,
                Rect::from_origin_size(Point::ZERO, Size::new(50.0, 10.0)),
            );
            for (index, length) in [1_u8, 2, 1, 1, 1].into_iter().enumerate() {
                parts.character_lengths.push(length);
                parts.character_positions.push(10.0 * index as f32);
                parts.character_widths.push(10.0);
            }
            let content = TextContent::try_new(vec![
                TextRun::try_new(parts).expect("a fixture run is well formed"),
            ])
            .expect("one run");
            let mut config = SemanticsConfiguration::new();
            config.set_label("héllo");
            config.add_anchor(self.0.clone());
            config.set_text_content(content);
            vec![SemanticsRegion::new(
                SemanticsRegionKey(1),
                Rect::from_origin_size(Point::ZERO, size),
                config,
            )]
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let text = SemanticsAnchor::new();
    let document = |anchor: DocumentPosition, focus: DocumentPosition| {
        Semantics::new()
            .role(SemanticsRole::Document)
            .merge_boundary(true)
            .document_selection(Some(DocumentSelection::new(anchor, focus)))
            .child(
                CustomPaint::new()
                    .size(Size::new(50.0, 10.0))
                    .painter(Static(text.clone())),
            )
    };
    let position = |byte| DocumentPosition::new(text.clone(), byte);
    let stale = DocumentPosition::new(SemanticsAnchor::new(), 0);
    for (case, anchor, focus, published) in [
        (
            "a byte inside 'é' snaps to its start",
            position(2),
            position(3),
            Some((1, 2)),
        ),
        (
            "a byte past the end snaps to the end",
            position(0),
            position(99),
            Some((0, 5)),
        ),
        (
            "an anchor naming no text withholds the selection",
            stale,
            position(1),
            None,
        ),
    ] {
        let laid = mounted(document(anchor, focus));
        let tree = consumer(&laid);
        let state = tree.state();
        let node = find_all(state, Role::Document)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{case}: the document is published"));
        let selection = node.data().text_selection().map(|selection| {
            (
                selection.anchor.character_index,
                selection.focus.character_index,
            )
        });
        assert_eq!(selection, published, "{case}");
        // What an adapter does with it: never panics on a published end.
        if let Some(range) = node.text_selection() {
            let _ = range.text();
        }
    }
}

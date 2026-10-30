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

/// Links in selectable text keep a node and a handler each, so wrapping
/// linked text in an area changes neither link navigation nor activation.
///
/// Red-check: make a linked selectable paragraph merge its descendants
/// (the two links fold into one node with the last handler).
pub(crate) fn links_in_a_selection_area_keep_their_own_nodes() {
    use flui_painting::typography::TextSpan;
    use flui_widgets::RichText;

    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&opened);
    let mut laid = mounted(SelectionArea::new(
        RichText::new(TextSpan::with_children(vec![
            TextSpan::new("Read the "),
            TextSpan::new("terms").with_link("app://terms"),
            TextSpan::new(" and the "),
            TextSpan::new("privacy notice").with_link("app://privacy"),
        ]))
        .on_link(move |_cx, link| record.borrow_mut().push(link.to_string())),
    ));
    let (terms, privacy) = {
        let tree = consumer(&laid);
        let state = tree.state();
        let links = find_all(state, Role::Link);
        let labels: Vec<_> = links
            .iter()
            .map(accesskit_consumer::NodeRef::label)
            .collect();
        assert_eq!(
            labels,
            [Some("terms".to_owned()), Some("privacy notice".to_owned())],
            "each link keeps its own node"
        );
        (links[0].locate().0, links[1].locate().0)
    };
    for link in [privacy, terms] {
        laid.invoke_semantics_action(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: link,
            data: None,
        })
        .expect("a link node is activated");
        laid.tick();
    }
    assert_eq!(
        *opened.borrow(),
        ["app://privacy", "app://terms"],
        "each link opens its own target"
    );
}

/// An end between the pieces of a grapheme cut for the 255-byte run limit
/// snaps to the grapheme's start, so no user-perceived character is split.
///
/// Red-check: snap ends to a `char` boundary only (the report holds half of
/// the long grapheme).
pub(crate) fn an_assistive_end_inside_a_long_grapheme_snaps_to_its_start() {
    let grapheme = format!("e{}", "\u{301}".repeat(130));
    let text = format!("ab{grapheme}c");
    let (area, seen) = recorded_area(Text::new(text.clone()));
    let mut laid = mounted(area);
    let runs = {
        let tree = consumer(&laid);
        let state = tree.state();
        let node = paragraph(state, &text);
        let runs = runs_of(&node);
        let characters: usize = node
            .children()
            .filter(|run| run.role() == Role::TextRun)
            .map(|run| run.data().character_lengths().len())
            .sum();
        assert_eq!(
            characters, 5,
            "the long grapheme is published as two pieces"
        );
        runs
    };
    assert_eq!(runs.len(), 1, "one line is one run");
    laid.invoke_semantics_action(selection_request(runs[0], at(runs[0], 0), at(runs[0], 3)))
        .expect("a selection inside one paragraph is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("ab".to_owned())],
        "the end inside the grapheme moves to its start"
    );
}

/// An offstage paragraph between two selected ones is not in the document,
/// so it is not in the selected text either.
///
/// Red-check: keep offstage entries in the reading order (the report holds
/// "Hidden" between the two visible texts).
pub(crate) fn offstage_text_is_left_out_of_the_selection() {
    use flui_widgets::Offstage;

    let (area, seen) = recorded_area(Column::new((
        Text::new("Alpha"),
        Offstage::new().child(Text::new("Hidden")),
        Text::new("Omega"),
    )));
    let mut laid = mounted(area);
    let (first, last) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Omega")),
        )
    };
    laid.invoke_semantics_action(selection_request(first[0], at(first[0], 0), at(last[0], 5)))
        .expect("a selection across the visible texts is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("Alpha\nOmega".to_owned())],
        "the offstage text is not selected"
    );
}

/// A wake that panics after the selection is committed still lets the
/// listener hear of it, and the failure is not swallowed.
///
/// Red-check: call the listener after the wake without containment (the
/// listener never hears of the committed selection).
pub(crate) fn a_panicking_wake_still_reports_the_committed_selection() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    // An element keeps the frame-request hook of its mount, so the hook is
    // installed before the area mounts and armed once it has.
    let armed = Arc::new(AtomicBool::new(false));
    let mut laid = mounted(SizedBox::new(1.0, 1.0));
    let arm = Arc::clone(&armed);
    laid.with_build_owner_mut(|owner| {
        owner.set_on_build_scheduled(move || {
            assert!(!arm.load(Ordering::SeqCst), "the frame request failed");
        });
    });
    let (area, seen) = recorded_area(Text::new("Alpha beta"));
    laid.pump_widget(area);
    laid.tick();
    let runs = {
        let tree = consumer(&laid);
        runs_of(&paragraph(tree.state(), "Alpha beta"))
    };
    armed.store(true, Ordering::SeqCst);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.invoke_semantics_action(selection_request(runs[0], at(runs[0], 0), at(runs[0], 5)))
    }));
    assert!(outcome.is_err(), "the wake's failure reaches the caller");
    assert_eq!(
        *seen.borrow(),
        vec![Some("Alpha".to_owned())],
        "the listener heard of the committed selection"
    );
}

/// The reported text follows the order the published document reads, so a
/// column laid out bottom-up reports what an adapter selected.
///
/// Red-check: order the texts by their top edge (the report is "\n").
pub(crate) fn the_reported_text_follows_the_published_document_order() {
    use flui_objects::VerticalDirection;

    let (area, seen) = recorded_area(
        Column::new((Text::new("Alpha"), Text::new("Beta")))
            .vertical_direction(VerticalDirection::Up),
    );
    let mut laid = mounted(area);
    let (first, last, text) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Beta")),
            find_all(state, Role::Document)[0].document_range().text(),
        )
    };
    assert_eq!(text, "AlphaBeta", "the document reads in tree order");
    laid.invoke_semantics_action(selection_request(first[0], at(first[0], 0), at(last[0], 4)))
        .expect("a selection across both texts is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("Alpha\nBeta".to_owned())],
        "the report covers what the adapter selected"
    );
}

/// A paragraph cut by a line limit contributes only its shown lines to a
/// selection that runs past it, as its runs do.
///
/// Red-check: extract each passed-over text to its authored end (the report
/// holds "hidden").
pub(crate) fn a_line_limit_bounds_what_a_paragraph_reports() {
    use flui_painting::typography::TextSpan;
    use flui_widgets::RichText;

    let (area, seen) = recorded_area(Column::new((
        RichText::new(TextSpan::new("shown\nhidden")).max_lines(1),
        Text::new("Omega"),
    )));
    let mut laid = mounted(area);
    let (first, last) = {
        let tree = consumer(&laid);
        let state = tree.state();
        let document = find_all(state, Role::Document)[0];
        assert!(
            !document.document_range().text().contains("hidden"),
            "the cut line publishes no runs"
        );
        let first = find_all(state, Role::Label)
            .into_iter()
            .find(|node| {
                node.data()
                    .value()
                    .is_some_and(|value| value.starts_with("shown"))
            })
            .expect("the cut paragraph is published");
        (runs_of(&first), runs_of(&paragraph(state, "Omega")))
    };
    laid.invoke_semantics_action(selection_request(first[0], at(first[0], 0), at(last[0], 5)))
        .expect("a selection across both texts is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("shown\nOmega".to_owned())],
        "the cut line is not reported"
    );
}

/// Text under a subtree that blocks user actions takes no assistive
/// selection, through its own runs or as an end of a selection sent to the
/// document (ADR-0216 §1).
///
/// Red-check: route a blocked text's selection to the document (the area
/// accepts it and reports "Blocked").
pub(crate) fn blocked_text_refuses_assistive_selection() {
    let (area, seen) = recorded_area(Column::new((
        Text::new("Alpha"),
        Semantics::new()
            .block_user_actions(true)
            .child(Text::new("Blocked")),
    )));
    let mut laid = mounted(area);
    let (open, blocked, document) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Blocked")),
            find_all(state, Role::Document)[0].locate().0,
        )
    };
    laid.invoke_semantics_action(selection_request(
        blocked[0],
        at(blocked[0], 0),
        at(blocked[0], 3),
    ))
    .expect_err("a blocked text refuses a selection in its runs");
    laid.invoke_semantics_action(selection_request(
        document,
        at(open[0], 0),
        at(blocked[0], 3),
    ))
    .expect_err("a document refuses an end in a blocked text");
    laid.tick();
    assert!(seen.borrow().is_empty(), "the area saw no selection");
}

/// Sort keys that reverse two texts reverse them in the report too: the
/// report follows the publication, not the render tree.
///
/// Red-check: order the texts by place in the render tree (the report is
/// "\n").
pub(crate) fn sort_keys_order_the_reported_text() {
    use flui_rendering::semantics::{SemanticsConfiguration, SemanticsSortKey};

    let keyed = |text: &str, order: f64| {
        let mut config = SemanticsConfiguration::new();
        config.set_sort_key(SemanticsSortKey::new(order));
        Semantics::from_configuration(config)
            .container(true)
            .child(Text::new(text.to_owned()))
    };
    let (area, seen) = recorded_area(Column::new((keyed("Alpha", 2.0), keyed("Beta", 1.0))));
    let mut laid = mounted(area);
    let (alpha, beta, text) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Beta")),
            find_all(state, Role::Document)[0].document_range().text(),
        )
    };
    assert_eq!(text, "BetaAlpha", "the document reads by sort key");
    laid.invoke_semantics_action(selection_request(beta[0], at(beta[0], 0), at(alpha[0], 5)))
        .expect("a selection across both texts is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("Beta\nAlpha".to_owned())],
        "the report covers what the adapter selected"
    );
}

/// A text whose semantics label is empty publishes nothing, so a selection
/// across it does not report it.
///
/// Red-check: keep every mounted text in the order (the report holds
/// "silent").
pub(crate) fn a_silenced_text_is_left_out_of_the_selection() {
    use flui_painting::typography::TextSpan;
    use flui_widgets::RichText;

    let (area, seen) = recorded_area(Column::new((
        Text::new("Alpha"),
        RichText::new(TextSpan::new("silent").with_semantics_label("")),
        Text::new("Omega"),
    )));
    let mut laid = mounted(area);
    let (first, last) = {
        let tree = consumer(&laid);
        let state = tree.state();
        (
            runs_of(&paragraph(state, "Alpha")),
            runs_of(&paragraph(state, "Omega")),
        )
    };
    laid.invoke_semantics_action(selection_request(first[0], at(first[0], 0), at(last[0], 5)))
        .expect("a selection across the published texts is accepted");
    laid.tick();
    assert_eq!(
        *seen.borrow(),
        vec![Some("Alpha\nOmega".to_owned())],
        "the silenced text is not selected"
    );
}

/// A span's accessibility override still decides what is read: overridden
/// or silenced text keeps its label and lends no runs to the document.
///
/// Red-check: publish runs from the authored text regardless (the document
/// reads "Dr." and "secret").
pub(crate) fn span_overrides_keep_their_reading_in_a_selection_area() {
    use flui_painting::typography::TextSpan;
    use flui_widgets::RichText;

    let laid = mounted(SelectionArea::new(Column::new((
        RichText::new(TextSpan::new("Dr.").with_semantics_label("Doctor")),
        RichText::new(TextSpan::with_children(vec![
            TextSpan::new("Hi "),
            TextSpan::new("secret").with_semantics_label(""),
        ])),
        Text::new("Omega"),
    ))));
    let tree = consumer(&laid);
    let state = tree.state();
    let read = find_all(state, Role::Document)[0].document_range().text();
    assert_eq!(read, "Omega", "only text read as written lends runs");
    assert!(
        find_all(state, Role::Label)
            .iter()
            .any(|node| node.label().as_deref() == Some("Doctor")),
        "the override is still the label"
    );
}

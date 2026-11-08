//! A text field's runs and selection, read the way assistive technology reads
//! them (ADR-0204, semantics-model R12 and R14).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use accesskit::{Action, ActionData, ActionRequest, NodeId, TextPosition, TextSelection, TreeId};
use flui_foundation::geometry::Rect;
use flui_semantics::{
    ActionCall, ActionDecodeError, DocumentPosition, DocumentSelection, MAX_RUN_CHARACTERS,
    SemanticsAnchor, SemanticsDiagnostic, SemanticsIdentitySource, SemanticsRegionId,
    SemanticsRole, TextContent, TextDirection, TextRun, TextRunError, TextRunParts,
    TextRunPosition, TextRunSelection, TreeUpdate,
};
use parking_lot::Mutex;

use crate::fixture::{Fixture, render_id};

/// A run of `text` from byte `start` on `line`, each character one `char`
/// ten pixels wide, words starting after spaces.
fn run(text: &str, start: usize, line: usize, direction: TextDirection) -> TextRun {
    let mut parts = TextRunParts::new(
        text,
        start,
        line,
        direction,
        Rect::from_ltwh(
            0.0,
            20.0 * line as f64,
            10.0 * text.chars().count() as f64,
            20.0,
        ),
    );
    let mut previous_space = true;
    for (index, character) in text.chars().enumerate() {
        parts
            .character_lengths
            .push(u8::try_from(character.len_utf8()).expect("a char is at most 4 bytes"));
        parts.character_positions.push(10.0 * index as f32);
        parts.character_widths.push(10.0);
        if previous_space && !character.is_whitespace() {
            parts
                .word_starts
                .push(u8::try_from(index).expect("fixture runs are short"));
        }
        previous_space = character.is_whitespace();
    }
    TextRun::try_new(parts).expect("a fixture run is well formed")
}

fn identities(source: &mut SemanticsIdentitySource, count: usize) -> Vec<SemanticsRegionId> {
    source
        .allocate(count)
        .expect("a fresh source has identities")
}

/// A window (node 1) holding text fields 2 and 3, each configured by its
/// closure, published once; the updates the owner delivered.
fn fields(
    first: impl FnOnce(&mut flui_semantics::SemanticsConfiguration),
    second: impl FnOnce(&mut flui_semantics::SemanticsConfiguration),
) -> (Fixture, Arc<Mutex<Vec<TreeUpdate>>>) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::default();
    let sink = Arc::clone(&received);
    let mut fixture = Fixture::new();
    fixture.owner = flui_semantics::SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    fixture
        .add(None, 1, (0.0, 0.0, 400.0, 200.0), |c| c.set_label("Window"))
        .add(Some(1), 2, (10.0, 10.0, 300.0, 40.0), |c| {
            c.set_role(SemanticsRole::TextInput);
            first(c);
        })
        .add(Some(1), 3, (10.0, 60.0, 300.0, 40.0), |c| {
            c.set_role(SemanticsRole::TextInput);
            second(c);
        })
        .flush();
    (fixture, received)
}

fn field_id(index: u32) -> NodeId {
    NodeId(flui_semantics::AccessibilityNodeId::from(render_id(index, 5)).as_u64())
}

/// A field's value and its runs, with `selection`, published under fresh
/// identities.
fn content(
    source: &mut SemanticsIdentitySource,
    runs: Vec<TextRun>,
    selection: Option<TextRunSelection>,
) -> (TextContent, Vec<SemanticsRegionId>) {
    let ids = identities(source, runs.len());
    let mut content = TextContent::try_new(runs).expect("fixture runs are contiguous");
    if let Some(selection) = selection {
        content = content
            .with_selection(selection)
            .expect("the fixture selection lies in the runs");
    }
    (
        content
            .with_run_identities(ids.clone())
            .expect("one identity per run"),
        ids,
    )
}

/// A two-line field reads by character, word and line through
/// `accesskit_consumer`, and its caret is where the editor put it.
#[test]
fn a_field_reads_by_character_word_and_line() {
    let mut source = SemanticsIdentitySource::default();
    let (text, _) = content(
        &mut source,
        vec![
            run("Hello world\n", 0, 0, TextDirection::Ltr),
            run("Bye", 12, 1, TextDirection::Ltr),
        ],
        Some(TextRunSelection::collapsed(TextRunPosition::new(0, 5))),
    );
    let (_fixture, received) = fields(
        |c| {
            c.set_value("Hello world\nBye");
            c.set_text_content(text);
        },
        |_| {},
    );
    let tree = accesskit_consumer::Tree::new(received.lock()[0].clone(), true);
    let field = tree
        .state()
        .node_by_tree_local_id(field_id(2), TreeId::ROOT)
        .expect("the field is published");
    assert!(field.supports_text_ranges());
    assert_eq!(field.document_range().text(), "Hello world\nBye");
    let caret = field.text_selection_focus().expect("the field has a caret");
    assert_eq!(caret.to_global_usv_index(), 5);
    assert_eq!(
        field
            .document_start()
            .forward_to_word_start()
            .to_global_usv_index(),
        6,
        "the second word starts after the space"
    );
    assert_eq!(
        field
            .line_range_from_index(1)
            .expect("a second line")
            .text(),
        "Bye"
    );
}

/// Runs of one line link to each other, so a change of direction inside a
/// line is not read as a line break.
#[test]
fn a_direction_change_inside_a_line_reads_as_one_line() {
    let mut source = SemanticsIdentitySource::default();
    let (text, _) = content(
        &mut source,
        vec![
            run("abc ", 0, 0, TextDirection::Ltr),
            run("אבג", 4, 0, TextDirection::Rtl),
        ],
        None,
    );
    let (_fixture, received) = fields(
        |c| {
            c.set_value("abc אבג");
            c.set_text_content(text);
        },
        |_| {},
    );
    let tree = accesskit_consumer::Tree::new(received.lock()[0].clone(), true);
    let field = tree
        .state()
        .node_by_tree_local_id(field_id(2), TreeId::ROOT)
        .expect("the field is published");
    assert_eq!(
        field.line_range_from_index(0).expect("one line").text(),
        "abc אבג"
    );
    assert!(field.line_range_from_index(1).is_none(), "and only one");
}

fn select(field: u32, anchor: (NodeId, usize), focus: (NodeId, usize)) -> ActionRequest {
    ActionRequest {
        action: Action::SetTextSelection,
        target_tree: TreeId::ROOT,
        target_node: field_id(field),
        data: Some(ActionData::SetTextSelection(TextSelection {
            anchor: TextPosition {
                node: anchor.0,
                character_index: anchor.1,
            },
            focus: TextPosition {
                node: focus.0,
                character_index: focus.1,
            },
        })),
    }
}

struct NoChanges;
impl accesskit_consumer::TreeChangeHandler for NoChanges {
    fn node_added(&mut self, _: &accesskit_consumer::NodeRef<'_>) {}
    fn node_updated(
        &mut self,
        _: &accesskit_consumer::NodeRef<'_>,
        _: &accesskit_consumer::NodeRef<'_>,
    ) {
    }
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef<'_>>,
        _: Option<&accesskit_consumer::NodeRef<'_>>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef<'_>) {}
}

fn replace_text(fixture: &mut Fixture, value: &str, text: TextContent) {
    let id = fixture.id_of(2);
    let mut node = fixture
        .owner
        .tree()
        .get(id)
        .expect("the field is in the tree")
        .clone();
    node.config_mut().set_value(value);
    node.config_mut().set_text_content(text);
    assert!(
        fixture.owner.tree_mut().replace_node(id, node),
        "the field is live"
    );
}

/// Editing every run and moving the caret keeps their identities and reaches a
/// live consumer incrementally. A selection-only change carries just its field.
#[test]
fn text_runs_follow_edits_incrementally() {
    let mut source = SemanticsIdentitySource::default();
    let (initial, ids) = content(
        &mut source,
        vec![
            run("ab\n", 0, 0, TextDirection::Ltr),
            run("cd", 3, 1, TextDirection::Ltr),
        ],
        Some(TextRunSelection::collapsed(TextRunPosition::new(0, 1))),
    );
    let (mut fixture, received) = fields(
        |c| {
            c.set_value("ab\ncd");
            c.set_text_content(initial);
        },
        |_| {},
    );
    let mut consumer = accesskit_consumer::Tree::new(received.lock()[0].clone(), true);

    let edited = TextContent::try_new(vec![
        run("a\n", 0, 0, TextDirection::Ltr),
        run("xyz", 2, 1, TextDirection::Ltr),
    ])
    .expect("the edited runs are contiguous")
    .with_selection(TextRunSelection::collapsed(TextRunPosition::new(1, 2)))
    .expect("the edited caret is in the second run")
    .with_run_identities(ids.clone())
    .expect("the run identities survive the edit");
    replace_text(&mut fixture, "a\nxyz", edited);
    fixture.owner.flush();
    let update = received
        .lock()
        .last()
        .expect("an edit is delivered")
        .clone();
    assert!(update.tree.is_none(), "the edit is incremental");
    consumer.update_and_process_changes(update, &mut NoChanges);
    let field = consumer
        .state()
        .node_by_tree_local_id(field_id(2), TreeId::ROOT)
        .expect("the field remains published");
    assert_eq!(field.document_range().text(), "a\nxyz");
    assert_eq!(
        field
            .text_selection_focus()
            .expect("the edited caret is published")
            .to_global_usv_index(),
        4
    );
    let second = consumer
        .state()
        .node_by_tree_local_id(NodeId(ids[1].accessibility_id().as_u64()), TreeId::ROOT)
        .expect("the second run keeps its identity");
    assert_eq!(second.data().value(), Some("xyz"));
    assert_eq!(second.data().character_widths(), Some(&[10.0; 3][..]));
    let bounds = second.data().bounds().expect("the run has bounds");
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (0.0, 20.0, 30.0, 40.0)
    );

    let selection_only = TextContent::try_new(vec![
        run("a\n", 0, 0, TextDirection::Ltr),
        run("xyz", 2, 1, TextDirection::Ltr),
    ])
    .expect("the runs are unchanged")
    .with_selection(TextRunSelection::collapsed(TextRunPosition::new(0, 0)))
    .expect("the new caret is in the first run")
    .with_run_identities(ids.clone())
    .expect("the run identities remain assigned");
    replace_text(&mut fixture, "a\nxyz", selection_only);
    fixture.owner.flush();
    let update = received
        .lock()
        .last()
        .expect("the selection is delivered")
        .clone();
    assert!(update.tree.is_none(), "the selection edit is incremental");
    assert!(
        update.nodes.iter().any(|(id, _)| *id == field_id(2)),
        "the field carries its new selection"
    );
    for identity in ids {
        assert!(
            update
                .nodes
                .iter()
                .all(|(id, _)| { *id != NodeId(identity.accessibility_id().as_u64()) }),
            "an unchanged run is not republished"
        );
    }
    consumer.update_and_process_changes(update, &mut NoChanges);
    let field = consumer
        .state()
        .node_by_tree_local_id(field_id(2), TreeId::ROOT)
        .expect("the field remains published");
    assert_eq!(
        field
            .text_selection_focus()
            .expect("the new caret is published")
            .to_global_usv_index(),
        0
    );
}

/// A stable run id does not authorize a position against text the adapter has
/// not received. This holds for field and document selections alike.
#[test]
fn text_positions_wait_for_their_text_to_be_delivered() {
    for document in [false, true] {
        let mut source = SemanticsIdentitySource::default();
        let (initial, ids) = content(&mut source, vec![run("a", 0, 0, TextDirection::Ltr)], None);
        let text_anchor = SemanticsAnchor::new();
        let declared_anchor = text_anchor.clone();
        let failing = Arc::new(AtomicBool::new(false));
        let reject = Arc::clone(&failing);
        let mut fixture = Fixture::new();
        fixture.owner = flui_semantics::SemanticsOwner::new(Arc::new(move |_: &TreeUpdate| {
            assert!(!reject.load(Ordering::Relaxed), "text delivery rejected");
        }));
        fixture.add(None, 1, (0.0, 0.0, 400.0, 200.0), |c| {
            c.set_label("Window");
        });
        if document {
            fixture.add(Some(1), 4, (0.0, 0.0, 300.0, 100.0), |c| {
                c.set_role(SemanticsRole::Document);
                c.set_merge_boundary(true);
                c.add_action(
                    flui_semantics::SemanticsAction::SetTextSelection,
                    std::rc::Rc::new(|_| {}),
                );
            });
        }
        fixture
            .add(
                Some(if document { 4 } else { 1 }),
                2,
                (10.0, 10.0, 300.0, 40.0),
                |c| {
                    c.set_role(if document {
                        SemanticsRole::None
                    } else {
                        SemanticsRole::TextInput
                    });
                    c.set_value("a");
                    c.set_text_content(initial);
                    if document {
                        c.add_anchor(declared_anchor);
                    } else {
                        c.add_action(
                            flui_semantics::SemanticsAction::SetTextSelection,
                            std::rc::Rc::new(|_| {}),
                        );
                    }
                },
            )
            .flush();

        let edited = TextContent::try_new(vec![run("é", 0, 0, TextDirection::Ltr)])
            .expect("one edited run")
            .with_run_identities(ids.clone())
            .expect("the run keeps its identity");
        replace_text(&mut fixture, "é", edited);
        failing.store(true, Ordering::Relaxed);
        let failure = catch_unwind(AssertUnwindSafe(|| fixture.owner.flush()))
            .expect_err("the adapter rejects the edited text");
        failing.store(false, Ordering::Relaxed);
        flui_foundation::panic::discard_payload(failure);

        let run_id = NodeId(ids[0].accessibility_id().as_u64());
        let request = select(2, (run_id, 1), (run_id, 1));
        let target =
            flui_semantics::AccessibilityNodeId::from(render_id(if document { 4 } else { 2 }, 5));
        assert_eq!(
            fixture.owner.semantics_action_request_for(&request),
            Err(ActionDecodeError::InvalidTextPosition { node_id: target }),
            "document={document}: undelivered text cannot decode an old position"
        );

        fixture.owner.flush();
        let decoded = fixture
            .owner
            .semantics_action_request_for(&request)
            .expect("delivery makes the same run position current");
        assert_eq!(decoded.node_id, target);
        if document {
            assert_eq!(
                decoded.call,
                ActionCall::SetDocumentSelection(DocumentSelection::new(
                    DocumentPosition::new(text_anchor.clone(), 2),
                    DocumentPosition::new(text_anchor, 2),
                ))
            );
        } else {
            assert_eq!(
                decoded.call,
                ActionCall::SetTextSelection(flui_semantics::TextSelection {
                    anchor: 2,
                    focus: 2,
                })
            );
        }
    }
}

/// A selection reads into bytes of the field's value when both ends lie in
/// its current runs, and is refused when an end lies in another field's run,
/// in a run the field no longer publishes, or past a run's end. A request on
/// a run goes to its field.
#[test]
fn a_selection_is_read_only_from_the_fields_own_runs() {
    let mut source = SemanticsIdentitySource::default();
    let (first, first_ids) = content(
        &mut source,
        vec![
            run("añb\n", 0, 0, TextDirection::Ltr),
            run("cd", 5, 1, TextDirection::Ltr),
        ],
        None,
    );
    let (second, second_ids) = content(
        &mut source,
        vec![run("xyz", 0, 0, TextDirection::Ltr)],
        None,
    );
    let stale = identities(&mut source, 1)[0];
    let (fixture, _) = fields(
        |c| {
            c.set_value("añb\ncd");
            c.set_text_content(first);
            c.add_action(
                flui_semantics::SemanticsAction::SetTextSelection,
                std::rc::Rc::new(|_| {}),
            );
        },
        |c| {
            c.set_value("xyz");
            c.set_text_content(second);
        },
    );
    let node = |id: SemanticsRegionId| NodeId(id.accessibility_id().as_u64());
    let field = flui_semantics::AccessibilityNodeId::from(render_id(2, 5));

    let decoded = fixture
        .owner
        .semantics_action_request_for(&select(2, (node(first_ids[0]), 2), (node(first_ids[1]), 1)))
        .expect("both ends lie in the field's runs");
    assert_eq!(decoded.node_id, field);
    assert_eq!(
        decoded.call,
        ActionCall::SetTextSelection(flui_semantics::TextSelection {
            anchor: 3,
            focus: 6
        }),
        "positions read as bytes: `ñ` is two"
    );

    for (case, request) in [
        (
            "an end in another field",
            select(2, (node(first_ids[0]), 0), (node(second_ids[0]), 1)),
        ),
        (
            "an end in a run no longer published",
            select(2, (node(first_ids[0]), 0), (node(stale), 0)),
        ),
        (
            "an end past its run",
            select(2, (node(first_ids[1]), 3), (node(first_ids[1]), 0)),
        ),
    ] {
        assert_eq!(
            fixture.owner.semantics_action_request_for(&request),
            Err(ActionDecodeError::InvalidTextPosition { node_id: field }),
            "{case}"
        );
    }

    let on_run = ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: node(first_ids[1]),
        data: None,
    };
    assert_eq!(
        fixture
            .owner
            .semantics_action_request_for(&on_run)
            .expect("a run's request decodes")
            .node_id,
        field,
        "a request on a run goes to its field"
    );
}

/// Content without identities, as when the identity space is exhausted,
/// publishes the field without runs or selection, and is reported.
#[test]
fn runs_without_identities_are_withheld_and_reported() {
    let text = TextContent::try_new(vec![run("Hi", 0, 0, TextDirection::Ltr)])
        .expect("one run")
        .with_selection(TextRunSelection::collapsed(TextRunPosition::new(0, 2)))
        .expect("the end of the run");
    let (mut fixture, received) = fields(
        |c| {
            c.set_value("Hi");
            c.set_text_content(text);
        },
        |_| {},
    );
    let update = received.lock()[0].clone();
    let (_, field) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == field_id(2))
        .expect("the field is published");
    assert!(field.children().is_empty(), "no runs");
    assert!(field.text_selection().is_none(), "and no selection");
    assert_eq!(field.value(), Some("Hi"), "the value still reads");
    let field = flui_semantics::AccessibilityNodeId::from(render_id(2, 5));
    assert!(
        fixture
            .owner
            .take_diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                SemanticsDiagnostic::TextRunsWithheld(withheld) if withheld.node == field
            )),
        "the withheld runs are reported"
    );
}

/// A run is checked against AccessKit's model before it is published.
#[test]
fn a_run_is_checked_against_accesskits_model() {
    let parts = |text: &str, lengths: Vec<u8>| {
        let count = lengths.len();
        let mut parts = TextRunParts::new(text, 0, 0, TextDirection::Ltr, Rect::ZERO);
        parts.character_lengths = lengths;
        parts.character_positions = vec![0.0; count];
        parts.character_widths = vec![0.0; count];
        parts
    };
    let full = "a".repeat(MAX_RUN_CHARACTERS);
    assert!(TextRun::try_new(parts(&full, vec![1; MAX_RUN_CHARACTERS])).is_ok());
    // A cluster longer than 255 bytes is several characters split on `char`
    // boundaries, and they count toward the limit.
    let long_cluster = format!("{}{}", "a".repeat(MAX_RUN_CHARACTERS - 2), "é".repeat(150));
    let mut lengths = vec![1; MAX_RUN_CHARACTERS - 2];
    lengths.extend([200, 100]);
    assert!(TextRun::try_new(parts(&long_cluster, lengths)).is_ok());

    let mut misaligned = parts("ab", vec![1, 1]);
    misaligned.word_starts = vec![1, 1];
    let mut unfinished = parts("ab", vec![1, 1]);
    unfinished.character_widths = vec![f32::NAN, 1.0];
    for (case, candidate, expected) in [
        (
            "past the limit",
            parts(&format!("{full}a"), vec![1; MAX_RUN_CHARACTERS + 1]),
            TextRunError::TooManyCharacters {
                count: MAX_RUN_CHARACTERS + 1,
            },
        ),
        (
            "inside a char",
            parts("é", vec![1, 1]),
            TextRunError::SplitsCharacter { character: 0 },
        ),
        (
            "short of the text",
            parts("abc", vec![1, 1]),
            TextRunError::LengthsDoNotCoverText {
                lengths: 2,
                text: 3,
            },
        ),
        (
            "a repeated word start",
            misaligned,
            TextRunError::InvalidWordStart { index: 1 },
        ),
        (
            "a width that is not finite",
            unfinished,
            TextRunError::NonFiniteGeometry,
        ),
    ] {
        assert_eq!(TextRun::try_new(candidate), Err(expected), "{case}");
    }

    assert_eq!(
        TextContent::try_new(vec![
            run("ab", 0, 0, TextDirection::Ltr),
            run("d", 3, 0, TextDirection::Ltr)
        ]),
        Err(TextRunError::RunsNotContiguous { run: 1 })
    );
    assert_eq!(
        TextContent::try_new(vec![
            run("ab", 0, 1, TextDirection::Ltr),
            run("c", 2, 0, TextDirection::Ltr)
        ]),
        Err(TextRunError::LinesDecrease { run: 1 })
    );
    assert_eq!(TextContent::try_new(Vec::new()), Err(TextRunError::NoRuns));
}

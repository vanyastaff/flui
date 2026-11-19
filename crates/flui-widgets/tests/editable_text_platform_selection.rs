//! An `EditableText` under a window whose text-store host draws the focused
//! field's selection UI (`SelectionUi::Platform`, iOS's `UITextInteraction`):
//! the field draws no caret or selection highlight and its own taps, drags
//! and double taps do nothing in it while it holds the input, a press lands
//! on it only where nothing is drawn over it, and the platform's edit menu
//! reaches it through its store as `EditCommand`s.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Offset, Point};
use flui_interaction::focus::FocusNode;
use flui_objects::RenderEditable;
use flui_painting::Alignment;
use flui_painting::typography::TextStyle;
use flui_platform::TargetPlatform;
use flui_platform::text_store::{
    EditCommand, LockGrant, LockTiming, PressTarget, SelectionUi, TextStore, TextStoreRead,
};
use flui_testing::replay::PointerScript;
use flui_view::ViewExt;
use flui_widgets::{
    ClipRect, Column, EditableText, GestureDetector, HitTestBehavior, LongPressSelection, Offstage,
    OverflowBox, Padding, Positioned, ScrollController, ScrollView, Scrollbar,
    SingleChildScrollView, SizedBox, Stack, TextEditingController, WidgetsApp,
};

use crate::common::harness::{Harness, mount_with_selection_ui_on};
use crate::common::{lay_out_with_selection_ui_on, tight};

/// What the mounted field's `RenderEditable` draws: its caret, its
/// selection highlight and its composing underline.
fn drawn(harness: &Harness) -> (bool, Option<std::ops::Range<usize>>, bool) {
    harness.pipeline_owner().with(|owner| {
        let tree = owner.render_tree();
        tree.iter()
            .find_map(|(_, node)| {
                node.as_box()
                    .and_then(|node| node.render_object().downcast_ref::<RenderEditable>()) // test-only reach to the one concrete render object type this widget mounts, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure.
                    .map(|editable| {
                        (
                            editable.show_caret(),
                            editable.selection().cloned(),
                            editable.rect_for_composing_range().is_some(),
                        )
                    })
            })
            .expect("a mounted EditableText has a RenderEditable")
    })
}

/// A field over "hello world" on an iOS window whose host answers `ui`,
/// focused, with "hello" selected.
fn focused_field(ui: SelectionUi) -> (Harness, TextEditingController, Rc<FocusNode>) {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("served field");
    let mut harness = mount_with_selection_ui_on(
        EditableText::new(controller.clone(), Rc::clone(&node)),
        TargetPlatform::iOS,
        ui,
    );
    let _ = node.request_focus();
    assert!(node.has_primary_focus(), "precondition: focused");
    controller.set_selection(0, 5);
    harness.tick();
    (harness, controller, node)
}

/// While a host that draws the selection UI serves the field, the field
/// draws no caret or selection highlight of its own and its taps, drags and
/// double taps change nothing; it still draws its composition's underline.
/// Once it holds the input no more, its own gestures are back: a press on it
/// focuses it and places its caret. The same field under a host that draws
/// nothing (the default, and every host but iOS's) draws its caret and
/// highlight and its tap moves the caret, so the rows differ only in the
/// host's answer.
///
/// Fails without the switch (the field draws its caret and highlight under
/// the platform's, and its pointer-down handler moves the caret under the
/// platform's own gesture), and without the field taking its gestures back
/// when it loses the input (the press on the unfocused field does nothing).
pub(crate) fn a_field_leaves_its_selection_ui_to_a_platform_that_draws_it() {
    for (ui, draws_itself) in [(SelectionUi::Field, true), (SelectionUi::Platform, false)] {
        let (mut harness, controller, node) = focused_field(ui);
        let (_, selection, _) = drawn(&harness);
        assert_eq!(selection.is_some(), draws_itself, "{ui:?}: the highlight");
        controller.set_caret_byte_offset(11);
        harness.tick();
        let (caret, _, _) = drawn(&harness);
        assert_eq!(caret, draws_itself, "{ui:?}: the caret");

        // A tap, a drag and a double tap at the field's left edge.
        harness.dispatch_pointer_down(1.0, 5.0);
        harness.dispatch_pointer_move(40.0, 5.0);
        harness.dispatch_pointer_up(40.0, 5.0);
        harness.pump_for(Duration::from_millis(40));
        harness.dispatch_pointer_down(1.0, 5.0);
        harness.dispatch_pointer_up(1.0, 5.0);
        harness.pump_for(Duration::from_millis(500));
        assert_eq!(
            controller.selection() == (11..11),
            !draws_itself,
            "{ui:?}: the field's own gestures move its caret only when it draws its selection UI"
        );

        // The composition's underline is the field's under either host.
        controller.set_caret_byte_offset(11);
        harness.dispatch_ime(&flui_platform::ImeEvent::Preedit {
            text: "ni".to_owned(),
            cursor: Some((0, 2)),
        });
        harness.tick();
        let (_, _, underline) = drawn(&harness);
        assert!(underline, "{ui:?}: the composing underline");

        // Once the field holds the input no more, its gestures are its own
        // again: a press focuses it and places the caret where it lands.
        node.unfocus();
        harness.tick();
        assert!(!node.has_primary_focus());
        controller.set_caret_byte_offset(11);
        harness.dispatch_pointer_down(1.0, 5.0);
        harness.dispatch_pointer_up(1.0, 5.0);
        harness.pump_for(Duration::from_millis(500));
        assert!(
            node.has_primary_focus(),
            "{ui:?}: a press on the unfocused field focuses it"
        );
        assert_eq!(
            controller.selection(),
            0..0,
            "{ui:?}: the press placed the caret where it landed"
        );
    }
}

/// A touch long press on a field that does not hold the input is the
/// field's: it selects the word and focuses the field. Under a host that
/// draws the selection UI the field then leaves it to the platform, and the
/// handles that press would show go with it, so no two sets of handles stand
/// over the field; under a host that draws nothing they stay.
///
/// Fails without taking the field's own handles down at the takeover: the
/// press leaves two handles in the overlay under either host.
pub(crate) fn the_fields_own_handles_go_when_the_platform_takes_the_selection_ui() {
    for (ui, handles) in [(SelectionUi::Field, 2), (SelectionUi::Platform, 0)] {
        let controller = TextEditingController::with_text("alpha beta gamma");
        let node = FocusNode::with_debug_label("long-pressed field");
        let field = EditableText::new(controller, Rc::clone(&node))
            .text_style(TextStyle::default().with_font_size(16.0))
            // iOS's own long press moves the caret, whose handle it does not
            // show; a word press shows one under each end.
            .long_press(LongPressSelection::Word)
            // The rows count the handles' followers; the toolbar would add one.
            .no_menu();
        let mut laid = lay_out_with_selection_ui_on(
            WidgetsApp::new(Column::new((SizedBox::new(400.0, 40.0).child(field),))),
            tight(400.0, 300.0),
            TargetPlatform::iOS,
            ui,
        );
        laid.pump();
        let editable = laid.find_all_by_render_type("RenderEditable")[0];
        let at = laid.absolute_offset(editable) + Offset::new(60.0, 10.0);
        laid.replay(&PointerScript::long_press(at, Duration::from_millis(800)));
        for _ in 0..4 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            node.has_primary_focus(),
            "{ui:?}: the press focused the field"
        );
        assert_eq!(
            laid.find_all_by_render_type("RenderFollowerLayer").len(),
            handles,
            "{ui:?}: the field's own handles"
        );
    }
}

/// What `read` answers of the store's field under a synchronous read lock.
fn read_store<R: 'static>(
    store: &Rc<dyn TextStore>,
    read: impl FnOnce(&dyn TextStoreRead) -> R + 'static,
) -> R {
    let answer = Rc::new(RefCell::new(None));
    let out = Rc::clone(&answer);
    let _granted = store
        .request_lock(
            LockGrant::read(move |session| *out.borrow_mut() = Some(read(session))),
            LockTiming::Sync,
        )
        .expect("a synchronous read outside a frame is granted");
    answer.take().expect("the read ran")
}

fn press_target_at(store: &Rc<dyn TextStore>, point: Point<f64>) -> PressTarget {
    read_store(store, move |session| session.press_target_at(point)).expect("the field is laid out")
}

/// A press lands on the field only where the window's own hit test finds the
/// field: a button drawn over its right half keeps the press there (another
/// target), and its tap still reaches it with the platform owning the
/// field's gestures; the bare left half is the field's; a button beside the
/// field, within a handle's reach, is another target; the bare page below is
/// nothing.
///
/// Fails with the store answering from the field's box alone: the point over
/// the button lies inside the box.
pub(crate) fn a_control_over_the_field_keeps_its_press() {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("covered field");
    let taps = Rc::new(Cell::new(0));
    let tapped = Rc::clone(&taps);
    let button = |taps: Rc<Cell<usize>>| {
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(move |_ctx| taps.set(taps.get() + 1))
            .child(SizedBox::new(100.0, 40.0))
    };
    let page = SizedBox::new(400.0, 200.0).child(Stack::new((
        SizedBox::new(200.0, 40.0).child(EditableText::new(controller, Rc::clone(&node))),
        Positioned::new(button(tapped)).left(100.0).top(0.0),
        Positioned::new(button(Rc::new(Cell::new(0))))
            .left(205.0)
            .top(0.0),
    )));
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");

    assert_eq!(
        press_target_at(&store, Point::new(20.0, 10.0)),
        PressTarget::Field,
        "the bare field"
    );
    assert_eq!(
        press_target_at(&store, Point::new(150.0, 10.0)),
        PressTarget::Other,
        "the button over the field"
    );
    assert_eq!(
        press_target_at(&store, Point::new(210.0, 10.0)),
        PressTarget::Other,
        "the button beside the field"
    );
    assert_eq!(
        press_target_at(&store, Point::new(20.0, 150.0)),
        PressTarget::Nothing,
        "the bare page"
    );
    harness.dispatch_pointer_down(150.0, 10.0);
    harness.dispatch_pointer_up(150.0, 10.0);
    harness.pump_for(Duration::from_millis(500));
    assert_eq!(taps.get(), 1, "the button took its tap");
}

/// A press the field's own ancestors would take keeps it: a scrollbar's thumb
/// beside the field, whose detector wraps the scroll view and so the field,
/// and a tappable row around the field, pressed outside the field. The bare
/// page beside the field under the same bar is nobody's, the scroll view's
/// drag taking a press only past its slop.
///
/// Fails with ancestors' targets left out: the thumb and the row answer
/// `Nothing`, and UIKit's gesture would take their press near a handle.
pub(crate) fn a_control_an_ancestor_draws_beside_the_field_keeps_its_press() {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("field under a scrollbar");
    let page = SizedBox::new(400.0, 200.0).child(
        Scrollbar::new(
            SingleChildScrollView::new().child(SizedBox::new(400.0, 600.0).child(Stack::new((
                SizedBox::new(300.0, 40.0).child(EditableText::new(controller, Rc::clone(&node))),
            )))),
        )
        .thumb_visibility(true),
    );
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    for (point, target, case) in [
        ((397.0, 10.0), PressTarget::Other, "the scrollbar's thumb"),
        (
            (350.0, 10.0),
            PressTarget::Nothing,
            "the page beside the field",
        ),
        ((20.0, 10.0), PressTarget::Field, "the field"),
    ] {
        assert_eq!(
            press_target_at(&store, Point::new(point.0, point.1)),
            target,
            "{case}"
        );
    }

    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("field in a row");
    let row = GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(|_ctx| {})
        .child(SizedBox::new(400.0, 60.0).child(Stack::new((
            SizedBox::new(200.0, 40.0).child(EditableText::new(controller, Rc::clone(&node))),
        ))));
    let mut harness =
        mount_with_selection_ui_on(row.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    assert_eq!(
        press_target_at(&store, Point::new(250.0, 10.0)),
        PressTarget::Other,
        "the row around the field, outside it"
    );
    assert_eq!(
        press_target_at(&store, Point::new(20.0, 10.0)),
        PressTarget::Field,
        "the field in the row"
    );
}

/// Inside a scroll view, whose gesture detector is on every press's path
/// through the field, a press on the bare page beside the field lands on
/// nothing of anyone else's, so the platform's handles keep their reach
/// there; a control drawn over the field and another field below it each
/// keep their press.
///
/// Fails with every pointer target on the path counted as another target:
/// the scroll view's own detector makes the bare spot beside the field one.
pub(crate) fn a_scroll_view_around_the_field_leaves_the_handles_their_reach() {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("scrolled field");
    let other = FocusNode::with_debug_label("the field below");
    let page = SizedBox::new(400.0, 200.0).child(
        SingleChildScrollView::new().child(
            SizedBox::new(400.0, 400.0).child(Stack::new((
                SizedBox::new(200.0, 40.0).child(EditableText::new(controller, Rc::clone(&node))),
                Positioned::new(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(|_ctx| {})
                        .child(SizedBox::new(50.0, 40.0)),
                )
                .left(100.0)
                .top(0.0),
                Positioned::new(SizedBox::new(200.0, 40.0).child(EditableText::new(
                    TextEditingController::with_text("below"),
                    other,
                )))
                .left(0.0)
                .top(100.0),
            ))),
        ),
    );
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");

    for (point, target, case) in [
        ((20.0, 10.0), PressTarget::Field, "the bare field"),
        (
            (120.0, 10.0),
            PressTarget::Other,
            "the control over the field",
        ),
        (
            (210.0, 10.0),
            PressTarget::Nothing,
            "the page beside the field",
        ),
        ((20.0, 110.0), PressTarget::Other, "the field below"),
    ] {
        assert_eq!(
            press_target_at(&store, Point::new(point.0, point.1)),
            target,
            "{case}"
        );
    }
}

/// A field taller than the viewport it scrolls in shows the same part of
/// the window wherever it is scrolled to while its text moves under it: a
/// scroll of 50 tells the observer the field moved, once.
///
/// Fails with only the shown part compared: it is the whole viewport before
/// and after the scroll.
pub(crate) fn a_field_scrolled_inside_a_viewport_it_overflows_says_it_moved() {
    let controller = TextEditingController::with_text("hello");
    let node = FocusNode::with_debug_label("tall field");
    let scroll = ScrollController::new();
    let page = SizedBox::new(400.0, 200.0).child(
        SingleChildScrollView::new()
            .controller(scroll.clone())
            .child(
                SizedBox::new(400.0, 600.0).child(EditableText::new(controller, Rc::clone(&node))),
            ),
    );
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    let notices = Rc::new(CountedNotices::default());
    store.set_observer(Some(
        Rc::clone(&notices) as Rc<dyn flui_platform::text_store::TextStoreObserver>
    ));
    harness.tick();
    let shown = || {
        read_store(&store, |session| session.visible_bounds())
            .expect("laid out")
            .expect("the field fills the viewport")
    };
    let before = shown();

    scroll.jump_to(50.0).expect("the scroll view is mounted");
    harness.tick();
    harness.tick();
    assert_eq!(shown(), before, "the field still fills the viewport");
    assert_eq!(notices.moves.get(), 1, "the scrolled field moved once");
}

/// A field the paint walk draws nothing of, offstage, shows no part of
/// itself, so the platform draws no selection for it.
///
/// Fails with the ancestors' clips walked alone: an offstage parent clips
/// nothing, and the field's box was reported.
pub(crate) fn an_offstage_field_shows_nothing() {
    let controller = TextEditingController::with_text("hello");
    let node = FocusNode::with_debug_label("offstage field");
    let field = |offstage: bool| {
        Offstage::new()
            .offstage(offstage)
            .child(EditableText::new(controller.clone(), Rc::clone(&node)))
            .boxed()
    };
    let mut harness =
        mount_with_selection_ui_on(field(false), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    let shown = |store: &Rc<dyn TextStore>| {
        read_store(store, |session| session.visible_bounds()).expect("laid out")
    };
    assert!(shown(&store).is_some(), "onstage, the field shows");

    harness.swap_root(field(true));
    harness.tick();
    assert_eq!(shown(&store), None, "offstage, it shows nothing");
}

/// The part of the field the window shows is its box cut by every clip
/// above it: a field half under the edge of a clipping ancestor shows only
/// the half inside, which is where a platform draws its selection.
///
/// Fails with the store answering its own box: the hidden half is included.
pub(crate) fn a_field_shows_only_what_its_ancestors_clip_leaves() {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("clipped field");
    let page = ClipRect::new().child(
        SizedBox::new(100.0, 40.0).child(
            OverflowBox::new()
                .max_width(200.0)
                .alignment(Alignment::TOP_LEFT)
                .child(
                    SizedBox::new(200.0, 40.0)
                        .child(EditableText::new(controller, Rc::clone(&node))),
                ),
        ),
    );
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    let whole = read_store(&store, |session| session.document_bounds()).expect("laid out");
    let shown = read_store(&store, |session| session.visible_bounds())
        .expect("laid out")
        .expect("half of the field is shown");
    assert_eq!(whole.size.width, 200.0, "the field's own box");
    assert_eq!(shown.size.width, 100.0, "what the clip leaves of it");
    assert_eq!(shown.origin, whole.origin);
}

/// Counts what the field's store tells its observer.
#[derive(Default)]
struct CountedNotices {
    layouts: Cell<usize>,
    moves: Cell<usize>,
}

impl flui_platform::text_store::TextStoreObserver for CountedNotices {
    fn text_changed(&self, _change: flui_platform::text_store::TextChange) {}
    fn selection_changed(&self) {}
    fn layout_changed(&self) {
        self.layouts.set(self.layouts.get() + 1);
    }
    fn field_moved(&self) {
        self.moves.set(self.moves.get() + 1);
    }
    fn status_changed(&self) {}
}

/// Typing and composing move the field's caret and tell its observer of a
/// layout change, never that the field moved; moving the field in the window
/// (here, a padding above it growing) tells it once.
///
/// Fails with every caret move reported as a move of the field, and with a
/// move of the field not reported.
pub(crate) fn typing_moves_the_caret_and_only_a_moved_field_says_it_moved() {
    let controller = TextEditingController::with_text("");
    let node = FocusNode::with_debug_label("moving field");
    let field = |top: f64| {
        Padding::new(EdgeInsets::only_top(top))
            .child(EditableText::new(controller.clone(), Rc::clone(&node)))
            .boxed()
    };
    let mut harness =
        mount_with_selection_ui_on(field(0.0), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    let notices = Rc::new(CountedNotices::default());
    store.set_observer(Some(
        Rc::clone(&notices) as Rc<dyn flui_platform::text_store::TextStoreObserver>
    ));

    for text in ["a", "ab", "abc"] {
        harness.dispatch_ime(&flui_platform::ImeEvent::Commit(
            text[text.len() - 1..].to_owned(),
        ));
        harness.tick();
    }
    harness.dispatch_ime(&flui_platform::ImeEvent::Preedit {
        text: "ni".to_owned(),
        cursor: Some((0, 2)),
    });
    harness.tick();
    assert!(notices.layouts.get() > 0, "the caret moved");
    assert_eq!(notices.moves.get(), 0, "the field did not move");

    harness.swap_root(field(30.0));
    harness.tick();
    harness.tick();
    assert_eq!(notices.moves.get(), 1, "the field moved down once");
}

/// The platform's edit menu reaches the focused field through its store:
/// Copy writes the selection, Select All selects the whole text, Select
/// selects the word at a collapsed caret, Cut deletes what the clipboard
/// took and Paste inserts at the caret, each as the field's chords do. A
/// field without the input performs nothing.
///
/// Fails without the command path: the store performs no command and enables
/// none.
pub(crate) fn the_platforms_edit_menu_reaches_the_field_through_its_store() {
    let (mut harness, controller, node) = focused_field(SelectionUi::Platform);
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");

    assert!(store.edit_command_enabled(EditCommand::Copy));
    assert!(
        !store.edit_command_enabled(EditCommand::SelectWord),
        "a selection offers no Select"
    );
    assert!(store.perform_edit_command(EditCommand::Copy));
    assert_eq!(harness.clipboard().text().as_deref(), Some("hello"));
    assert_eq!(controller.selection(), 0..5, "a copy keeps the selection");

    assert!(store.perform_edit_command(EditCommand::SelectAll));
    assert_eq!(controller.selection(), 0..11);

    // Select takes the word at the caret, and past punctuation the word
    // before it, never the punctuation or the space.
    for (text, caret, word) in [
        ("hello world", 8, 6..11),
        ("hello world", 11, 6..11),
        ("hello world", 6, 6..11),
        ("hello world", 2, 0..5),
        ("hello, world", 6, 0..5),
        ("hello, world", 5, 0..5),
        ("hello, world", 7, 7..12),
        ("(hello)", 7, 1..6),
        ("!! x", 0, 3..4),
        // A line break bounds the search: the nearest word on the caret's
        // own line, never one past a break.
        ("abc\r, xyz", 5, 6..9),
        ("abc,\nxyz", 4, 0..3),
    ] {
        controller.set_text(text);
        controller.set_caret_byte_offset(caret);
        assert!(
            store.edit_command_enabled(EditCommand::SelectWord),
            "{text:?} at {caret}"
        );
        assert!(store.perform_edit_command(EditCommand::SelectWord));
        assert_eq!(controller.selection(), word, "{text:?} at {caret}");
    }
    for (text, caret) in [("!!", 1), ("abc,\n\nxyz", 5), ("abc\n, \nxyz", 4)] {
        controller.set_text(text);
        controller.set_caret_byte_offset(caret);
        assert!(
            !store.edit_command_enabled(EditCommand::SelectWord),
            "{text:?} at {caret}: a line with no word offers no Select"
        );
    }
    controller.set_text("hello world");
    controller.set_selection(0, 5);

    controller.set_selection(0, 6);
    assert!(store.perform_edit_command(EditCommand::Cut));
    harness.tick();
    assert_eq!(controller.text(), "world");
    assert_eq!(harness.clipboard().text().as_deref(), Some("hello "));

    controller.set_caret_byte_offset(5);
    assert!(store.perform_edit_command(EditCommand::Paste));
    harness.tick();
    assert_eq!(controller.text(), "worldhello ");

    node.unfocus();
    harness.tick();
    assert!(
        !store.edit_command_enabled(EditCommand::Paste),
        "a field without the input takes no paste"
    );
    assert!(!store.perform_edit_command(EditCommand::Paste));
    assert_eq!(controller.text(), "worldhello ");
}

/// An obscured field's menu offers no Copy, Cut or Select, as a password
/// field's does not, and a command it is asked for anyway leaves the
/// clipboard and the selection alone; Select All is still offered.
pub(crate) fn an_obscured_field_offers_the_platform_no_copy_or_cut() {
    let controller = TextEditingController::with_text("secret");
    let node = FocusNode::with_debug_label("password field");
    let mut harness = mount_with_selection_ui_on(
        EditableText::new(controller.clone(), Rc::clone(&node)).obscure_text(true),
        TargetPlatform::iOS,
        SelectionUi::Platform,
    );
    let _ = node.request_focus();
    controller.set_selection(0, 6);
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");
    for command in [EditCommand::Copy, EditCommand::Cut] {
        assert!(!store.edit_command_enabled(command), "{command:?}");
        assert!(!store.perform_edit_command(command), "{command:?}");
    }
    controller.set_caret_byte_offset(3);
    assert!(!store.edit_command_enabled(EditCommand::SelectWord));
    assert!(!store.perform_edit_command(EditCommand::SelectWord));
    assert_eq!(controller.selection(), 3..3);
    assert!(store.edit_command_enabled(EditCommand::SelectAll));
    assert_eq!(harness.clipboard().text(), None);
    assert_eq!(controller.text(), "secret");
}

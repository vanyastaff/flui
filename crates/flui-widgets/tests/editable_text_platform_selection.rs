//! An `EditableText` under a window whose text-store host draws the focused
//! field's selection UI (`SelectionUi::Platform`, iOS's `UITextInteraction`):
//! the field draws no caret or selection highlight and its own taps, drags
//! and double taps do nothing in it while it holds the input, a press lands
//! on it only where nothing is drawn over it, and the platform's edit menu
//! reaches it through its store as `EditCommand`s.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::focus::FocusNode;
use flui_objects::RenderEditable;
use flui_painting::typography::TextStyle;
use flui_platform::TargetPlatform;
use flui_platform::text_store::{EditCommand, LockGrant, LockTiming, SelectionUi, TextStore};
use flui_testing::replay::PointerScript;
use flui_view::ViewExt;
use flui_widgets::{
    Column, EditableText, GestureDetector, HitTestBehavior, LongPressSelection, Positioned,
    SizedBox, Stack, TextEditingController, WidgetsApp,
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

/// Whether the store says a press at `point` lands on its field.
fn is_hit_at(store: &Rc<dyn TextStore>, point: Point<f64>) -> bool {
    let answer = Rc::new(Cell::new(None));
    let out = Rc::clone(&answer);
    let _granted = store
        .request_lock(
            LockGrant::read(move |session| out.set(Some(session.is_hit_at(point)))),
            LockTiming::Sync,
        )
        .expect("a synchronous read outside a frame is granted");
    answer
        .take()
        .expect("the read ran")
        .expect("the field is laid out")
}

/// A press lands on the field only where the window's own hit test finds the
/// field: a button drawn over its right half keeps the press there, and the
/// button's tap still reaches it with the platform owning the field's
/// gestures; the bare left half is the field's.
///
/// Fails with the store answering from the field's box alone: the point over
/// the button lies inside the box.
pub(crate) fn a_control_over_the_field_keeps_its_press() {
    let controller = TextEditingController::with_text("hello world");
    let node = FocusNode::with_debug_label("covered field");
    let taps = Rc::new(Cell::new(0));
    let tapped = Rc::clone(&taps);
    let page = Stack::new((
        SizedBox::new(200.0, 40.0).child(EditableText::new(controller, Rc::clone(&node))),
        Positioned::new(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move |_ctx| tapped.set(tapped.get() + 1))
                .child(SizedBox::new(100.0, 40.0)),
        )
        .left(100.0)
        .top(0.0),
    ));
    let mut harness =
        mount_with_selection_ui_on(page.boxed(), TargetPlatform::iOS, SelectionUi::Platform);
    let _ = node.request_focus();
    harness.tick();
    let store = harness
        .active_text_store()
        .expect("the focused field's store is served");

    assert!(is_hit_at(&store, Point::new(20.0, 10.0)), "the bare field");
    assert!(
        !is_hit_at(&store, Point::new(150.0, 10.0)),
        "the button over the field"
    );
    harness.dispatch_pointer_down(150.0, 10.0);
    harness.dispatch_pointer_up(150.0, 10.0);
    harness.pump_for(Duration::from_millis(500));
    assert_eq!(taps.get(), 1, "the button took its tap");
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

    for (caret, word) in [(8, 6..11), (11, 6..11), (6, 6..11), (2, 0..5)] {
        controller.set_caret_byte_offset(caret);
        assert!(
            store.edit_command_enabled(EditCommand::SelectWord),
            "a caret at {caret}"
        );
        assert!(store.perform_edit_command(EditCommand::SelectWord));
        assert_eq!(controller.selection(), word, "a caret at {caret}");
    }

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

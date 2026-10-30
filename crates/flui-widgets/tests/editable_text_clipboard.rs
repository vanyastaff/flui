//! Copy, cut and paste in a mounted [`EditableText`], through the key path a
//! platform chord takes: `FocusManager::dispatch_key_event` → the field's key
//! handler leaves the chord unconsumed → `DefaultFocusTraversal`'s
//! `Shortcuts` (installed by the harness's `FocusRoot`) resolves the intent
//! against the chain the field recorded on its node. The clipboard read back
//! is the one the harness installs, the headless platform's type.

use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::focus::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_widgets::{EditableText, TextEditingController};

/// The platform's command modifier, as `DefaultFocusTraversal` binds it.
fn command() -> Modifiers {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

fn chord(character: &str, modifiers: Modifiers) -> KeyEvent {
    KeyEventBuilder::new(Code::KeyC)
        .with_key(Key::character(character))
        .with_state(KeyState::Down)
        .with_modifiers(modifiers)
        .build()
}

fn mount_field(
    controller: &TextEditingController,
    configure: impl FnOnce(EditableText) -> EditableText,
) -> (crate::common::harness::Harness, Rc<FocusNode>) {
    let focus_node = FocusNode::with_debug_label("clipboard field");
    let harness = crate::common::harness::mount(configure(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    )));
    let _ = focus_node.request_focus();
    assert!(focus_node.has_primary_focus(), "precondition: focused");
    (harness, focus_node)
}

/// Copy writes the selection and keeps it; paste inserts at the caret.
///
/// Fails without the change: the chord was left unconsumed by the field and
/// bound by nothing, so the clipboard stayed empty.
pub(crate) fn copy_then_paste_round_trips_text_in_an_editable_text() {
    let controller = TextEditingController::with_text("abc");
    let (mut harness, _node) = mount_field(&controller, |field| field);
    controller.set_selection(0, 3);

    let consumed = harness
        .focus_manager()
        .dispatch_key_event(&chord("c", command()))
        .is_handled();
    assert!(consumed, "copy consumes the chord");
    assert_eq!(harness.clipboard().text().as_deref(), Some("abc"));
    assert_eq!(controller.selection(), 0..3, "copy keeps the selection");

    controller.set_caret_byte_offset(3);
    let _ = harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()))
        .is_handled();
    assert_eq!(
        controller.text(),
        "abc",
        "the read settles at the next owner turn, never inside the chord"
    );
    harness.tick();
    assert_eq!(controller.text(), "abcabc");
    assert_eq!(controller.caret_byte_offset(), 6);
}

/// A password field never hands its text to the clipboard, and the chord it
/// declines keeps bubbling.
pub(crate) fn copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed()
 {
    let controller = TextEditingController::with_text("secret");
    let (harness, _node) = mount_field(&controller, |field| field.obscure_text(true));
    controller.set_selection(0, 6);

    for key in ["c", "x"] {
        let consumed = harness
            .focus_manager()
            .dispatch_key_event(&chord(key, command()))
            .is_handled();
        assert!(
            !consumed,
            "{key}: a disabled action leaves the key unconsumed"
        );
    }
    assert_eq!(harness.clipboard().text(), None);
    assert_eq!(controller.text(), "secret", "cut deleted nothing");
}

pub(crate) fn select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit()
 {
    crate::common::cases::run_cases(
        "select all",
        &[
            ("plain field", select_all_plain as fn()),
            ("obscured field", select_all_obscured),
        ],
    );
}

fn select_all_plain() {
    select_all_replacement(false);
}

fn select_all_obscured() {
    select_all_replacement(true);
}

fn select_all_replacement(obscured: bool) {
    use std::cell::Cell;
    let controller = TextEditingController::new();
    let changes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&changes);
    let (harness, _node) = mount_field(&controller, move |field| {
        field
            .obscure_text(obscured)
            .on_changed(move |_cx, _text| counted.set(counted.get() + 1))
    });
    let text = "A😀e\u{301}東京";
    for scalar in text.chars() {
        assert!(
            harness
                .focus_manager()
                .dispatch_key_event(&chord(&scalar.to_string(), Modifiers::NONE))
                .is_handled()
        );
    }
    let edits = changes.get();
    let other_command = if command() == Modifiers::META {
        Modifiers::CONTROL
    } else {
        Modifiers::META
    };
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", other_command))
            .is_handled()
    );
    assert!(
        !controller.has_selection(),
        "the other platform's command chord does not select text"
    );
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("A", command()))
            .is_handled(),
        "Caps Lock does not defeat select all"
    );
    assert_eq!(
        controller.selection(),
        0..text.len(),
        "the whole UTF-8 document is selected"
    );
    assert_eq!(controller.selected_text(), text);
    assert_eq!(controller.text(), text, "selecting never changes text");
    assert_eq!(changes.get(), edits, "selection does not call on_changed");
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("文", Modifiers::NONE))
            .is_handled()
    );
    assert_eq!(
        controller.text(),
        "文",
        "a real typed scalar replaces every selected grapheme"
    );
    assert_eq!(controller.caret_byte_offset(), "文".len());
    assert!(!controller.has_selection());
    assert_eq!(changes.get(), edits + 1, "replacement is one user edit");
    assert_eq!(
        harness.clipboard().text(),
        None,
        "selection requires no clipboard operation"
    );
}

pub(crate) fn select_all_without_a_focused_text_field_leaves_the_key_unconsumed() {
    let controller = TextEditingController::with_text("retained");
    let (harness, node) = mount_field(&controller, |field| field);
    node.set_can_request_focus(false);
    assert!(!node.has_primary_focus());
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
            .is_handled()
    );
    assert!(!controller.has_selection());
    assert_eq!(controller.text(), "retained");
}

pub(crate) fn select_all_defers_to_an_active_composition_and_recovers_after_commit() {
    let controller = TextEditingController::new();
    let node = FocusNode::new();
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&node),
    ));
    let _ = node.request_focus();
    harness.dispatch_ime(&flui_platform::ImeEvent::Preedit {
        text: "東京".to_owned(),
        cursor: Some(("東京".len(), "東京".len())),
    });
    let before = controller.selection();
    assert!(controller.is_composing());
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
            .is_handled()
    );
    assert_eq!(controller.selection(), before, "IME retains its selection");
    assert_eq!(controller.text(), "東京");
    harness.dispatch_ime(&flui_platform::ImeEvent::Commit("東京".to_owned()));
    assert!(!controller.is_composing());
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
            .is_handled()
    );
    assert_eq!(
        controller.selection(),
        0.."東京".len(),
        "select all resumes after commit"
    );
}

pub(crate) fn paste_rechecks_focus_after_committing_composition() {
    let controller = TextEditingController::new();
    let node = FocusNode::new();
    let retiring = node.clone();
    let mut harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), node.clone())
            .on_changed(move |_cx, _text| retiring.unfocus()),
    );
    let _ = node.request_focus();
    harness.clipboard().set_text("paste");
    harness.dispatch_ime(&flui_platform::ImeEvent::Preedit {
        text: "composition".into(),
        cursor: None,
    });
    assert!(
        node.has_primary_focus(),
        "preedit does not report a committed edit"
    );
    let _ = harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()))
        .is_handled();
    // The read settles at the next owner turn; the field is unfocused by then.
    harness.tick();
    assert_eq!(
        controller.text(),
        "composition",
        "the composition committed, but an unfocused field cannot paste"
    );
}

/// A field's selection is offered as the primary selection and read only
/// when asked, so a growing selection keeps one claim that answers with what
/// is selected then; an empty selection lets go, and an obscured field never
/// offers its text.
///
/// Fails without the change: nothing claimed the primary selection.
pub(crate) fn a_field_selection_is_offered_lazily_as_the_primary_selection() {
    let controller = TextEditingController::with_text("hello world");
    let (harness, _node) = mount_field(&controller, |field| field);
    let primary = harness.primary_selection();
    assert!(!primary.is_claimed(), "a caret selects nothing");
    controller.set_selection(0, 5);
    assert!(primary.is_claimed());
    controller.set_selection(0, 11);
    assert_eq!(
        primary.text().as_deref(),
        Some("hello world"),
        "the claim reads the selection when asked"
    );

    primary.select("elsewhere");
    assert!(!primary.is_claimed(), "another application selected");
    controller.set_selection(6, 11);
    assert_eq!(
        primary.text().as_deref(),
        Some("world"),
        "a changed selection claims again"
    );

    controller.set_caret_byte_offset(3);
    assert!(!primary.is_claimed(), "an empty selection lets go");

    let secret = TextEditingController::with_text("secret");
    let (harness, _node) = mount_field(&secret, |field| field.obscure_text(true));
    secret.set_selection(0, 6);
    assert!(
        !harness.primary_selection().is_claimed(),
        "an obscured field offers nothing"
    );
}

/// A middle press pastes the primary selection where it lands, once the
/// read settles, and starts no selection drag.
///
/// Fails without the change: the press moved the caret and pasted nothing.
pub(crate) fn a_middle_press_pastes_the_primary_selection_where_it_lands() {
    use flui_interaction::events::PointerButton;

    let controller = TextEditingController::with_text("abc");
    let focus_node = FocusNode::with_debug_label("middle-pressed field");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    harness.primary_selection().select("XY\n");

    harness.dispatch_pointer_down_with_button(1.0, 5.0, PointerButton::AUXILIARY);
    assert!(
        focus_node.has_primary_focus(),
        "the press focuses the field"
    );
    assert_eq!(controller.text(), "abc", "the read settles at a later turn");
    harness.tick();
    assert_eq!(
        controller.text(),
        "XYabc",
        "pasted at the press, line break dropped"
    );
    assert_eq!(controller.caret_byte_offset(), 2);

    harness.dispatch_pointer_move(40.0, 5.0);
    assert!(!controller.has_selection(), "the press started no drag");
}

/// A field mounted over a controller that already holds a selection offers
/// it at once: no change will report it.
pub(crate) fn a_selection_held_before_mounting_is_offered() {
    let controller = TextEditingController::with_text("hello world");
    controller.set_selection(0, 5);
    let (harness, _node) = mount_field(&controller, |field| field);
    let primary = harness.primary_selection();
    assert!(primary.is_claimed(), "the existing selection is claimed");
    assert_eq!(primary.text().as_deref(), Some("hello"));
}

/// A controller listener that panics when the middle-press paste lands
/// cannot lose the paste: the caret move and the insertion are one edit.
pub(crate) fn a_panicking_listener_keeps_the_middle_press_paste() {
    use flui_interaction::events::PointerButton;
    use std::cell::Cell;

    let controller = TextEditingController::with_text("abc");
    let focus_node = FocusNode::with_debug_label("middle-pressed field");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    harness.primary_selection().select("XY");
    harness.dispatch_pointer_down_with_button(1.0, 5.0, PointerButton::AUXILIARY);

    let armed = Rc::new(Cell::new(true));
    let _listener = controller.listenable().add_listener(Rc::new({
        let armed = Rc::clone(&armed);
        move || {
            assert!(!armed.replace(false), "listener failed");
        }
    }));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
    assert!(!armed.get(), "the paste notified the listener");
    assert_eq!(controller.text(), "XYabc", "the paste landed whole");
}

/// Acquires the primary selection in `init_state` and hands the handle out.
#[derive(Clone)]
struct PrimaryAcquirer {
    handle: Rc<std::cell::RefCell<Option<flui_interaction::PrimarySelectionHandle>>>,
}

struct PrimaryAcquirerState {
    handle: Rc<std::cell::RefCell<Option<flui_interaction::PrimarySelectionHandle>>>,
}

impl flui_view::StatefulView for PrimaryAcquirer {
    type State = PrimaryAcquirerState;

    fn create_state(&self) -> Self::State {
        PrimaryAcquirerState {
            handle: Rc::clone(&self.handle),
        }
    }
}

impl flui_view::View for PrimaryAcquirer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl flui_view::ViewState<PrimaryAcquirer> for PrimaryAcquirerState {
    fn init_state(&mut self, cx: &dyn flui_view::LifecycleContext) {
        use flui_view::LifecycleContextExt as _;
        self.handle.replace(
            cx.capability::<flui_interaction::PrimarySelectionCapability>()
                .ok(),
        );
    }

    fn build(
        &self,
        _view: &PrimaryAcquirer,
        _cx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// Collapses a field's selection as it drops: another selection's capture
/// that changes the field while the field's own claim replaces it.
struct CollapsesOnDrop(TextEditingController);

impl Drop for CollapsesOnDrop {
    fn drop(&mut self) {
        self.0.set_caret_byte_offset(0);
    }
}

/// The field's claim replaces a selection whose capture empties the field's
/// selection as it drops, inside the claim: that newer change stands, and
/// the field holds no claim for a selection it no longer has.
pub(crate) fn a_selection_changed_while_the_field_claims_stands() {
    let controller = TextEditingController::with_text("hello world");
    let handle = Rc::new(std::cell::RefCell::new(None));
    let harness = crate::common::harness::mount(flui_widgets::Column::new((
        PrimaryAcquirer {
            handle: Rc::clone(&handle),
        },
        EditableText::new(
            controller.clone(),
            FocusNode::with_debug_label("re-entered field"),
        ),
    )));
    let primary = harness.primary_selection();
    let other = handle
        .borrow()
        .clone()
        .expect("the headless host provides a primary selection")
        .claim({
            let capture = CollapsesOnDrop(controller.clone());
            move || {
                let _ = &capture;
                Some("elsewhere".to_owned())
            }
        });
    assert!(other.is_current());

    controller.set_selection(0, 5);
    assert!(
        !controller.has_selection(),
        "the replaced capture collapsed it"
    );
    assert!(
        !primary.is_claimed(),
        "the field let go with the newer, empty selection"
    );
    drop(other);
}

/// Reads the primary selection as it drops: a selection whose owner asks
/// the platform what replaced it.
struct ReadsOnDrop {
    primary: flui_testing::ScriptedPrimarySelection,
    read: Rc<std::cell::RefCell<Vec<Option<String>>>>,
}

impl Drop for ReadsOnDrop {
    fn drop(&mut self) {
        self.read.borrow_mut().push(self.primary.text());
    }
}

/// A rebuild that both unobscures a field and replaces its controller
/// claims for the replacement: what replaced the selection never reads the
/// old, obscured controller's text.
pub(crate) fn an_unobscuring_rebuild_claims_for_the_new_controller() {
    let secret = TextEditingController::with_text("secret");
    secret.set_selection(0, 6);
    let shown = TextEditingController::with_text("shown");
    shown.set_selection(0, 5);
    let focus = FocusNode::with_debug_label("rebuilt field");
    let handle = Rc::new(std::cell::RefCell::new(None));
    let field = |controller: &TextEditingController, obscure: bool| {
        flui_widgets::Column::new((
            PrimaryAcquirer {
                handle: Rc::clone(&handle),
            },
            EditableText::new(controller.clone(), Rc::clone(&focus)).obscure_text(obscure),
        ))
    };
    let mut harness = crate::common::harness::mount(field(&secret, true));
    let primary = harness.primary_selection();
    assert!(!primary.is_claimed(), "an obscured field offers nothing");
    let read = Rc::new(std::cell::RefCell::new(Vec::new()));
    let other = handle
        .borrow()
        .clone()
        .expect("the headless host provides a primary selection")
        .claim({
            let capture = ReadsOnDrop {
                primary: primary.clone(),
                read: Rc::clone(&read),
            };
            move || {
                let _ = &capture;
                Some("elsewhere".to_owned())
            }
        });

    harness.swap_root(field(&shown, false));
    assert_eq!(
        *read.borrow(),
        [Some("shown".to_owned())],
        "the replaced selection read the replacement's text"
    );
    assert_eq!(primary.text().as_deref(), Some("shown"));
    drop(other);
}

/// A secondary contact through the harness is one consistent stream: its
/// moves hold, and its release lets go of, the button the press pressed.
pub(crate) fn the_harness_releases_the_button_it_pressed() {
    use flui_interaction::events::{PointerButton, PointerButtons, PointerEvent};

    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut harness = crate::common::harness::mount(
        flui_widgets::Listener::new()
            .behavior(flui_widgets::prelude::HitTestBehavior::Opaque)
            .on_pointer_move({
                let seen = Rc::clone(&seen);
                move |_, event| {
                    if let PointerEvent::Move(event) = event.global {
                        seen.borrow_mut().push(format!("move {:?}", event.buttons));
                    }
                }
            })
            .on_pointer_up({
                let seen = Rc::clone(&seen);
                move |_, event| {
                    if let PointerEvent::Up(event) = event.global {
                        seen.borrow_mut().push(format!("up {:?}", event.button()));
                    }
                }
            })
            .child(flui_widgets::SizedBox::expand()),
    );
    harness.tick();
    harness.dispatch_pointer_down_with_button(10.0, 10.0, PointerButton::SECONDARY);
    harness.dispatch_pointer_move(12.0, 10.0);
    harness.dispatch_pointer_up(12.0, 10.0);
    assert_eq!(
        *seen.borrow(),
        [
            format!("move {:?}", PointerButtons::only(PointerButton::SECONDARY)),
            format!("up {:?}", PointerButton::SECONDARY),
        ]
    );
}

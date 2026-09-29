//! [`Form`], [`FormField`] and [`RawTextFormField`] against a mounted tree:
//! validation and its autovalidate modes, save, reset, Tab traversal between
//! fields, the form's semantics, and the clipboard in a form field.
//!
//! Frames are driven with `tick`, which does not dirty the root: an error
//! appears only if the field itself scheduled the rebuild that shows it.
//!
//! Flutter reference: `packages/flutter/test/widgets/form_test.dart`, tag
//! `3.44.0` — the cases here port its validate/save/reset/autovalidate
//! coverage onto FLUI's handles.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::{BoxedView, ViewExt as _};
use flui_widgets::{
    AutovalidateMode, Column, Form, FormFieldHandle, FormHandle, FormHandleAlreadyAttached,
    RawTextFormField, SizedBox, TextEditingController,
};

use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};

fn character(ch: char) -> KeyEvent {
    KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build()
}

fn named(key: NamedKey, modifiers: Modifiers) -> KeyEvent {
    KeyEventBuilder::new(Code::Tab)
        .with_key(Key::Named(key))
        .with_state(KeyState::Down)
        .with_modifiers(modifiers)
        .build()
}

fn type_text(laid: &LaidOut, text: &str) {
    for ch in text.chars() {
        laid.focus_manager().dispatch_key_event(&character(ch));
    }
}

fn required(message: &'static str) -> impl Fn(&String) -> Option<String> {
    move |value: &String| value.is_empty().then(|| message.to_owned())
}

fn at_least_three(value: &str) -> Option<String> {
    (value.chars().count() < 3).then(|| "Too short".to_owned())
}

fn fields(children: Vec<BoxedView>) -> Column {
    Column::new(children)
}

fn mount(form: Form) -> LaidOut {
    lay_out(form, tight(400.0, 300.0))
}

/// [`mount`] below a probe, whose writer source opens the `cx` that
/// `FormHandle::save` and `reset` take, as a submit button's press would.
fn mount_probed(form: Form) -> (LaidOut, SignalProbe) {
    let probe = SignalProbe::new(move |_| form.clone());
    (lay_out(probe.view(), tight(400.0, 300.0)), probe)
}

/// `validate()` shows the validator's message, and a valid value clears it
/// on the next `validate()`.
///
/// Also fails if the error is stored but the field never schedules its
/// rebuild: `tick` would then keep painting the old frame.
#[test]
fn validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it() {
    let form = FormHandle::new();
    let node = FocusNode::with_debug_label("name");
    let mut laid = mount(
        Form::new(
            RawTextFormField::with_initial_value("")
                .validator(required("Required"))
                .focus_node(Rc::clone(&node)),
        )
        .handle(form.clone()),
    );
    assert!(
        laid.find_text("Required").is_none(),
        "no error before validate"
    );

    assert!(!form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "validate shows the error"
    );

    node.request_focus();
    type_text(&laid, "x");
    assert!(form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "a valid value clears it"
    );
}

/// `OnUserInteraction`: nothing at mount; the first edit validates, and so
/// does every later one.
#[test]
fn on_user_interaction_validates_only_after_the_first_edit() {
    let node = FocusNode::with_debug_label("interaction");
    let mut laid = mount(Form::new(
        RawTextFormField::with_initial_value("")
            .validator(|value| at_least_three(value))
            .autovalidate_mode(AutovalidateMode::OnUserInteraction)
            .focus_node(Rc::clone(&node)),
    ));
    assert!(
        laid.find_text("Too short").is_none(),
        "not validated at mount"
    );

    node.request_focus();
    type_text(&laid, "a");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_some(),
        "the first edit validates"
    );

    type_text(&laid, "bc");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_none(),
        "a valid edit clears it"
    );
}

/// `OnUnfocus`: no error while the field has focus; Tab away shows it.
///
/// Also fails without the focus wrapper the field puts around its content:
/// nothing would observe the focus leaving.
#[test]
fn on_unfocus_validates_when_tab_leaves_the_field() {
    let first = FocusNode::with_debug_label("password");
    let second = FocusNode::with_debug_label("next");
    let mut laid = mount(Form::new(fields(vec![
        RawTextFormField::with_initial_value("")
            .validator(required("Required"))
            .autovalidate_mode(AutovalidateMode::OnUnfocus)
            .focus_node(Rc::clone(&first))
            .boxed(),
        RawTextFormField::with_initial_value("ok")
            .focus_node(Rc::clone(&second))
            .boxed(),
    ])));
    first.request_focus();
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "no error while focused"
    );

    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::empty()));
    laid.tick();
    assert!(second.has_primary_focus(), "precondition: Tab moved on");
    assert!(laid.find_text("Required").is_some(), "leaving validates");
}

/// Tab and Shift+Tab walk the fields in order; the focus wrapper each field
/// carries never takes the focus itself.
#[test]
fn tab_and_shift_tab_move_focus_between_form_fields_in_order() {
    let first = FocusNode::with_debug_label("first");
    let second = FocusNode::with_debug_label("second");
    let laid = mount(Form::new(fields(vec![
        RawTextFormField::with_initial_value("")
            .focus_node(Rc::clone(&first))
            .boxed(),
        RawTextFormField::with_initial_value("")
            .focus_node(Rc::clone(&second))
            .boxed(),
    ])));
    let is_primary = |node: &Rc<FocusNode>| {
        laid.focus_manager()
            .primary_focus()
            .is_some_and(|focused| Rc::ptr_eq(&focused, node))
    };

    first.request_focus();
    assert!(is_primary(&first));
    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::empty()));
    assert!(is_primary(&second), "Tab: first -> second");
    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::SHIFT));
    assert!(is_primary(&first), "Shift+Tab: second -> first");
}

/// `save()` hands each field's current value to its `on_saved`, in the
/// order the fields registered.
#[test]
fn save_calls_on_saved_with_each_fields_value_in_registration_order() {
    let form = FormHandle::new();
    let saved: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let second = FocusNode::with_debug_label("second");
    let field = |initial: &str, node: Option<&Rc<FocusNode>>| {
        let sink = Rc::clone(&saved);
        let mut field = RawTextFormField::with_initial_value(initial)
            .on_saved(move |_cx, value| sink.borrow_mut().push(value.clone()));
        if let Some(node) = node {
            field = field.focus_node(Rc::clone(node));
        }
        field.boxed()
    };
    let (laid, probe) = mount_probed(
        Form::new(fields(vec![
            field("a", None),
            field("b", Some(&second)),
            field("c", None),
        ]))
        .handle(form.clone()),
    );
    second.request_focus();
    type_text(&laid, "x");

    probe.write(|cx| form.save(cx)).expect("same presentation");

    assert_eq!(*saved.borrow(), ["a", "bx", "c"]);
}

/// `reset()` puts the text back, clears the error and the interaction; the
/// next edit then validates again under `OnUserInteraction`.
///
/// Also fails if writing the initial text back into the controller counted
/// as the user's edit: the field would stay interacted and show the error.
#[test]
fn reset_restores_initial_values_and_clears_errors_and_interaction() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let controller = TextEditingController::with_text("init");
    let node = FocusNode::with_debug_label("reset");
    let (mut laid, probe) = mount_probed(
        Form::new(
            RawTextFormField::new(controller.clone())
                .validator(required("Required"))
                .autovalidate_mode(AutovalidateMode::OnUserInteraction)
                .focus_node(Rc::clone(&node))
                .handle(field.clone()),
        )
        .handle(form.clone()),
    );
    let backspace = named(NamedKey::Backspace, Modifiers::empty());
    node.request_focus();
    for _ in 0..4 {
        laid.focus_manager().dispatch_key_event(&backspace);
    }
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "precondition: error up"
    );

    probe.write(|cx| form.reset(cx)).expect("same presentation");
    laid.tick();

    assert_eq!(controller.text(), "init");
    assert_eq!(field.value(), "init");
    assert!(laid.find_text("Required").is_none(), "the error is cleared");
    assert!(!field.has_interacted_by_user());
    assert!(!form.has_interacted_by_user());

    for _ in 0..4 {
        laid.focus_manager().dispatch_key_event(&backspace);
    }
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "an edit after the reset validates again"
    );
}

/// A field that leaves the tree leaves the form: it no longer fails its
/// `validate()`.
#[test]
fn a_disposed_field_no_longer_takes_part_in_validate() {
    let form = FormHandle::new();
    let tree = |with_invalid: bool| {
        let first = if with_invalid {
            RawTextFormField::with_initial_value("")
                .validator(required("Required"))
                .boxed()
        } else {
            SizedBox::new(10.0, 10.0).boxed()
        };
        Form::new(fields(vec![
            first,
            RawTextFormField::with_initial_value("ok")
                .validator(required("Required"))
                .boxed(),
        ]))
        .handle(form.clone())
    };
    let mut laid = mount(tree(true));
    assert!(!form.validate(), "precondition: the empty field fails");

    laid.pump_widget(tree(false));

    assert!(form.validate(), "the removed field no longer takes part");
}

/// A mounted field rebuilt with a different handle moves onto it: the new
/// handle reads the field's value and interaction, its error is the one the
/// form's `validate()` shows, and the old handle no longer reaches the field.
///
/// Fails if the new handle is ignored: `value()` panics on a handle whose
/// field never mounted, and `validate()` reports its error to the old one.
#[test]
fn a_new_handle_on_rebuild_takes_the_mounted_field_over() {
    let form = FormHandle::new();
    let first = FormFieldHandle::new();
    let second = FormFieldHandle::new();
    let node = FocusNode::with_debug_label("rehandled");
    let tree = |handle: &FormFieldHandle<String>| {
        Form::new(
            RawTextFormField::with_initial_value("")
                .validator(|value: &String| at_least_three(value))
                .focus_node(Rc::clone(&node))
                .handle(handle.clone()),
        )
        .handle(form.clone())
    };
    let mut laid = mount(tree(&first));
    node.request_focus();
    type_text(&laid, "ab");
    assert_eq!(first.value(), "ab", "precondition");

    laid.pump_widget(tree(&second));

    assert_eq!(second.value(), "ab");
    assert!(second.has_interacted_by_user());
    assert!(!form.validate());
    assert_eq!(second.error_text().as_deref(), Some("Too short"));
    assert_eq!(first.error_text(), None, "the old handle is detached");
    laid.tick();
    assert!(laid.find_text("Too short").is_some(), "the field rebuilt");

    type_text(&laid, "c");
    assert_eq!(second.value(), "abc");
    assert_eq!(first.value(), "ab", "the old handle keeps its last value");
}

/// The form is a semantics node with the form role, and the error line is
/// a live region so an appearing error is announced.
#[test]
fn form_reports_the_form_role_and_the_error_line_is_a_live_region() {
    use flui_testing::a11y::Role;

    let mut laid = mount(Form::new(
        RawTextFormField::with_initial_value("")
            .validator(required("Required"))
            .autovalidate_mode(AutovalidateMode::Always),
    ));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    tree.find(Role::Form)
        .unwrap_or_else(|error| panic!("expected one form node: {error}\n{}", tree.describe()));
    let error = tree
        .find_by_label("Required")
        .unwrap_or_else(|error| panic!("expected the error line: {error}\n{}", tree.describe()));
    assert!(
        error.raw().live().is_some(),
        "the error line is a live region"
    );
}

// ============================================================================
// Event context (ADR-0086): the handle methods take the caller's `cx` and
// hand it to the callbacks they run; a field's edit hands on its own.
// ============================================================================

#[test]
fn a_panicking_reset_callback_does_not_disable_later_form_validation() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let controller = TextEditingController::with_text("valid");
    let (mut laid, probe) = mount_probed(
        Form::new(
            RawTextFormField::new(controller.clone())
                .handle(field.clone())
                .validator(required("Required"))
                .on_reset(|_cx| -> () { panic!("test callback failure") }),
        )
        .handle(form.clone())
        .autovalidate_mode(AutovalidateMode::OnUserInteraction),
    );
    probe
        .write(|cx| field.did_change(cx, String::new()))
        .expect("same presentation");
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "precondition: the edited field shows its error"
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        probe.write(|cx| form.reset(cx)).expect("same presentation");
    }));
    assert!(result.is_err(), "callback panic propagates");
    assert_eq!(controller.text(), "valid", "the controller was reset");
    assert_eq!(field.error_text(), None, "the error state was reset");
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "the committed reset remains dirty when on_reset panics"
    );

    probe
        .write(|cx| field.did_change(cx, String::new()))
        .expect("same presentation");
    assert_eq!(field.error_text().as_deref(), Some("Required"));
}

#[test]
fn a_foreign_presentation_cannot_partially_reset_a_form() {
    use flui_view::SignalWriteExt as _;

    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let mounted_form = form.clone();
    let mounted_field = field.clone();
    let callbacks = Rc::new(Cell::new(0));
    let observed = Rc::clone(&callbacks);
    let owner = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let observed = Rc::clone(&observed);
        Form::new(
            RawTextFormField::with_initial_value("initial")
                .handle(mounted_field.clone())
                .on_reset(move |cx| {
                    observed.set(observed.get() + 1);
                    count.set(cx, 1)
                }),
        )
        .handle(mounted_form.clone())
    });
    let _owner_tree = lay_out(owner.view(), tight(400.0, 300.0));
    owner
        .write(|cx| field.did_change(cx, "edited".to_owned()))
        .expect("same presentation");
    assert!(field.has_interacted_by_user());

    let foreign = SignalProbe::new(|_| SizedBox::shrink());
    let _foreign_tree = lay_out(foreign.view(), tight(100.0, 100.0));
    assert_eq!(
        foreign.write(|cx| form.reset(cx)),
        Err(flui_view::EventContextError::ForeignPresentation)
    );

    assert_eq!(
        field.value(),
        "edited",
        "ownership is checked before mutation"
    );
    assert!(field.has_interacted_by_user());
    assert_eq!(
        foreign.write(|cx| form.save(cx)),
        Err(flui_view::EventContextError::ForeignPresentation)
    );
    assert_eq!(
        foreign.write(|cx| field.reset(cx)),
        Err(flui_view::EventContextError::ForeignPresentation)
    );
    assert_eq!(callbacks.get(), 0, "a rejected reset emits no callbacks");
    assert_eq!(owner.value(), Ok(0));
}

#[test]
fn detached_forms_and_fields_refuse_event_operations() {
    use flui_view::EventContextError;

    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let (mut laid, probe) = mount_probed(
        Form::new(RawTextFormField::with_initial_value("initial").handle(field.clone()))
            .handle(form.clone()),
    );
    probe
        .write(|cx| field.did_change(cx, "edited".to_owned()))
        .expect("mounted owner");
    laid.pump_widget(SizedBox::shrink());

    assert_eq!(
        probe.write(|cx| form.reset(cx)),
        Err(EventContextError::Detached)
    );
    assert_eq!(
        probe.write(|cx| form.save(cx)),
        Err(EventContextError::Detached)
    );
    assert_eq!(
        probe.write(|cx| field.reset(cx)),
        Err(EventContextError::Detached)
    );
    assert_eq!(
        probe.write(|cx| field.did_change(cx, "late".to_owned())),
        Err(EventContextError::Detached)
    );
    assert_eq!(field.value(), "edited");
    assert!(field.has_interacted_by_user());
}

#[test]
fn an_owner_context_opened_during_build_cannot_partially_reset_a_form() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let mounted_form = form.clone();
    let mounted_field = field.clone();
    let source = Rc::new(RefCell::new(None::<SignalProbe>));
    let weak_source = Rc::downgrade(&source);
    let attempted = Rc::new(Cell::new(false));
    let build_attempted = Rc::clone(&attempted);
    let result = Rc::new(RefCell::new(None));
    let observed = Rc::clone(&result);
    let probe = SignalProbe::new(move |_| {
        if build_attempted.get() {
            let source = weak_source.upgrade().expect("test owns source slot");
            let source = source.borrow().clone().expect("probe mounted");
            *observed.borrow_mut() = Some(source.write(|cx| mounted_form.reset(cx)));
        }
        Form::new(RawTextFormField::with_initial_value("initial").handle(mounted_field.clone()))
            .handle(mounted_form.clone())
    });
    let mut laid = lay_out(probe.view(), tight(400.0, 300.0));
    *source.borrow_mut() = Some(probe.clone());
    probe
        .write(|cx| field.did_change(cx, "edited".to_owned()))
        .expect("mounted owner");
    attempted.set(true);
    laid.pump_widget(probe.view());

    assert!(matches!(
        *result.borrow(),
        Some(Err(flui_view::EventContextError::WrittenDuringBuild { .. }))
    ));
    assert_eq!(field.value(), "edited");
    assert!(field.has_interacted_by_user());
}

#[test]
fn replacing_form_and_field_handles_transfers_event_ownership() {
    use flui_view::EventContextError;

    let old_form = FormHandle::new();
    let old_field = FormFieldHandle::new();
    let new_form = FormHandle::new();
    let new_field = FormFieldHandle::new();
    let selected = Rc::new(RefCell::new((old_form.clone(), old_field.clone())));
    let selection = Rc::clone(&selected);
    let probe = SignalProbe::new(move |_| {
        let (form, field) = selection.borrow().clone();
        Form::new(RawTextFormField::with_initial_value("initial").handle(field)).handle(form)
    });
    let mut laid = lay_out(probe.view(), tight(400.0, 300.0));
    probe
        .write(|cx| old_field.did_change(cx, "edited".to_owned()))
        .expect("original owner");
    *selected.borrow_mut() = (new_form.clone(), new_field.clone());
    laid.pump_widget(probe.view());

    assert_eq!(
        probe.write(|cx| old_form.reset(cx)),
        Err(EventContextError::Detached)
    );
    assert_eq!(
        probe.write(|cx| old_field.reset(cx)),
        Err(EventContextError::Detached)
    );
    assert_eq!(new_field.value(), "edited");
    probe
        .write(|cx| new_form.save(cx))
        .expect("new form owns the event target");
    probe
        .write(|cx| new_field.did_change(cx, "new owner".to_owned()))
        .expect("new field owns event target");
    assert_eq!(new_field.value(), "new owner");
}

/// One handle names one mounted form. A second simultaneous mount must fail
/// before its configuration can replace the first form's callback or owner.
///
/// Fails without an attachment lease because `Form::create_state` configures
/// the shared handle and the second `init_state` replaces its writer.
#[test]
fn a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let first_changes = Rc::new(Cell::new(0));
    let observed_first = Rc::clone(&first_changes);
    let mounted_form = form.clone();
    let mounted_field = field.clone();
    let first_probe = SignalProbe::new(move |_| {
        let observed_first = Rc::clone(&observed_first);
        Form::new(RawTextFormField::with_initial_value("first").handle(mounted_field.clone()))
            .handle(mounted_form.clone())
            .on_changed(move |_cx| observed_first.set(observed_first.get() + 1))
    });
    let _first_tree = lay_out(first_probe.view(), tight(400.0, 300.0));

    let duplicate_changes = Rc::new(Cell::new(0));
    let observed_duplicate = Rc::clone(&duplicate_changes);
    let duplicate_form = form.clone();
    let (duplicate_tree, log) = flui_testing::log_capture::capture(|| {
        lay_out(
            Form::new(SizedBox::shrink())
                .handle(duplicate_form)
                .on_changed(move |_cx| {
                    observed_duplicate.set(observed_duplicate.get() + 1);
                }),
            tight(400.0, 300.0),
        )
    });
    assert!(
        log.contains("FormHandle is already attached"),
        "the caller-triggerable refusal is reported without unwinding: {log}"
    );
    assert_eq!(
        form.take_attachment_error(),
        Some(FormHandleAlreadyAttached),
        "the refusal is available as a typed diagnostic"
    );

    first_probe
        .write(|cx| field.did_change(cx, "edited".to_owned()))
        .expect("the original form remains attached");
    assert_eq!(first_changes.get(), 1);
    assert_eq!(
        duplicate_changes.get(),
        0,
        "the refused form cannot replace the original callback"
    );

    drop(duplicate_tree);
    first_probe
        .write(|cx| field.did_change(cx, "still attached".to_owned()))
        .expect("disposing the refused form cannot detach the original form");
    assert_eq!(first_changes.get(), 2);
}

#[test]
fn form_and_field_handles_can_reattach_after_their_owner_unmounts() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    {
        let _first_owner = lay_out(
            Form::new(RawTextFormField::with_initial_value("first owner").handle(field.clone()))
                .handle(form.clone()),
            tight(400.0, 300.0),
        );
    }

    let mounted_form = form.clone();
    let mounted_field = field.clone();
    let second_probe = SignalProbe::new(move |_| {
        Form::new(
            RawTextFormField::with_initial_value("second owner").handle(mounted_field.clone()),
        )
        .handle(mounted_form.clone())
    });
    let (_second_owner, log) =
        flui_testing::log_capture::capture(|| lay_out(second_probe.view(), tight(400.0, 300.0)));

    assert!(
        !log.contains("already attached"),
        "unmount releases both exclusive leases: {log}"
    );
    second_probe
        .write(|cx| field.did_change(cx, "reattached".to_owned()))
        .expect("the field writer belongs to the new owner");
    second_probe
        .write(|cx| form.reset(cx))
        .expect("the form writer belongs to the new owner");
    assert_eq!(field.value(), "second owner");
}

#[test]
fn a_busy_form_handle_rebind_keeps_the_old_owner_until_a_later_update_can_acquire() {
    use flui_view::EventContextError;

    let old_form = FormHandle::new();
    let busy_form = FormHandle::new();
    let first_field = FormFieldHandle::new();
    let busy_field = FormFieldHandle::new();
    let request_busy = Rc::new(Cell::new(false));
    let show_busy_owner = Rc::new(Cell::new(true));

    let mounted_old = old_form.clone();
    let requested = busy_form.clone();
    let mounted_first_field = first_field.clone();
    let mounted_busy = busy_form.clone();
    let mounted_busy_field = busy_field.clone();
    let choose_busy = Rc::clone(&request_busy);
    let include_busy = Rc::clone(&show_busy_owner);
    let probe = SignalProbe::new(move |_| {
        let first_handle = if choose_busy.get() {
            requested.clone()
        } else {
            mounted_old.clone()
        };
        let mut children = vec![
            Form::new(
                RawTextFormField::with_initial_value("first").handle(mounted_first_field.clone()),
            )
            .handle(first_handle)
            .boxed(),
        ];
        if include_busy.get() {
            children.push(
                Form::new(
                    RawTextFormField::with_initial_value("busy").handle(mounted_busy_field.clone()),
                )
                .handle(mounted_busy.clone())
                .boxed(),
            );
        }
        fields(children)
    });
    let mut laid = lay_out(probe.view(), tight(400.0, 300.0));
    probe
        .write(|cx| first_field.did_change(cx, "first edited".to_owned()))
        .expect("old form owns the first field");
    probe
        .write(|cx| busy_field.did_change(cx, "busy edited".to_owned()))
        .expect("busy form owns the second field");

    request_busy.set(true);
    let ((), log) = flui_testing::log_capture::capture(|| laid.pump_widget(probe.view()));
    assert!(log.contains("retaining the current attachment"), "{log}");
    assert_eq!(
        busy_form.take_attachment_error(),
        Some(FormHandleAlreadyAttached)
    );
    probe
        .write(|cx| old_form.reset(cx))
        .expect("failed acquisition keeps the old binding live");
    probe
        .write(|cx| busy_form.reset(cx))
        .expect("failed acquisition leaves the busy owner untouched");
    assert_eq!(first_field.value(), "first");
    assert_eq!(busy_field.value(), "busy");

    show_busy_owner.set(false);
    laid.pump_widget(probe.view());
    laid.pump_widget(probe.view());
    assert_eq!(
        probe.write(|cx| old_form.reset(cx)),
        Err(EventContextError::Detached),
        "the old handle detaches only after the replacement is acquired"
    );
    probe
        .write(|cx| busy_form.reset(cx))
        .expect("a later update retries and acquires the released handle");
}

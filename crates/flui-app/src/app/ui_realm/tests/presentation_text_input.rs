use std::cell::RefCell;

use flui_types::ImeEvent;
use flui_types::geometry::Bounds;

use super::*;

/// A concrete headless recorder and the same value viewed through the
/// platform capability supplied to the presentation.
fn headless_text_input() -> (
    Arc<flui_platform::FakeTextInput>,
    Arc<dyn flui_platform::traits::PlatformTextInput>,
) {
    let fake = Arc::new(flui_platform::FakeTextInput::new());
    let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
    (fake, capability)
}

fn test_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
}

/// Attach records `set_ime_allowed(true)`; preedit/commit events
/// routed through `handle_input_entered` reach the attached client
/// with the exact delivered strings; detach from the still-active
/// token records `set_ime_allowed(false)`.
#[test]
fn attach_dispatch_and_active_detach_round_trip_through_the_platform() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
    let handle = realm.text_input_handle();

    let received = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&received);
    let token = handle
        .attach(Rc::new(move |event: &ImeEvent| {
            sink.borrow_mut().push(event.clone());
        }))
        .expect("headless presentation supports text input");

    assert_eq!(
        fake.last_ime_allowed(),
        Some(true),
        "attach must enable platform IME composition"
    );

    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Ime(ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        }));
        realm.handle_input_entered(PlatformInput::Ime(ImeEvent::Commit("你好".to_string())));
    });

    assert_eq!(
        received.borrow().as_slice(),
        [
            ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: Some((0, 2)),
            },
            ImeEvent::Commit("你好".to_string()),
        ],
        "handle_input_entered must deliver the exact ImeEvent payload to the attached client"
    );

    assert_eq!(
        handle.detach(token).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Detached
    );
    assert_eq!(
        fake.last_ime_allowed(),
        Some(false),
        "detaching the active token must disable platform IME composition"
    );
}

/// The stale-detach race named in `TextInputOwner`'s module doc:
/// field A attaches, field B attaches (replacing A), and A's
/// now-stale detach must record NOTHING on the platform side — only
/// B's later, active-token detach may disable IME.
#[test]
fn a_stale_detach_records_nothing_on_the_platform() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
    let handle = realm.text_input_handle();

    let token_a = handle
        .attach(Rc::new(|_event: &ImeEvent| {}))
        .expect("supported presentation");
    assert_eq!(fake.ime_allowed_calls(), vec![true]);

    let token_b = handle
        .attach(Rc::new(|_event: &ImeEvent| {}))
        .expect("supported presentation");
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true],
        "replacement on one presentation keeps the already-enabled IME session"
    );

    assert_eq!(
        handle.detach(token_a).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Stale
    );
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true],
        "a stale detach (token_a, already replaced by token_b) records nothing"
    );

    assert_eq!(
        handle.detach(token_b).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Detached
    );
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true, false],
        "the active token's detach still disables IME"
    );
}

/// End-to-end proof of a claim `flui-widgets`' own
/// `editable_text::tests` cannot make on their own: a real
/// mounted `EditableText` receives the weak handle of this realm's
/// directly owned platform capability, not a mock or a stand-in.
#[test]
fn a_mounted_editable_text_toggles_platform_ime_on_focus_and_blur() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

    let controller = flui_widgets::TextEditingController::new();
    let focus_node = flui_interaction::FocusNode::with_debug_label("app-ime-integration");
    realm
        .enter(|realm| {
            realm.attach_root_widget(&flui_widgets::EditableText::new(
                controller.clone(),
                Rc::clone(&focus_node),
            ))
        })
        .expect("attach succeeds");
    let _ = realm.draw_frame(test_constraints());

    assert!(focus_node.is_attached());
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus());
    assert_eq!(
        fake.last_ime_allowed(),
        Some(true),
        "focusing a mounted EditableText must attach through its presentation \
         and enable platform IME composition"
    );

    focus_node.unfocus();
    assert_eq!(
        fake.last_ime_allowed(),
        Some(false),
        "blurring the field must detach and disable platform IME composition"
    );
}

/// `TextInputHandle::set_cursor_area` reaches this realm's exact
/// `PlatformTextInput` capability through the same `PresentationState`
/// ownership path used in production. Deliberately does not mount a
/// widget tree: this test proves the owner forwards an
/// already-computed `Bounds` to the platform, not that a real
/// `EditableText` computes the right one.
#[test]
fn set_ime_cursor_area_reaches_the_presentations_platform_capability() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

    let area = Bounds::new(
        flui_types::Point::new(px(10.0), px(20.0)),
        flui_types::Size::new(px(2.0), px(18.0)),
    );
    realm
        .text_input_handle()
        .set_cursor_area(area)
        .expect("headless presentation supports text input");

    assert_eq!(
        fake.cursor_area_calls(),
        vec![area],
        "set_ime_cursor_area must call through to the presentation-owned \
         PlatformTextInput capability with the exact area"
    );
}

/// A presentation without IME support reports a typed error.
#[test]
fn set_ime_cursor_area_without_platform_support_is_typed() {
    let realm = UiRealm::for_test();
    assert_eq!(
        realm.text_input_handle().set_cursor_area(Bounds::new(
            flui_types::Point::new(px(0.0), px(0.0)),
            flui_types::Size::new(px(1.0), px(1.0)),
        )),
        Err(flui_interaction::TextInputError::Unsupported)
    );
}

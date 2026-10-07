//! [`EditableText`] against a mounted tree: focus and enablement, submit
//! keys, the IME session and its cursor-area loop, pointer selection, the
//! obscured-text mapping as the render object sees it, and composing-region
//! paint. The key handler, the mask and the render-view assembly stay unit
//! tests in `src/text/editable_text.rs`.

use std::rc::Rc;

use flui_interaction::events::{Key, KeyState};
use flui_interaction::routing::FocusNode;
use flui_objects::RenderEditable;
use flui_widgets::{EditableText, TextEditingController};

/// Platform requests travel through the real realm inbox and the mounted
/// EditableText producer; no callback or controller setter stands in for them.
pub(crate) mod native_actions {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use flui_interaction::routing::FocusNode;
    use flui_rendering::pipeline::PipelineCell;
    use flui_testing::a11y::Role;
    use flui_testing::{
        A11yTree, Action, ActionData, ActionRequest, HeadlessHost, HeadlessWindow, NodeId, TreeId,
    };
    use flui_view::prelude::*;
    use flui_widgets::{EditableText, SizedBox, TextEditingController};

    use crate::common::SignalProbe;

    // The host acquires only the published-tree inspection capability, at
    // the same lifecycle boundary a consumer acquires frame capabilities.
    #[derive(Clone, StatefulView)]
    struct FieldHost {
        pipeline: Rc<RefCell<Option<PipelineCell>>>,
        child: BoxedView,
    }

    struct FieldHostState {
        pipeline: Rc<RefCell<Option<PipelineCell>>>,
    }

    impl std::fmt::Debug for FieldHostState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FieldHostState").finish()
        }
    }

    impl StatefulView for FieldHost {
        type State = FieldHostState;

        fn create_state(&self) -> Self::State {
            FieldHostState {
                pipeline: Rc::clone(&self.pipeline),
            }
        }
    }

    impl ViewState<FieldHost> for FieldHostState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            *self.pipeline.borrow_mut() = ctx.pipeline_owner();
        }

        fn build(&self, view: &FieldHost, _ctx: &dyn BuildContext) -> impl IntoView {
            view.child.clone()
        }
    }

    struct Fixture {
        realm: HeadlessHost,
        probe: SignalProbe,
        controller: Rc<RefCell<TextEditingController>>,
        node: Rc<RefCell<Rc<FocusNode>>>,
        enabled: Rc<Cell<bool>>,
        shown: Rc<Cell<bool>>,
        changed: Rc<RefCell<Vec<String>>>,
        pipeline: Rc<RefCell<Option<PipelineCell>>>,
        revision: Rc<RefCell<Option<Signal<u32>>>>,
    }

    impl Fixture {
        fn new() -> Self {
            let controller = Rc::new(RefCell::new(TextEditingController::with_text("abc")));
            let node = Rc::new(RefCell::new(FocusNode::with_debug_label("native field")));
            let enabled = Rc::new(Cell::new(true));
            let shown = Rc::new(Cell::new(true));
            let changed = Rc::new(RefCell::new(Vec::new()));
            let pipeline = Rc::new(RefCell::new(None));
            let revision = Rc::new(RefCell::new(None));
            let revision_sink = Rc::clone(&revision);
            let (document, focus, active, visible, changes, capture) = (
                Rc::clone(&controller),
                Rc::clone(&node),
                Rc::clone(&enabled),
                Rc::clone(&shown),
                Rc::clone(&changed),
                Rc::clone(&pipeline),
            );
            let probe = SignalProbe::new(move |signals| {
                *revision_sink.borrow_mut() = Some(signals.count);
                let child = if visible.get() {
                    let changes = Rc::clone(&changes);
                    EditableText::new(document.borrow().clone(), Rc::clone(&focus.borrow()))
                        .enabled(active.get())
                        .on_changed(move |cx, text| {
                            changes.borrow_mut().push(text.to_owned());
                            signals.count.update(cx, |n| *n += 1)
                        })
                        .boxed()
                } else {
                    SizedBox::new(1.0, 1.0).boxed()
                };
                FieldHost {
                    pipeline: Rc::clone(&capture),
                    child,
                }
            });
            let mut realm = HeadlessHost::new(HeadlessWindow::new(400, 100).with_text_input());
            realm.attach(&probe.view()).expect("fresh realm");
            realm.enable_semantics();
            let _ = realm.pump(Duration::ZERO);
            Self {
                realm,
                probe,
                controller,
                node,
                enabled,
                shown,
                changed,
                pipeline,
                revision,
            }
        }

        fn pump(&mut self) {
            let _ = self.realm.pump(Duration::ZERO);
        }

        fn rebuild(&mut self) {
            self.probe
                .write(|cx| {
                    self.revision
                        .borrow()
                        .expect("mounted signal")
                        .update(cx, |n| *n += 1)
                })
                .expect("live rebuild signal");
            self.pump();
        }

        fn tree(&self) -> A11yTree {
            let pipeline = self.pipeline.borrow().clone().expect("lifecycle pipeline");
            A11yTree::new(pipeline.with(|owner| {
                owner
                    .semantics_owner()
                    .and_then(|owner| owner.to_accesskit_tree_update(None))
                    .expect("published semantics")
            }))
        }

        fn id(&self) -> NodeId {
            self.tree().find(Role::TextInput).expect("sole field").id()
        }

        fn request(&self, action: Action, id: NodeId, data: Option<ActionData>) {
            self.realm
                .accessibility_action_listener()
                .expect("platform listener")(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: id,
                data,
            });
        }

        fn set_text(&mut self, text: &str) {
            self.request(
                Action::SetValue,
                self.id(),
                Some(ActionData::Value(text.into())),
            );
            self.pump();
        }
    }

    pub(crate) fn queued_focus_and_text_reach_the_current_field_and_event_context() {
        let mut fixture = Fixture::new();
        let tree = fixture.tree();
        let field = tree.find(Role::TextInput).expect("sole field");
        assert!(field.supports_action(Action::Focus));
        assert!(field.supports_action(Action::SetValue));
        assert!(!fixture.node.borrow().has_primary_focus());
        fixture.set_text("unfocused");
        assert_eq!(fixture.controller.borrow().text(), "unfocused");
        assert!(
            !fixture.node.borrow().has_primary_focus(),
            "SetValue does not move focus"
        );
        fixture.request(Action::Focus, field.id(), None);
        assert!(
            !fixture.node.borrow().has_primary_focus(),
            "platform request is queued"
        );
        fixture.pump();
        assert!(fixture.node.borrow().has_primary_focus());
        assert_eq!(
            fixture
                .realm
                .window()
                .ime_allowed_calls()
                .and_then(|calls| calls.last().copied()),
            Some(true)
        );
        assert_eq!(
            fixture.tree().focus().map(|node| node.id()),
            Some(fixture.id())
        );
        // A real IME commit reaches the session attached by semantic focus.
        fixture
            .realm
            .dispatch(flui_platform_api::PlatformInput::Ime(
                flui_platform_api::ImeEvent::Commit("😀".into()),
            ));
        assert_eq!(fixture.controller.borrow().text(), "unfocused😀");
        fixture.pump();
        fixture.changed.borrow_mut().clear();
        let before = fixture.probe.value().expect("live callback signal");
        for (index, text) in ["a😀e\u{301}", "", "next"].into_iter().enumerate() {
            fixture.set_text(text);
            assert_eq!(fixture.controller.borrow().text(), text);
            assert_eq!(
                fixture.tree().find(Role::TextInput).expect("field").value(),
                Some(text)
            );
            assert_eq!(
                fixture.probe.value(),
                Ok(before + u32::try_from(index + 1).expect("small table"))
            );
        }
        assert_eq!(*fixture.changed.borrow(), ["a😀e\u{301}", "", "next"]);
        fixture.set_text("next");
        assert_eq!(
            fixture.changed.borrow().len(),
            3,
            "unchanged value reports no edit"
        );
    }

    pub(crate) fn native_actions_follow_the_replacement_controller_and_focus_node() {
        let mut fixture = Fixture::new();
        let id = fixture.id();
        let old = fixture.controller.borrow().clone();
        let old_node = Rc::clone(&fixture.node.borrow());
        let replacement = TextEditingController::with_text("replacement");
        let replacement_node = FocusNode::with_debug_label("replacement native field");
        *fixture.controller.borrow_mut() = replacement.clone();
        *fixture.node.borrow_mut() = Rc::clone(&replacement_node);
        fixture.rebuild();
        assert_eq!(fixture.id(), id, "a live field retains semantic identity");
        fixture.request(Action::Focus, id, None);
        fixture.request(
            Action::SetValue,
            id,
            Some(ActionData::Value("current😀".into())),
        );
        fixture.pump();
        assert!(replacement_node.has_primary_focus());
        assert!(!old_node.is_attached());
        assert_eq!(
            old.text(),
            "abc",
            "retired controller never receives the edit"
        );
        assert_eq!(replacement.text(), "current😀");
        assert_eq!(*fixture.changed.borrow(), ["current😀"]);
        assert_eq!(
            fixture.tree().find(Role::TextInput).expect("field").value(),
            Some("current😀")
        );
        let before = fixture.probe.value();
        fixture.request(Action::SetValue, id, None);
        fixture.pump();
        assert_eq!(
            replacement.text(),
            "current😀",
            "missing data does not erase the field"
        );
        assert_eq!(fixture.probe.value(), before);
        fixture.set_text("healthy");
        assert_eq!(replacement.text(), "healthy");
    }

    pub(crate) fn disabled_unmounted_and_closed_fields_refuse_native_actions() {
        let mut fixture = Fixture::new();
        let id = fixture.id();
        let controller = fixture.controller.borrow().clone();
        fixture.enabled.set(false);
        fixture.rebuild();
        let before = fixture.probe.value();
        fixture.request(Action::Focus, id, None);
        fixture.request(
            Action::SetValue,
            id,
            Some(ActionData::Value("disabled".into())),
        );
        fixture.pump();
        assert_eq!(controller.text(), "abc");
        assert!(!fixture.node.borrow().has_primary_focus());
        assert_eq!(fixture.probe.value(), before);

        fixture.enabled.set(true);
        fixture.rebuild();
        fixture.request(Action::Focus, id, None);
        fixture.set_text("enabled");
        assert!(fixture.node.borrow().has_primary_focus());
        assert_eq!(controller.text(), "enabled");

        // The public node can become unfocusable independently of a rebuild.
        fixture.node.borrow().set_can_request_focus(false);
        let before = fixture.probe.value().expect("live edit signal");
        fixture.request(Action::Focus, id, None);
        fixture.set_text("unfocusable");
        assert_eq!(
            controller.text(),
            "unfocusable",
            "focus eligibility does not make an enabled document read-only"
        );
        assert_eq!(fixture.probe.value(), Ok(before + 1));
        assert_eq!(
            fixture.changed.borrow().last().map(String::as_str),
            Some("unfocusable")
        );
        assert!(
            !fixture.node.borrow().has_primary_focus(),
            "SetValue does not override focus eligibility"
        );
        // The callback signal rebuilt the parent, whose enabled view restores
        // its node's focus eligibility. Refresh only the field now: a public
        // selection notification rebuilds AnimatedBuilder without changing
        // text, reporting on_changed, or rebuilding the probe parent.
        fixture.node.borrow().set_can_request_focus(false);
        controller.set_caret_byte_offset(0);
        fixture.pump();
        assert_eq!(fixture.probe.value(), Ok(before + 1));
        assert!(!fixture.node.borrow().can_request_focus());
        let tree = fixture.tree();
        let field = tree.find(Role::TextInput).expect("live editable field");
        assert!(field.supports_action(Action::SetValue));
        assert!(
            !field.supports_action(Action::Focus),
            "the rebuilt field does not advertise ineligible focus"
        );
        fixture.node.borrow().set_can_request_focus(true);
        fixture.set_text("recovered");
        assert_eq!(controller.text(), "recovered");
        fixture.request(Action::Focus, fixture.id(), None);
        fixture.pump();
        assert!(fixture.node.borrow().has_primary_focus());

        fixture.shown.set(false);
        fixture.rebuild();
        assert!(fixture.tree().find_all(Role::TextInput).is_empty());
        fixture.request(Action::Focus, id, None);
        fixture.request(
            Action::SetValue,
            id,
            Some(ActionData::Value("unmounted".into())),
        );
        fixture.pump();
        assert_eq!(controller.text(), "recovered");
        fixture.shown.set(true);
        fixture.rebuild();
        let current = fixture.id();
        assert_ne!(current, id, "remount mints a new generational identity");
        fixture.request(
            Action::SetValue,
            id,
            Some(ActionData::Value("stale".into())),
        );
        fixture.set_text("remounted");
        assert_eq!(controller.text(), "remounted");
        let listener = fixture
            .realm
            .accessibility_action_listener()
            .expect("platform listener");
        drop(fixture);
        listener(ActionRequest {
            action: Action::SetValue,
            target_tree: TreeId::ROOT,
            target_node: current,
            data: Some(ActionData::Value("closed".into())),
        });
        let mut independent = Fixture::new();
        independent.set_text("independent");
        assert_eq!(
            controller.text(),
            "remounted",
            "closed target never receives work"
        );
        assert_eq!(independent.controller.borrow().text(), "independent");
    }

    pub(crate) fn a_deferred_focus_change_preserves_the_semantic_edit_that_follows_it() {
        use flui_platform_api::text_store::{LockOutcome, project_ime_event};
        let controller = TextEditingController::new();
        let node = FocusNode::with_debug_label("deferred semantic edit");
        let make_unfocusable = Rc::clone(&node);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let reported = Rc::clone(&changes);
        let mut harness = crate::common::harness::mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, text| {
                reported.borrow_mut().push(text.to_owned());
                if text == "IME" {
                    make_unfocusable.set_can_request_focus(false);
                }
            }),
        );
        harness.enable_semantics();
        harness.tick();
        let id = harness
            .a11y_tree()
            .expect("semantics")
            .find(Role::TextInput)
            .expect("field")
            .id();
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            })
            .expect("semantic focus");
        assert_eq!(harness.active_ime_clients(), 1);
        let store = harness.active_text_store().expect("focused store");
        harness
            .local_post_frame_handle()
            .schedule_local(move |_| {
                assert_eq!(
                    project_ime_event(&*store, &flui_platform_api::ImeEvent::Commit("IME".into())),
                    Ok(LockOutcome::Deferred)
                );
                panic!("leave the accepted grant queued before its commit anchor");
            })
            .expect("post-frame lane");
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
        assert!(failed.is_err());
        assert_eq!(controller.text(), "");
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: Some(ActionData::Value("late".into())),
            })
            .expect("advertised setter");
        assert_eq!(
            controller.text(),
            "late",
            "the deferred callback changes focus eligibility, not document mutability"
        );
        assert!(!node.has_primary_focus());
        assert_eq!(
            *changes.borrow(),
            ["IME", "late"],
            "accepted grant is reported before the semantic edit"
        );
        node.set_can_request_focus(true);
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: Some(ActionData::Value("healthy".into())),
            })
            .expect("next setter");
        assert_eq!(controller.text(), "healthy");
        assert_eq!(*changes.borrow(), ["IME", "late", "healthy"]);
    }

    pub(crate) fn re_adoption_before_queued_delivery_retires_the_old_field_authority() {
        let mut fixture = Fixture::new();
        let id = fixture.id();
        let old_controller = fixture.controller.borrow().clone();
        let node = Rc::clone(&fixture.node.borrow());
        let parent = node.parent().expect("field attached to its presentation");
        fixture.request(Action::Focus, id, None);
        fixture.request(
            Action::SetValue,
            id,
            Some(ActionData::Value("stale owner".into())),
        );
        let adopted = parent
            .adopt_node(&node)
            .expect("same-parent public takeover");
        assert!(node.is_attached() && node.can_request_focus());
        assert!(adopted.is_attached(), "new owner has a current attachment");
        fixture.pump();
        assert!(
            !node.has_primary_focus(),
            "old field cannot focus a node it no longer owns"
        );
        assert_eq!(old_controller.text(), "abc");
        assert!(
            fixture.changed.borrow().is_empty(),
            "refused edit has no callback"
        );

        fixture.shown.set(false);
        fixture.rebuild();
        assert!(
            adopted.is_attached(),
            "retired field does not detach the later owner"
        );
        let _ = adopted.detach();
        assert!(!node.is_attached());
        let current = TextEditingController::with_text("current");
        *fixture.controller.borrow_mut() = current.clone();
        *fixture.node.borrow_mut() = FocusNode::with_debug_label("current attachment owner");
        fixture.shown.set(true);
        fixture.rebuild();
        fixture.request(Action::Focus, fixture.id(), None);
        fixture.set_text("healthy current");
        assert!(fixture.node.borrow().has_primary_focus());
        assert_eq!(current.text(), "healthy current");
        assert_eq!(old_controller.text(), "abc");
        let mut independent = Fixture::new();
        independent.set_text("independent");
        assert_eq!(independent.controller.borrow().text(), "independent");
    }

    pub(crate) fn re_adoption_during_a_deferred_grant_refuses_the_resumed_semantic_edit() {
        use flui_platform_api::text_store::{LockOutcome, project_ime_event};
        let controller = TextEditingController::new();
        let node = FocusNode::with_debug_label("grant takeover");
        let takeover_node = Rc::clone(&node);
        let authority = Rc::new(RefCell::new(None));
        let issued = Rc::clone(&authority);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let reported = Rc::clone(&changes);
        let mut harness = crate::common::harness::mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, text| {
                reported.borrow_mut().push(text.to_owned());
                if text == "IME" {
                    let parent = takeover_node.parent().expect("live parent during grant");
                    let attachment = parent
                        .adopt_node(&takeover_node)
                        .expect("grant callback takeover");
                    let previous = issued.replace(Some(attachment));
                    drop(previous);
                }
            }),
        );
        harness.enable_semantics();
        harness.tick();
        let id = harness
            .a11y_tree()
            .expect("semantics")
            .find(Role::TextInput)
            .expect("field")
            .id();
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            })
            .expect("initial field authority");
        let store = harness
            .active_text_store()
            .expect("semantic focus attached an IME session");
        harness
            .local_post_frame_handle()
            .schedule_local(move |_| {
                assert_eq!(
                    project_ime_event(&*store, &flui_platform_api::ImeEvent::Commit("IME".into())),
                    Ok(LockOutcome::Deferred)
                );
                panic!("leave an accepted grant queued before its commit anchor");
            })
            .expect("post-frame lane");
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
        assert!(failed.is_err());
        assert_eq!(controller.text(), "");
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: Some(ActionData::Value("late".into())),
            })
            .expect("setter still advertised before delivery");
        assert!(
            node.is_attached() && node.can_request_focus(),
            "takeover preserves node eligibility"
        );
        assert!(
            authority
                .borrow()
                .as_ref()
                .is_some_and(flui_interaction::FocusAttachment::is_attached)
        );
        assert_eq!(
            controller.text(),
            "IME",
            "resumed setter cannot cross attachment takeover"
        );
        assert_eq!(*changes.borrow(), ["IME"]);
        harness.focus_manager().unfocus();
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            })
            .expect("old field still advertises focus");
        assert!(
            !node.has_primary_focus(),
            "lost attachment refuses subsequent focus too"
        );

        harness.swap_root(SizedBox::new(1.0, 1.0));
        let adopted = authority.borrow_mut().take().expect("later owner token");
        assert!(
            adopted.is_attached(),
            "old field disposal preserves later owner"
        );
        let _ = adopted.detach();
        let current = TextEditingController::new();
        let current_node = FocusNode::with_debug_label("healthy after grant takeover");
        harness.swap_root(EditableText::new(current.clone(), Rc::clone(&current_node)));
        let current_id = harness
            .a11y_tree()
            .expect("semantics")
            .find(Role::TextInput)
            .expect("new field")
            .id();
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: current_id,
                data: None,
            })
            .expect("new field focus");
        harness
            .invoke_semantics_action(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: current_id,
                data: Some(ActionData::Value("healthy".into())),
            })
            .expect("new field setter");
        assert!(current_node.has_primary_focus());
        assert_eq!(harness.active_ime_clients(), 1);
        assert_eq!(current.text(), "healthy");
        assert_eq!(
            controller.text(),
            "IME",
            "new field never edits the old document"
        );
    }
}

// ------------------------------------------------------------------
// IME integration
//
// `mount_with_ime` installs a `TextInputHandle` tied to a harness-owned
// `TextInputOwner`. Application tests separately cover a real
// presentation-owned platform capability. These tests dispatch through
// the SAME owner the field attaches to, matching production routing after
// the platform event has been demultiplexed to its presentation.
// ------------------------------------------------------------------

fn dispatch_ime(harness: &crate::common::harness::Harness, event: &flui_platform_api::ImeEvent) {
    harness.dispatch_ime(event);
}

fn character_key_event(ch: char) -> flui_interaction::events::KeyEvent {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build()
}

/// A normal post-mount focus edge attaches one IME client and routes
/// composition to this field's controller.
pub(crate) fn focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME focus gain");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    focus_node.request_focus();
    assert_eq!(
        harness.active_ime_clients(),
        1,
        "focus gain must attach an IME client"
    );

    dispatch_ime(
        &harness,
        &flui_platform_api::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        },
    );

    assert_eq!(controller.text(), "ni");
    assert_eq!(controller.composing_range(), Some(0..2));
}

// ------------------------------------------------------------------
// IME cursor-area tracking (ADR-0030)
//
// `CursorAreaLoop`'s `LocalPostFrameHandle::schedule_local` call
// addresses the harness's lane directly (a `Weak` pointer, minted once
// by `install_build_capabilities`) — it does not need `enter_owner_scope`
// active to succeed, only the lane and its scheduler to still be alive.
// These tests still wrap focusing/blurring in `harness.
// enter_owner_scope(...)` for parity with production's `realm.enter`
// shape, but that wrapping is no longer load-bearing for the loop
// itself; a focus change dispatched outside it starts the loop exactly
// the same way. A focus change with the harness's binding already
// dropped would still attach/detach the IME client correctly (that part
// needs no lane at all), it would just never start the loop — the
// `LocalPostFrameScheduleError::LaneClosed` path `CursorAreaLoop::schedule`
// warns on rather than panicking over.
//
// Transient-`None` resilience (a fully in-place red-check for "skip
// the send, keep the loop alive" — one of `CursorAreaLoop::fire`'s
// two branches) is not constructed here: forcing `global_caret_rect`
// to observe the inner anchor mid-unmount deterministically would
// require reaching into the pipeline mid-rebuild, which this
// harness has no cheap hook for. The branch itself is exercised
// structurally by every test below during the ordinary frame in which
// the tree is *not* yet built (`mount_with_ime`'s own initial
// attach), and its shape (`if let Some(rect) = ... { send } ;
// self.schedule()` — the reschedule is unconditional, not gated on
// the `Some` arm) is the same one line the `loop_stops_sending_after_*`
// tests below would fail to distinguish from a real stop if it were
// wrong.
// ------------------------------------------------------------------

// ------------------------------------------------------------------
// Composing-region underline + hidden caret (ADR-0030)
// ------------------------------------------------------------------

/// Runs `f` against the mounted field's single `RenderEditable`, found
/// by downcasting the one render object this widget mounts.
fn with_render_editable<T>(
    harness: &crate::common::harness::Harness,
    f: impl FnOnce(&RenderEditable) -> T,
) -> Option<T> {
    let owner = harness.pipeline_owner();
    owner.with(|owner| {
        let tree = owner.render_tree();
        let mut f = Some(f);
        for (_, node) in tree.iter() {
            let editable = node
                .as_box()
                .and_then(|b| b.render_object().downcast_ref::<RenderEditable>()); // test-only reach to the one concrete render object type this widget mounts, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — same sanctioned boundary as `CursorAreaLoop::global_caret_rect` above.
            if let Some(editable) = editable {
                return f.take().map(|f| f(editable));
            }
        }
        None
    })
}

/// An obscured field's real characters never reach the render object.
///
/// This is the criterion — "obscured text never leaks through paint,
/// semantics, or diagnostics" — asserted where it is decidable. The
/// substitution happens at the one point the controller's text becomes
/// the render view's, so `RenderEditable::plain_text` is downstream of it
/// and so is everything below: the `TextPainter`, the layer tree, and
/// every diagnostic that renders the tree. Redacting at each of those
/// instead would leave the next one to be remembered.
///
/// The assertion is on the ABSENCE of the plaintext, not merely on the
/// presence of bullets: a mask built beside a still-forwarded original
/// would satisfy the second and fail this.
pub(crate) fn an_obscured_field_never_hands_its_real_text_to_the_render_object() {
    let controller = TextEditingController::with_text("hunter2");
    let focus_node = FocusNode::with_debug_label("obscured field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller, Rc::clone(&focus_node)).obscure_text(true),
    );

    let painted = with_render_editable(&harness, |editable| editable.plain_text().to_string())
        .expect("a mounted EditableText always has a RenderEditable");

    assert_eq!(
        painted, "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}",
        "seven source characters must reach the render object as seven \
             bullets, got {painted:?}"
    );
    assert!(
        !painted.contains("hunter") && !painted.contains('h') && !painted.contains('2'),
        "no fragment of the plaintext may reach the render object, got \
             {painted:?}"
    );
}

/// A tap places the caret where it landed.
///
/// The x is chosen from the field's own geometry rather than guessed: the
/// caret rect after the tap is compared against the caret rect the same
/// offset produces when set programmatically, so the assertion does not
/// depend on this host's font metrics.
///
/// Red-check: drop `controller.set_caret_byte_offset(offset)` from the
/// pointer-down handler — the caret stays at the end, where
/// `with_text` left it.
pub(crate) fn a_tap_places_the_caret_where_it_landed() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("tapped field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        11,
        "precondition: the caret starts at the end"
    );

    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.caret_byte_offset(),
        0,
        "a tap at the left edge belongs before the first character"
    );
    assert!(!controller.has_selection(), "a tap collapses");
}

/// A pointer-down on a composing field and a paste into one commit the
/// composition before they act, keeping its text, through the window's
/// input-method host (ADR-0142 item 4): the caret lands where the tap did,
/// and the clipboard's text lands after the committed text.
///
/// Red-checks: drop the commit from the pointer-down handler (the tap moves
/// the caret inside a composition that is still open), or from the paste
/// action (a paste is refused while composing).
pub(crate) fn a_pointer_down_and_a_paste_commit_the_composition_first() {
    use flui_platform_api::Clipboard as _;
    use flui_testing::StoreHostCall;

    fn composing(text: &str) -> (crate::common::harness::Harness, TextEditingController) {
        let controller = TextEditingController::with_text(text);
        let node = FocusNode::with_debug_label("composing field");
        let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
            controller.clone(),
            Rc::clone(&node),
        ));
        node.request_focus();
        harness.tick();
        harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
            text: "東京".to_owned(),
            cursor: Some(("東京".len(), "東京".len())),
        });
        assert!(controller.is_composing(), "precondition: composing");
        (harness, controller)
    }
    let committed = |harness: &crate::common::harness::Harness| {
        harness
            .store_host_calls()
            .contains(&StoreHostCall::CompleteComposition)
    };

    let (harness, controller) = composing("ab");
    harness.dispatch_pointer_down(1.0, 5.0);
    assert!(committed(&harness), "pointer: the host is asked first");
    assert!(
        !controller.is_composing(),
        "pointer: the composition is committed"
    );
    assert_eq!(controller.text(), "ab東京", "pointer: keeping its text");
    assert_eq!(
        controller.caret_byte_offset(),
        0,
        "pointer: then the caret moves"
    );

    let (harness, controller) = composing("ab");
    harness.clipboard().write_text("!".to_owned());
    assert!(
        harness.focus_manager().dispatch_key_event(
            &flui_interaction::testing::input::KeyEventBuilder::new(
                flui_interaction::events::Code::KeyV
            )
            .with_key(Key::Character("v".to_owned()))
            .with_state(KeyState::Down)
            .with_modifiers(if cfg!(any(target_os = "macos", target_os = "ios")) {
                flui_interaction::events::Modifiers::META
            } else {
                flui_interaction::events::Modifiers::CONTROL
            })
            .build()
        ),
        "paste: the chord is consumed while composing"
    );
    assert!(committed(&harness), "paste: the host is asked first");
    assert!(
        !controller.is_composing(),
        "paste: the composition is committed"
    );
    assert_eq!(
        controller.text(),
        "ab東京!",
        "paste: lands after the committed text"
    );
}

/// A press reentered from the commit it runs (the commit's `on_changed`
/// dispatching another press) finds the outer press's contact already
/// recorded: the nested one is refused and moves nothing, and the outer one
/// places the caret.
///
/// Red-check: record the contact after the commit (the nested press is
/// admitted as a second contact and moves the caret to where it landed).
pub(crate) fn a_press_reentered_by_its_commit_is_not_a_second_contact() {
    use std::cell::{Cell, RefCell};
    type Slot = Rc<RefCell<Option<&'static crate::common::harness::Harness>>>;
    let controller = TextEditingController::with_text("ab");
    let node = FocusNode::with_debug_label("reentered press");
    let slot: Slot = Rc::default();
    let seen = Rc::new(Cell::new(None));
    let (nested, observed, read) = (Rc::clone(&slot), Rc::clone(&seen), controller.clone());
    let mut harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, _text| {
            let harness = nested.borrow_mut().take();
            if let Some(harness) = harness {
                harness.dispatch_pointer_down(400.0, 5.0);
                observed.set(Some(read.caret_byte_offset()));
            }
        }),
    );
    node.request_focus();
    harness.tick();
    harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
        text: "東京".to_owned(),
        cursor: Some(("東京".len(), "東京".len())),
    });
    assert!(controller.is_composing(), "precondition: composing");
    let harness: &'static _ = Box::leak(Box::new(harness));
    slot.replace(Some(harness));

    harness.dispatch_pointer_down(1.0, 5.0);
    let committed = "ab東京".len();
    assert_eq!(
        seen.get(),
        Some(committed),
        "the nested press moved nothing (the commit left the caret after the text)"
    );
    assert_eq!(
        controller.caret_byte_offset(),
        0,
        "the outer press placed the caret where it landed"
    );
}

/// A drag selects from where it started to where the pointer is, and the
/// caret follows the pointer rather than the lower end.
///
/// Red-check: drop the `set_selection(from, to)` in the pointer-move
/// handler — the selection stays collapsed at the down position.
pub(crate) fn a_drag_selects_from_its_start_to_the_pointer() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("dragged field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.dispatch_pointer_down(1.0, 5.0);
    let from = controller.caret_byte_offset();
    harness.dispatch_pointer_move(400.0, 5.0);

    let selection = controller.selection();
    assert_eq!(
        selection.start, from,
        "the anchor stays where the drag began"
    );
    assert!(
        selection.end > from,
        "dragging right must extend the selection, got {selection:?}"
    );
    assert_eq!(
        controller.caret_byte_offset(),
        selection.end,
        "the caret follows the pointer, not the lower end"
    );
}

/// A double-tap selects the whole word under it — the composition
/// `EditableTextState::wrap_double_tap_word_select` adds around
/// `install_pointer_handlers`'s plain tap-places-caret behavior. The
/// first tap alone still just collapses (`Listener` never waits for
/// the arena); the second tap's own DOWN then widens that caret into
/// the enclosing word.
///
/// Red-check: skip wrapping `install_pointer_handlers`'s return value
/// in `wrap_double_tap_word_select` — the selection stays collapsed
/// after the second tap, same as the first.
pub(crate) fn a_double_tap_selects_the_word_under_it() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("double-tapped field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    // First tap: places a collapsed caret, same as
    // `a_tap_places_the_caret_where_it_landed`.
    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_up(1.0, 5.0);
    assert!(
        !controller.has_selection(),
        "the first tap alone only collapses"
    );

    // Second tap, same spot: `on_double_tap_down` widens it to the word.
    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        0..5,
        "a double-tap at the start of \"hello\" selects the whole word"
    );
}

/// The arrow keys stop on the grapheme boundaries the painter clusters text
/// by and snaps a tap to (`flui_painting::text_boundaries`, ICU4X): right
/// arrow from the start stops at each cluster's end, left arrow from the
/// end at each cluster's start, and one Backspace removes one cluster. The
/// texts hold a ZWJ family, two regional-indicator flags, stacked combining
/// marks, a CR LF and the Devanagari conjunct "क्षि", one cluster since
/// UAX #29's conjunct rule (GB9c). Fails if the editor clusters by another
/// segmenter that splits any of them differently: then a caret can stop
/// where a tap never lands, or a Backspace leaves part of what was drawn as
/// one character.
pub(crate) fn the_editor_steps_the_graphemes_the_painter_snaps_to() {
    use flui_painting::text_boundaries::graphemes;

    let texts = [
        "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}b",
        "\u{1F1FA}\u{1F1F8}\u{1F1EB}\u{1F1F7}",
        "e\u{301}\u{302}x",
        "a\r\nb",
        "\u{915}\u{94D}\u{937}\u{93F}",
    ];
    let mut failures = Vec::new();
    for text in texts {
        let clusters: Vec<_> = graphemes(text).collect();
        let controller = TextEditingController::with_text(text);

        controller.set_caret_byte_offset(0);
        let mut right = Vec::new();
        for _ in 0..text.len() {
            controller.move_caret_right();
            right.push(controller.caret_byte_offset());
            if controller.caret_byte_offset() == text.len() {
                break;
            }
        }
        let ends: Vec<usize> = clusters.iter().map(|cluster| cluster.end).collect();

        controller.set_caret_byte_offset(text.len());
        let mut left = Vec::new();
        for _ in 0..text.len() {
            controller.move_caret_left();
            left.push(controller.caret_byte_offset());
            if controller.caret_byte_offset() == 0 {
                break;
            }
        }
        let starts: Vec<usize> = clusters.iter().rev().map(|cluster| cluster.start).collect();

        let mut backspaces = 0;
        while !controller.text().is_empty() && backspaces <= text.len() {
            controller.move_caret_end();
            controller.backspace();
            backspaces += 1;
        }

        if right != ends || left != starts || backspaces != clusters.len() {
            failures.push(format!(
                "{text:?}: right {right:?} vs {ends:?}, left {left:?} vs {starts:?}, \
                 {backspaces} backspaces for {} clusters",
                clusters.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ------------------------------------------------------------------
// The field as a text store (ADR-0090)
//
// The conformance kit (`text_store_kit.rs`) certifies the contract; these
// pin what is specific to `EditableText`: how the store's UTF-16 surface
// maps onto the controller, and when the field and the platform hear of
// each other's changes.
// ------------------------------------------------------------------

pub(crate) mod text_store {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, Selection, TextStore, TextStoreEdit, TextStoreError,
        TextStoreRead, Utf16Offset,
    };
    use flui_widgets::{EditableText, TextEditingController};

    use super::character_key_event;
    use crate::common::harness::{Harness, mount_with_ime};

    /// "a", a supplementary emoji, "e" plus a combining acute, a ZWJ family
    /// and a flag: every offset kind the UTF-16 surface has to map.
    const CORPUS: &str = "a😀e\u{301}👨‍👩‍👧🇯🇵";

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    fn focused(controller: &TextEditingController) -> (Harness, Rc<FocusNode>) {
        let focus_node = FocusNode::with_debug_label("text store field");
        let mut harness = mount_with_ime(EditableText::new(
            controller.clone(),
            Rc::clone(&focus_node),
        ));
        focus_node.request_focus();
        harness.tick();
        (harness, focus_node)
    }

    fn store(harness: &Harness) -> Rc<dyn TextStore> {
        harness
            .active_text_store()
            .expect("the focused field is the active IME client")
    }

    fn read<R: 'static>(
        store: &Rc<dyn TextStore>,
        body: impl FnOnce(&dyn TextStoreRead) -> R + 'static,
    ) -> R {
        let slot = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);
        let outcome = store.request_lock(
            LockGrant::read(move |session| *sink.borrow_mut() = Some(body(session))),
            LockTiming::Sync,
        );
        assert_eq!(outcome, Ok(LockOutcome::Granted));
        slot.take().expect("the grant ran")
    }

    fn edit<R: 'static>(
        store: &Rc<dyn TextStore>,
        body: impl FnOnce(&mut dyn TextStoreEdit) -> R + 'static,
    ) -> R {
        let slot = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);
        let outcome = store.request_lock(
            LockGrant::read_write(move |session| *sink.borrow_mut() = Some(body(session))),
            LockTiming::Sync,
        );
        assert_eq!(outcome, Ok(LockOutcome::Granted));
        slot.take().expect("the grant ran")
    }

    pub(crate) fn rtl_scalar_rect_midpoints_resolve_to_the_source_scalar() {
        use flui_foundation::geometry::Point;
        use flui_platform_api::text_store::{PointMode, Utf16Range};
        let controller = TextEditingController::with_text("אב");
        let (harness, _focus) = focused(&controller);
        let field = store(&harness);
        read(&field, |session| {
            for scalar in 0..2 {
                let from = session
                    .rect_for_range(Utf16Range::collapsed(at(scalar)))
                    .expect("laid-out scalar caret")
                    .bounds;
                let to = session
                    .rect_for_range(Utf16Range::collapsed(at(scalar + 1)))
                    .expect("laid-out next scalar caret")
                    .bounds;
                assert!(from.origin.x > to.origin.x, "Hebrew source carets descend");
                let point = Point::new(
                    from.origin.x.midpoint(to.origin.x),
                    from.origin.y + from.size.height / 2.0,
                );
                assert_eq!(
                    session.index_at_point(point, PointMode::Exact),
                    Ok(at(scalar))
                );
                assert_eq!(
                    session.index_at_point(from.origin, PointMode::Nearest),
                    Ok(at(scalar))
                );
            }
        });
    }

    /// The store's UTF-16 offsets and the controller's UTF-8 bytes name the
    /// same positions, in both directions.
    pub(crate) fn store_offsets_match_controller_bytes_across_surrogates_and_graphemes() {
        let controller = TextEditingController::with_text(CORPUS);
        let (harness, _focus) = focused(&controller);
        let store = store(&harness);

        // Platform to controller: UTF-16 13 is the flag's start, byte 26.
        let set = edit(&store, |session| {
            session.set_selection(Selection {
                anchor: at(13),
                active: at(1),
            })
        });
        assert_eq!(set, Ok(()));
        assert_eq!(controller.selection(), 1..26);
        assert_eq!(
            controller.caret_byte_offset(),
            1,
            "the active end is the caret"
        );

        // Controller to platform: bytes 5..8 ("e" and its mark) are 3..5.
        controller.set_selection(5, 8);
        let seen = read(&store, |session| session.selection());
        assert_eq!(
            seen,
            Selection {
                anchor: at(3),
                active: at(5)
            }
        );
        assert_eq!(read(&store, |session| session.document_len()), at(17));
    }

    /// A commit the input method sent inside a frame is queued; when that
    /// frame fails before its commit anchor, the commit is still queued as
    /// the next key arrives, and the key lands after it.
    ///
    /// Red-check: drop `run_deferred_before_app_edit` from the key handler —
    /// the key lands while the commit is still queued and the text reads "b".
    pub(crate) fn typing_after_a_deferred_commit_lands_after_the_commit() {
        let controller = TextEditingController::new();
        let (mut harness, _focus) = focused(&controller);
        let field = store(&harness);
        let text_in_frame = Rc::new(RefCell::new(None));
        let (observed_controller, observed_text) = (controller.clone(), Rc::clone(&text_in_frame));
        harness
            .local_post_frame_handle()
            .schedule_local(move |_| {
                let outcome = flui_platform_api::text_store::project_ime_event(
                    &*field,
                    &flui_platform_api::ImeEvent::Commit("A".to_owned()),
                );
                assert_eq!(outcome, Ok(LockOutcome::Deferred));
                *observed_text.borrow_mut() = Some(observed_controller.text());
                panic!("the frame fails after the commit was queued");
            })
            .expect("post-frame handle installed");

        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
        assert!(failed.is_err(), "the failing frame is raised");
        assert_eq!(
            text_in_frame.borrow().as_deref(),
            Some(""),
            "inside the frame the commit waits for the anchor"
        );
        assert_eq!(
            controller.text(),
            "",
            "the failed frame never reached its anchor, so the commit is still queued"
        );

        let handled = harness
            .focus_manager()
            .dispatch_key_event(&character_key_event('b'));
        assert!(handled);
        assert_eq!(controller.text(), "Ab");
    }

    /// `on_changed` runs once the platform's session has released its lock:
    /// it receives the committed text (the composition left out), and a
    /// synchronous lock it requests is granted and sees the session's
    /// result.
    ///
    /// Red-check: call `on_changed` from the session's write-back, under the
    /// lock — the nested request is refused; or compare the whole text — the
    /// owner receives the preedit.
    pub(crate) fn on_changed_runs_after_the_lock_is_released() {
        use flui_platform_api::text_store::{Composition, Utf16Range};
        type Seen = (
            Result<LockOutcome, TextStoreError>,
            Option<(Utf16Offset, Option<Utf16Range>)>,
        );
        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("settled field");
        let slot: Rc<RefCell<Option<Rc<dyn TextStore>>>> = Rc::new(RefCell::new(None));
        type Heard = Rc<RefCell<Vec<(String, Option<Seen>)>>>;
        let heard: Heard = Rc::new(RefCell::new(Vec::new()));
        let (store_slot, sink) = (Rc::clone(&slot), Rc::clone(&heard));
        let mut harness = mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&focus_node)).on_changed(
                move |_cx, text| {
                    let store = store_slot.borrow().clone();
                    let seen = store.map(|store| {
                        let read = Rc::new(RefCell::new(None));
                        let out = Rc::clone(&read);
                        let outcome = store.request_lock(
                            LockGrant::read(move |session| {
                                *out.borrow_mut() = Some((
                                    session.document_len(),
                                    session.composition().map(|composition| composition.range),
                                ));
                            }),
                            LockTiming::Sync,
                        );
                        (outcome, read.take())
                    });
                    sink.borrow_mut().push((text.to_owned(), seen));
                },
            ),
        );
        focus_node.request_focus();
        harness.tick();
        let field = store(&harness);
        *slot.borrow_mut() = Some(Rc::clone(&field));
        let composing = Utf16Range::new(at(2), at(6)).expect("ordered");
        edit(&field, move |session| {
            session.insert_at_selection("東京").expect("in range");
            session.insert_at_selection("おおさか").expect("in range");
            session
                .set_composition(Some(Composition {
                    range: composing,
                    hides_caret: false,
                }))
                .expect("in range");
        });
        // The store holds `on_changed`, which holds the slot.
        slot.borrow_mut().take();
        assert_eq!(
            *heard.borrow(),
            vec![(
                "東京".to_owned(),
                Some((Ok(LockOutcome::Granted), Some((at(6), Some(composing))))),
            )],
            "one owner notification with the committed text, its lock granted after the session"
        );
        assert_eq!(controller.text(), "東京おおさか");
    }

    /// A field that gains focus is the store the window's input-method host
    /// serves; losing focus ends its composition, then takes it away
    /// (ADR-0135, ADR-0142 item 4). The pull window is the one Windows
    /// offers.
    ///
    /// Red-check: have the presentation's text-input owner skip its host —
    /// the host hears nothing and serves no store.
    pub(crate) fn focus_gain_and_loss_reach_the_store_host() {
        use flui_testing::StoreHostCall;

        let controller = TextEditingController::with_text("ab");
        let (mut harness, focus_node) = focused(&controller);
        assert_eq!(harness.store_host_calls(), [StoreHostCall::Focus]);
        edit(&store(&harness), |session| {
            session.insert_at_selection("c").expect("insert");
        });
        assert_eq!(controller.text(), "abc", "the host serves this field");

        focus_node.unfocus();
        harness.tick();
        assert_eq!(
            harness.store_host_calls(),
            [
                StoreHostCall::Focus,
                StoreHostCall::CompleteComposition,
                StoreHostCall::Unfocus
            ]
        );
        assert!(harness.active_text_store().is_none());
    }

    thread_local! {
        /// The field a controller listener reaches: a listener is
        /// `Send + Sync` and the store is not.
        static LISTENED_FIELD: RefCell<Option<Rc<dyn TextStore>>> = const { RefCell::new(None) };
    }

    /// A controller listener that answers the session it hears of with a
    /// synchronous session of its own: each committed session is one
    /// `on_changed`, in commit order, with the committed text that session
    /// produced; the nested one settles inside the outer's and absorbs
    /// nothing of it.
    ///
    /// Red-check: take the owed `on_changed` after the controller's
    /// listeners run — the owner hears once; or deliver the live committed
    /// text after the listeners — the owner hears "ab" twice.
    pub(crate) fn a_listener_session_inside_settle_is_its_own_on_changed() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        use flui_foundation::Listenable as _;

        let controller = TextEditingController::new();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&calls);
        let focus_node = FocusNode::with_debug_label("listener session field");
        let mut harness = mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&focus_node))
                .on_changed(move |_cx, text| sink.borrow_mut().push(text.to_owned())),
        );
        focus_node.request_focus();
        harness.tick();
        let field = store(&harness);
        LISTENED_FIELD.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&field)));
        let answered = Arc::new(AtomicBool::new(false));
        let once = Arc::clone(&answered);
        let listener = controller.add_listener(Arc::new(move || {
            if once.swap(true, Ordering::SeqCst) {
                return;
            }
            let field = LISTENED_FIELD.with(|slot| slot.borrow().clone());
            if let Some(field) = field {
                let outcome = field.request_lock(
                    LockGrant::read_write(|session| {
                        session.insert_at_selection("b").expect("in range");
                    }),
                    LockTiming::Sync,
                );
                assert_eq!(outcome, Ok(LockOutcome::Granted), "the listener's session");
            }
        }));
        edit(&field, |session| {
            session.insert_at_selection("a").expect("in range");
        });
        controller.remove_listener(listener);
        LISTENED_FIELD.with(|slot| slot.borrow_mut().take());
        assert!(
            answered.load(Ordering::SeqCst),
            "the listener heard the session"
        );
        assert_eq!(controller.text(), "ab");
        assert_eq!(
            *calls.borrow(),
            ["a", "ab"],
            "one on_changed per committed session, in commit order, each with the text it committed"
        );
    }

    /// The application edits the field while the platform holds a lock (a
    /// nested modal loop, an async task): the platform's session is dropped,
    /// the application's edit stays, and the platform hears of it once the
    /// lock is released.
    ///
    /// Red-check: write the session back without comparing the controller's
    /// generation — the text reads "ime" and the observer hears nothing.
    pub(crate) fn an_app_edit_during_a_lock_is_not_overwritten() {
        use flui_platform_api::text_store::{TextChange, TextStoreObserver};
        struct Changes(Rc<RefCell<Vec<TextChange>>>);
        impl TextStoreObserver for Changes {
            fn text_changed(&self, change: TextChange) {
                self.0.borrow_mut().push(change);
            }
            fn selection_changed(&self) {}
            fn layout_changed(&self) {}
            fn status_changed(&self) {}
        }
        let controller = TextEditingController::new();
        let (harness, _focus) = focused(&controller);
        let field = store(&harness);
        let heard = Rc::new(RefCell::new(Vec::new()));
        field.set_observer(Some(Rc::new(Changes(Rc::clone(&heard)))));
        let app = controller.clone();
        edit(&field, move |session| {
            session.insert_at_selection("ime").expect("in range");
            app.set_text("app");
        });
        field.set_observer(None);
        assert_eq!(controller.text(), "app", "the application's edit stays");
        assert_eq!(
            *heard.borrow(),
            [TextChange {
                start: at(0),
                old_end: at(0),
                new_end: at(3),
            }],
            "the platform hears of the application's edit after the lock"
        );
    }

    /// The application gives the field another controller while an input
    /// method holds a read-write lock (a rebuild in a nested owner-thread
    /// loop): the session was made against the replaced controller, so it is
    /// dropped, and neither controller receives it.
    ///
    /// Red-check: drop the `is_same_controller` check in the store's
    /// write-back — the replaced controller, whose generation did not move,
    /// receives "ime".
    pub(crate) fn swapping_the_controller_during_a_grant_drops_the_session() {
        let old = TextEditingController::with_text("old");
        let new = TextEditingController::with_text("new");
        let focus_node = FocusNode::with_debug_label("swapped field");
        let harness = Rc::new(RefCell::new(mount_with_ime(EditableText::new(
            old.clone(),
            Rc::clone(&focus_node),
        ))));
        focus_node.request_focus();
        harness.borrow_mut().tick();
        let field = store(&harness.borrow());
        let (nested, replacement, node) =
            (Rc::clone(&harness), new.clone(), Rc::clone(&focus_node));
        edit(&field, move |session| {
            session.insert_at_selection("ime").expect("in range");
            nested
                .borrow_mut()
                .swap_root(EditableText::new(replacement, node));
        });
        assert_eq!(old.text(), "old", "the replaced controller is not written");
        assert_eq!(
            new.text(),
            "new",
            "the new controller keeps the application's text"
        );
    }

    /// A focused field whose `on_changed` runs `on_changed`.
    fn focused_with(
        controller: &TextEditingController,
        on_changed: impl Fn(&str) + 'static,
    ) -> (Harness, Rc<FocusNode>) {
        let focus_node = FocusNode::with_debug_label("owner failure field");
        let mut harness = mount_with_ime(
            EditableText::new(controller.clone(), Rc::clone(&focus_node))
                .on_changed(move |_cx, text| on_changed(text)),
        );
        focus_node.request_focus();
        harness.tick();
        (harness, focus_node)
    }

    /// The text of the panic `run` raised, if it raised one.
    fn raised(run: impl FnOnce()) -> Option<String> {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).err()?;
        let text = flui_foundation::panic::payload_text(&*payload)
            .unwrap_or("an opaque payload")
            .to_owned();
        flui_foundation::panic::retain_opaque_payload(payload);
        Some(text)
    }

    /// An `on_changed` that records each committed text it receives and
    /// panics, naming the text, for the texts in `failing`.
    fn failing_owner(
        failing: &'static [&'static str],
    ) -> (Rc<RefCell<Vec<String>>>, impl Fn(&str) + 'static) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&calls);
        (calls, move |text: &str| {
            sink.borrow_mut().push(text.to_owned());
            assert!(!failing.contains(&text), "owner failure on {text}");
        })
    }

    /// Queue `grants` read-write grants from a post-frame callback, inside
    /// the frame's transaction, so the next commit anchor runs them in order.
    fn queue_in_a_frame(harness: &mut Harness, field: &Rc<dyn TextStore>, grants: Vec<LockGrant>) {
        let field = Rc::clone(field);
        harness
            .local_post_frame_handle()
            .schedule_local(move |_| {
                for grant in grants {
                    assert_eq!(
                        field.request_lock(grant, LockTiming::Async),
                        Ok(LockOutcome::Deferred)
                    );
                }
            })
            .expect("post-frame handle installed");
    }

    fn insert(text: &'static str) -> LockGrant {
        LockGrant::read_write(move |session| {
            session.insert_at_selection(text).expect("in range");
        })
    }

    /// A failure-path matrix for an `on_changed` that panics after an input
    /// method's grant (ADR-0142 item 2): the grant stands, the
    /// failure reaches the realm's report exactly once, the first of two
    /// stays authoritative, the field keeps working, and the platform hears
    /// of an owner's edit before the next grant runs.
    pub(crate) fn a_panicking_on_changed_is_reported_once_and_the_field_keeps_working() {
        crate::common::cases::run_cases(
            "panicking on_changed",
            &[
                (
                    "a failure after a direct grant reaches the next owner turn once",
                    a_failure_after_a_direct_grant_reaches_the_next_owner_turn_once as fn(),
                ),
                (
                    "a failure in a dispatched commit is raised by the dispatch",
                    a_failure_in_a_dispatched_commit_is_raised_by_the_dispatch,
                ),
                (
                    "of two failures at one anchor the first is reported",
                    of_two_failures_at_one_anchor_the_first_is_reported,
                ),
                (
                    "the platform hears of an owner edit before the next grant",
                    the_platform_hears_of_an_owner_edit_before_the_next_grant,
                ),
            ],
        );
    }

    fn a_failure_after_a_direct_grant_reaches_the_next_owner_turn_once() {
        let controller = TextEditingController::new();
        let (calls, owner) = failing_owner(&["a"]);
        let (mut harness, _focus) = focused_with(&controller, owner);
        let field = store(&harness);
        assert_eq!(
            field.request_lock(insert("a"), LockTiming::Sync),
            Ok(LockOutcome::Granted),
            "the owner's failure does not undo the grant"
        );
        assert_eq!(controller.text(), "a");
        assert_eq!(
            raised(|| harness.tick()),
            Some("owner failure on a".to_owned()),
            "the next owner turn reports the failure"
        );
        assert_eq!(raised(|| harness.tick()), None, "and reports it once");
        edit(&field, |session| {
            session.insert_at_selection("b").expect("in range");
        });
        assert_eq!(controller.text(), "ab", "the next grant runs");
        assert_eq!(*calls.borrow(), ["a", "ab"], "and its owner hears of it");
    }

    fn a_failure_in_a_dispatched_commit_is_raised_by_the_dispatch() {
        let controller = TextEditingController::new();
        let (calls, owner) = failing_owner(&["a"]);
        let (mut harness, _focus) = focused_with(&controller, owner);
        assert_eq!(
            raised(|| harness.dispatch_ime(&flui_platform_api::ImeEvent::Commit("a".to_owned()))),
            Some("owner failure on a".to_owned()),
            "the dispatch that ran the grant reports the failure"
        );
        assert_eq!(controller.text(), "a");
        assert_eq!(raised(|| harness.tick()), None, "it is reported once");
        harness.dispatch_ime(&flui_platform_api::ImeEvent::Commit("b".to_owned()));
        assert_eq!(controller.text(), "ab");
        assert_eq!(*calls.borrow(), ["a", "ab"]);
    }

    fn of_two_failures_at_one_anchor_the_first_is_reported() {
        let controller = TextEditingController::new();
        let (calls, owner) = failing_owner(&["a", "ab"]);
        let (mut harness, _focus) = focused_with(&controller, owner);
        let field = store(&harness);
        queue_in_a_frame(&mut harness, &field, vec![insert("a"), insert("b")]);
        assert_eq!(
            raised(|| harness.tick()),
            Some("owner failure on a".to_owned()),
            "the first failure stays authoritative"
        );
        assert_eq!(
            controller.text(),
            "ab",
            "the queue drained past the failure"
        );
        assert_eq!(*calls.borrow(), ["a", "ab"]);
        assert_eq!(
            raised(|| harness.tick()),
            None,
            "the second failure is retained, not reported"
        );
        edit(&field, |session| {
            session.insert_at_selection("c").expect("in range");
        });
        assert_eq!(
            *calls.borrow(),
            ["a", "ab", "abc"],
            "the next grant reaches its owner"
        );
    }

    fn the_platform_hears_of_an_owner_edit_before_the_next_grant() {
        use flui_platform_api::text_store::{TextChange, TextStoreObserver};
        struct Logged(Rc<RefCell<Vec<&'static str>>>);
        impl TextStoreObserver for Logged {
            fn text_changed(&self, _: TextChange) {
                self.0.borrow_mut().push("platform heard the owner's edit");
            }
            fn selection_changed(&self) {}
            fn layout_changed(&self) {}
            fn status_changed(&self) {}
        }
        let controller = TextEditingController::new();
        let app = controller.clone();
        let (mut harness, _focus) = focused_with(&controller, move |text| {
            if text == "a" {
                app.set_text("app");
                panic!("owner failure on {text}");
            }
        });
        let field = store(&harness);
        let log = Rc::new(RefCell::new(Vec::new()));
        field.set_observer(Some(Rc::new(Logged(Rc::clone(&log)))));
        let second = Rc::clone(&log);
        queue_in_a_frame(
            &mut harness,
            &field,
            vec![
                insert("a"),
                LockGrant::read_write(move |_| second.borrow_mut().push("second grant")),
            ],
        );
        assert_eq!(
            raised(|| harness.tick()),
            Some("owner failure on a".to_owned())
        );
        field.set_observer(None);
        assert_eq!(
            *log.borrow(),
            ["platform heard the owner's edit", "second grant"],
            "the owner's edit is reported before the next grant, though the owner panicked"
        );
        assert_eq!(controller.text(), "app");
    }

    /// Text entered through the focus manager remains editable in a narrow
    /// viewport: geometry, candidate placement and pointer insertion agree.
    pub(crate) fn long_input_reveals_the_caret_and_maps_visible_pointer_positions() {
        crate::common::cases::run_cases(
            "long input viewport",
            &[
                ("latin input", long_latin_input as fn()),
                ("rtl input", long_rtl_input),
                ("obscured input", long_obscured_input),
                (
                    "push candidate area",
                    long_input_reports_the_visible_candidate_area,
                ),
            ],
        );
    }

    /// A push-model platform places its candidate window from the area the
    /// field reports, which follows the visible caret, not its position in
    /// the whole text.
    fn long_input_reports_the_visible_candidate_area() {
        use flui_interaction::events::{Code, Key, KeyState, Modifiers, NamedKey};
        use flui_interaction::testing::input::KeyEventBuilder;
        use flui_widgets::SizedBox;

        let controller = TextEditingController::new();
        let focus = FocusNode::new();
        let mut harness = crate::common::harness::mount_with_push_ime(
            SizedBox::new(60.0, 30.0).child(EditableText::new(controller, Rc::clone(&focus))),
        );
        focus.request_focus();
        let assert_visible = |harness: &Harness| {
            let candidate = harness
                .cursor_area_calls()
                .last()
                .copied()
                .expect("candidate area reported");
            assert!(
                candidate.origin.x >= -0.001 && candidate.origin.x + candidate.size.width <= 60.001,
                "IME candidate tracks the visible caret: {candidate:?}"
            );
        };
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            assert!(
                harness
                    .focus_manager()
                    .dispatch_key_event(&super::character_key_event(ch))
            );
            harness.tick();
        }
        assert_visible(&harness);
        for key in [NamedKey::Home, NamedKey::End] {
            let event = KeyEventBuilder::new(Code::Home)
                .with_key(Key::Named(key))
                .with_state(KeyState::Down)
                .with_modifiers(Modifiers::empty())
                .build();
            assert!(harness.focus_manager().dispatch_key_event(&event));
            harness.tick();
            assert_visible(&harness);
        }
    }

    fn long_latin_input() {
        long_input("abcdefghijklmnopqrstuvwxyz", false);
    }

    fn long_rtl_input() {
        long_input("אבגדהוזחטיכלמנסעפצקרשת", false);
    }

    fn long_obscured_input() {
        long_input("abcdefghijklmnopqrstuvwxyz", true);
    }

    fn long_input(text: &'static str, obscured: bool) {
        use flui_foundation::geometry::Point;
        use flui_interaction::events::{Code, Key, KeyState, Modifiers, NamedKey};
        use flui_interaction::testing::input::KeyEventBuilder;
        use flui_platform_api::text_store::{PointMode, TextStoreError, Utf16Range};
        use flui_widgets::SizedBox;

        let controller = TextEditingController::new();
        let focus = FocusNode::new();
        let mut harness = crate::common::harness::mount_with_ime(SizedBox::new(60.0, 30.0).child(
            EditableText::new(controller.clone(), Rc::clone(&focus)).obscure_text(obscured),
        ));
        focus.request_focus();
        for ch in text.chars() {
            assert!(
                harness
                    .focus_manager()
                    .dispatch_key_event(&super::character_key_event(ch))
            );
            harness.tick();
        }
        assert_eq!(controller.text(), text);
        let field = store(&harness);
        let assert_visible = |_: &Harness| {
            let caret = controller.caret_byte_offset();
            let units =
                flui_platform_api::text_store::utf16::utf16_offset(&controller.text(), caret)
                    .expect("controller caret is a scalar boundary");
            let rect = read(&field, move |session| {
                session
                    .rect_for_range(Utf16Range::collapsed(units))
                    .expect("laid-out caret")
                    .bounds
            });
            assert!(
                rect.origin.x >= -0.001 && rect.origin.x + rect.size.width <= 60.001,
                "active caret must stay inside the field for {text:?}: {rect:?}"
            );
            read(&field, move |session| {
                let mut hidden = 0;
                for scalar in 0..session.document_len().get() {
                    let from = session
                        .rect_for_range(Utf16Range::collapsed(at(scalar)))
                        .expect("scalar geometry")
                        .bounds;
                    let to = session
                        .rect_for_range(Utf16Range::collapsed(at(scalar + 1)))
                        .expect("next scalar geometry")
                        .bounds;
                    let x = from.origin.x.midpoint(to.origin.x);
                    if !(0.0..60.0).contains(&x) {
                        hidden += 1;
                        let point = Point::new(x, from.origin.y + from.size.height / 2.0);
                        assert_eq!(
                            session.index_at_point(point, PointMode::Exact),
                            Err(TextStoreError::PointOutside),
                            "an offscreen glyph must not answer an exact viewport query"
                        );
                    }
                }
                assert!(hidden > 0, "long input has offscreen glyphs");
            });
            rect
        };
        assert_visible(&harness);
        let units = flui_platform_api::text_store::utf16::utf16_len(text);
        let full = read(&field, move |session| {
            session
                .rect_for_range(Utf16Range::new(at(0), units).expect("ordered document range"))
                .expect("laid-out document")
        });
        assert!(full.clipped, "long document exceeds the visible field");
        for key in [NamedKey::Home, NamedKey::End, NamedKey::Home, NamedKey::End] {
            let event = KeyEventBuilder::new(Code::Home)
                .with_key(Key::Named(key))
                .with_state(KeyState::Down)
                .with_modifiers(Modifiers::empty())
                .build();
            assert!(harness.focus_manager().dispatch_key_event(&event));
            harness.tick();
            assert_visible(&harness);
        }

        let select = KeyEventBuilder::new(Code::ArrowLeft)
            .with_key(Key::Named(NamedKey::ArrowLeft))
            .with_state(KeyState::Down)
            .with_modifiers(Modifiers::SHIFT)
            .build();
        assert!(harness.focus_manager().dispatch_key_event(&select));
        harness.tick();
        let selected = read(&field, |session| {
            session
                .rect_for_range(session.selection().range())
                .expect("selected geometry")
        });
        assert!(
            !selected.clipped,
            "the selected adjacent grapheme uses viewport coordinates"
        );
        let end = KeyEventBuilder::new(Code::End)
            .with_key(Key::Named(NamedKey::End))
            .with_state(KeyState::Down)
            .build();
        assert!(harness.focus_manager().dispatch_key_event(&end));
        harness.tick();

        // A visible suffix boundary is not the same x as its full-content
        // position. Tapping the caret must retain the byte insertion point.
        let rect = assert_visible(&harness);
        let before = controller.caret_byte_offset();
        harness.dispatch_pointer_down(rect.origin.x, rect.origin.y + rect.size.height / 2.0);
        harness.dispatch_pointer_up(rect.origin.x, rect.origin.y + rect.size.height / 2.0);
        assert_eq!(
            controller.caret_byte_offset(),
            before,
            "pointer inverse must include horizontal reveal"
        );
        assert!(
            harness
                .focus_manager()
                .dispatch_key_event(&super::character_key_event('!'))
        );
        assert_eq!(
            controller.text(),
            format!("{text}!"),
            "typing follows the tapped visible boundary"
        );
        harness.tick();
        assert_visible(&harness);

        harness.swap_root(SizedBox::new(30.0, 30.0).child(
            EditableText::new(controller.clone(), Rc::clone(&focus)).obscure_text(obscured),
        ));
        let units = flui_platform_api::text_store::utf16::utf16_len(&controller.text());
        let rect = read(&field, move |session| {
            session
                .rect_for_range(Utf16Range::collapsed(units))
                .expect("resized caret")
                .bounds
        });
        assert!(
            rect.origin.x >= -0.001 && rect.origin.x + rect.size.width <= 30.001,
            "resize reveals the same active caret: {rect:?}"
        );
        harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
            text: "xy".to_owned(),
            cursor: Some((0, 2)),
        });
        harness.tick();
        let composition = read(&field, |session| {
            session
                .rect_for_range(
                    session
                        .composition()
                        .expect("preedit established composition")
                        .range,
                )
                .expect("composition geometry")
        });
        assert!(
            !composition.clipped,
            "short preedit is revealed in the resized viewport"
        );
        assert!(
            composition.bounds.origin.x >= -0.001
                && composition.bounds.origin.x + composition.bounds.size.width <= 30.001,
            "composition geometry shares the text displacement: {composition:?}"
        );
    }

    pub(crate) fn editable_paint_places_long_text_under_the_viewport_clip() {
        use flui_painting::DrawOp;
        use flui_painting::paint::Clip;
        use flui_rendering::layer::Layer;
        use flui_widgets::SizedBox;
        let controller = TextEditingController::new();
        let focus = FocusNode::new();
        let mut laid = crate::common::lay_out(
            SizedBox::new(60.0, 30.0).child(EditableText::new(controller, Rc::clone(&focus))),
            crate::common::tight(60.0, 30.0),
        );
        focus.request_focus();
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            laid.focus_manager()
                .dispatch_key_event(&super::character_key_event(ch));
        }
        laid.tick();
        let tree = laid.layer_tree().expect("typing painted a frame");
        let mut text_pictures = 0;
        for (id, node) in tree.iter() {
            let Layer::Picture(picture) = node.layer() else {
                continue;
            };
            if !picture
                .picture()
                .commands()
                .iter()
                .any(|command| matches!(command.op, DrawOp::Paragraph { .. }))
            {
                continue;
            }
            text_pictures += 1;
            let mut parent = tree.parent(id);
            let mut clipped = false;
            while let Some(id) = parent {
                if let Some(Layer::ClipRect(clip)) = tree.get_layer(id) {
                    clipped |=
                        clip.clip_behavior() == Clip::HardEdge && clip.clip_rect().width() == 60.0;
                }
                parent = tree.parent(id);
            }
            assert!(
                clipped,
                "the text's actual picture must descend from its viewport clip"
            );
        }
        assert!(text_pictures > 0, "input produced painted text");
    }
}

/// Event context (ADR-0086): `on_changed` and `on_submitted` run inside a
/// write the field opens from the writer source it acquired in
/// `init_state`, whichever path made the edit.
pub(crate) mod event_cx {
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_view::prelude::*;
    use flui_widgets::{EditableText, TextEditingController};

    use super::character_key_event;
    use crate::common::harness::{Harness, mount_with_ime};
    use crate::common::{ProbeSignals, SignalProbe};

    /// A focused field built by `field`, below a probe.
    fn mounted(
        field: impl Fn(ProbeSignals, TextEditingController, Rc<FocusNode>) -> EditableText + 'static,
    ) -> (SignalProbe, Harness, TextEditingController) {
        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("event field");
        let (probe_controller, probe_node) = (controller.clone(), Rc::clone(&focus_node));
        let probe = SignalProbe::new(move |signals| {
            field(signals, probe_controller.clone(), Rc::clone(&probe_node))
        });
        let mut harness = mount_with_ime(probe.view());
        harness.enter_owner_scope(|| focus_node.request_focus());
        harness.tick();
        (probe, harness, controller)
    }

    pub(crate) fn typing_writes_through_on_changed_and_rebuilds_its_reader() {
        let (probe, mut harness, _controller) = mounted(|signals, controller, node| {
            let count = signals.count;
            EditableText::new(controller, node)
                .on_changed(move |cx, text| count.set(cx, text.len() as u32))
        });

        let keys = harness.focus_manager();
        keys.dispatch_key_event(&character_key_event('a'));
        keys.dispatch_key_event(&character_key_event('b'));

        assert_eq!(probe.value(), Ok(2));
        harness.tick();
        assert_eq!(probe.reads().last(), Some(&2), "the reader rebuilt");
    }

    pub(crate) fn a_refused_write_in_on_changed_is_reported_not_panicked() {
        let (probe, harness, controller) = mounted(|signals, controller, node| {
            let released = signals.released;
            EditableText::new(controller, node).on_changed(move |cx, _text| released.set(cx, 1))
        });

        let (_, log) = flui_testing::log_capture::capture(|| {
            harness
                .focus_manager()
                .dispatch_key_event(&character_key_event('a'))
        });

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        assert_eq!(controller.text(), "a", "the edit itself landed");
        assert_eq!(probe.value(), Ok(0));
    }
}

/// Inserting before an existing mark joins a cluster; Backspace removes it whole.
pub(crate) fn insertion_keeps_the_caret_after_the_joined_combining_cluster() {
    let controller = TextEditingController::with_text("\u{301}");
    controller.set_caret_byte_offset(0);
    controller.insert_str("e");
    assert_eq!(controller.text(), "e\u{301}");
    assert_eq!(controller.caret_byte_offset(), controller.text().len());
    controller.backspace();
    assert_eq!(controller.text(), "");
    controller.insert_str("ok");
    assert_eq!(controller.caret_byte_offset(), 2);
}

pub(crate) fn deleting_a_separator_keeps_the_caret_after_the_joined_flag() {
    let controller = TextEditingController::with_text("🇦 🇧");
    controller.set_selection(4, 5);
    controller.insert_str("");
    assert_eq!(controller.text(), "🇦🇧");
    assert_eq!(controller.caret_byte_offset(), 8);
    controller.backspace();
    assert_eq!(controller.text(), "");
    controller.insert_str("x");
    assert_eq!(controller.caret_byte_offset(), 1);
}

fn selection_contact(id: u64) -> flui_interaction::PointerId {
    flui_interaction::PointerId::new(id).expect("nonzero fixture contact")
}

fn selection_field() -> (crate::common::LaidOut, TextEditingController, Rc<FocusNode>) {
    use flui_foundation::geometry::Size;
    use flui_rendering::constraints::BoxConstraints;
    let controller = TextEditingController::with_text("hello world");
    let focus = FocusNode::with_debug_label("persistent selection contact");
    let tree = crate::common::lay_out(
        EditableText::new(controller.clone(), Rc::clone(&focus)),
        BoxConstraints::tight(Size::new(500.0, 40.0)),
    );
    (tree, controller, focus)
}

pub(crate) fn selection_drag_survives_a_same_controller_rebuild() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    };
    let (mut tree, controller, focus) = selection_field();
    let contact = selection_contact(41);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        contact,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = controller.caret_byte_offset();
    assert_eq!(anchor, 0);
    tree.pump_widget(EditableText::new(controller.clone(), Rc::clone(&focus)).caret_height(19.0));
    tree.dispatch_pointer_event(&make_move_event_for_id(
        contact,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(controller.selection().start, anchor);
    assert!(controller.selection().end > anchor);
    tree.dispatch_pointer_event(&make_cancel_event_for_id(contact, PointerType::Touch));
    let next = selection_contact(42);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let next_anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(controller.caret_byte_offset() < next_anchor);
}

fn foreign_selection_terminal(cancel: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
        make_up_event_for_id,
    };
    let (tree, controller, _focus) = selection_field();
    let own = selection_contact(51);
    let foreign = selection_contact(52);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        own,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_down_event_for_id(
        foreign,
        Offset::new(150.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        anchor,
        "first contact owns selection"
    );
    tree.dispatch_pointer_event(&make_move_event_for_id(
        foreign,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        anchor,
        "foreign move cannot select"
    );
    let terminal = if cancel {
        make_cancel_event_for_id(foreign, PointerType::Touch)
    } else {
        make_up_event_for_id(foreign, Offset::new(400.0, 5.0), PointerType::Touch)
    };
    tree.dispatch_pointer_event(&terminal);
    tree.dispatch_pointer_event(&make_move_event_for_id(
        own,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(controller.selection().start, anchor);
    assert!(
        controller.selection().end > anchor,
        "foreign terminal preserves own drag"
    );
    tree.dispatch_pointer_event(&make_cancel_event_for_id(own, PointerType::Touch));
    let next = selection_contact(53);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let next_anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(controller.caret_byte_offset() < next_anchor);
}

pub(crate) fn foreign_release_preserves_the_selection_contact() {
    foreign_selection_terminal(false);
}
pub(crate) fn foreign_cancel_preserves_the_selection_contact() {
    foreign_selection_terminal(true);
}

fn selection_retarget(replace: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    };
    let (mut tree, old, focus) = selection_field();
    let own = selection_contact(61);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        own,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let current = if replace {
        TextEditingController::with_text("replacement")
    } else {
        old
    };
    tree.pump_widget(EditableText::new(current.clone(), Rc::clone(&focus)).enabled(replace));
    if !replace {
        tree.pump_widget(EditableText::new(current.clone(), Rc::clone(&focus)));
    }
    let before = current.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        own,
        Offset::new(if replace { 1.0 } else { 400.0 }, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        current.caret_byte_offset(),
        before,
        "retired contact cannot edit the current document"
    );
    tree.dispatch_pointer_event(&make_cancel_event_for_id(own, PointerType::Touch));
    let next = selection_contact(62);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = current.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(
        current.caret_byte_offset() < anchor,
        "new contact edits after retirement"
    );
}

pub(crate) fn disabling_the_field_retires_its_selection_contact() {
    selection_retarget(false);
}
pub(crate) fn replacing_the_controller_retires_the_old_selection_contact() {
    selection_retarget(true);
}

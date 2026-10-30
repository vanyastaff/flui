//! An agent's action reads the tree back once its effect has settled
//! (ADR-0238 §9, R16): a delta against the tree the action started on, the
//! whole tree, or nothing, taken from the frame the action's effect landed in.

use std::rc::Rc;
use std::sync::atomic::AtomicU32;

use flui_protocol::{
    ActOutcome, ActionName, ActionRequest, ChangedField, EffectKind, InputPath, Snapshot,
    SnapshotReading,
};
use flui_semantics::{
    AccessibilityNodeId, SemanticsAction, SemanticsNode, SemanticsOwner, SemanticsRole,
};

use super::agent_semantics::{counter, find, frame, read};
use super::*;

static_assertions::assert_impl_all!(AgentReply<ActOutcome>: Send);

fn shown() -> (UiRuntime, Arc<AtomicU32>, SemanticsAgent, ScriptedSink) {
    let ui_runtime = UiRuntime::for_test();
    let presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, presses, agent, sink)
}

fn tap_and_read(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    snapshot: Snapshot,
) -> AgentReply<ActOutcome> {
    let button = find(&read(ui_runtime, agent), "Increment").id;
    agent
        .act_and_read(ActionRequest::new(button, ActionName::Invoke), snapshot)
        .expect("the inbox has room")
}

#[test]
fn a_delta_is_read_after_the_frame_the_actions_effect_landed_in() {
    let (ui_runtime, presses, agent, mut sink) = shown();
    let mut reply = tap_and_read(&ui_runtime, &agent, Snapshot::Delta);

    let _report = ui_runtime.drain_commands();
    assert_eq!(
        presses.load(Ordering::Relaxed),
        1,
        "the handler ran at the drain"
    );
    assert!(
        reply.try_take().is_none(),
        "the reading waits for the frame its effect lands in"
    );
    frame(&ui_runtime, &mut sink);
    let outcome = reply
        .try_take()
        .expect("the frame answered")
        .expect("the action ran");

    assert_eq!(outcome.via, InputPath::Semantic);
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    assert!(!outcome.readback_failed);
    let Some(SnapshotReading::Delta(delta)) = outcome.snapshot else {
        panic!("a delta was asked for: {:?}", outcome.snapshot);
    };
    assert_eq!(delta.changed.len(), 1, "only the count changed: {delta:?}");
    assert_eq!(delta.changed[0].node.name.as_deref(), Some("Count: 1"));
    assert_eq!(delta.changed[0].changed, vec![ChangedField::Name]);
    assert!(delta.appeared.is_empty() && delta.gone.is_empty());
}

#[test]
fn a_full_reading_is_the_tree_the_frame_left() {
    let (ui_runtime, _presses, agent, mut sink) = shown();
    let mut reply = tap_and_read(&ui_runtime, &agent, Snapshot::Full);
    let _report = ui_runtime.drain_commands();
    frame(&ui_runtime, &mut sink);

    let outcome = reply
        .try_take()
        .expect("the frame answered")
        .expect("the action ran");
    let Some(SnapshotReading::Full(tree)) = outcome.snapshot else {
        panic!("the whole tree was asked for: {:?}", outcome.snapshot);
    };
    assert_eq!(find(&tree, "Count: 1").name.as_deref(), Some("Count: 1"));
}

#[test]
fn no_reading_answers_at_the_drain() {
    let (ui_runtime, presses, agent, _sink) = shown();
    let mut reply = tap_and_read(&ui_runtime, &agent, Snapshot::None);
    let _report = ui_runtime.drain_commands();

    let outcome = reply
        .try_take()
        .expect("nothing is owed, so the drain answered")
        .expect("the action ran");
    assert_eq!(outcome.snapshot, None);
    assert!(!outcome.readback_failed);
    assert_eq!(presses.load(Ordering::Relaxed), 1);
}

#[test]
fn an_action_that_dirtied_nothing_still_reads_back_after_a_frame() {
    let ui_runtime = UiRuntime::for_test();
    let render_id = RenderId::new(1);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().set_role(SemanticsRole::Button);
    node.config_mut().set_label("Press");
    node.config_mut()
        .add_action(SemanticsAction::Click, Rc::new(|_| {}));
    let mut owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = owner.insert(node);
    owner.set_root(Some(root));
    ui_runtime
        .pipeline_for_test()
        .with_mut(|pipeline| pipeline.set_semantics_owner(Some(owner)));
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let element = flui_protocol::ElementId::from_u64(AccessibilityNodeId::from(render_id).as_u64())
        .expect("an accessibility id is non-zero");

    let presentation = ui_runtime.presentation_id();
    let _ = ui_runtime.take_redraw_pending_for_test(presentation);
    let mut reply = agent
        .act_and_read(
            ActionRequest::new(element, ActionName::Invoke),
            Snapshot::Delta,
        )
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    assert!(reply.try_take().is_none());
    assert_eq!(
        ui_runtime.take_redraw_pending_for_test(presentation),
        Some(true),
        "an action that dirtied nothing still asks for the frame its reading waits on"
    );
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);

    let outcome = reply
        .try_take()
        .expect("a frame with nothing to do still passed")
        .expect("the action ran");
    let Some(SnapshotReading::Delta(delta)) = outcome.snapshot else {
        panic!("a delta was asked for: {:?}", outcome.snapshot);
    };
    assert!(delta.is_empty(), "{delta:?}");
}

#[test]
fn a_window_that_closes_first_leaves_an_outcome_that_ran_without_a_reading() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let render_id = RenderId::new(1);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().set_role(SemanticsRole::Button);
    node.config_mut().set_label("Press");
    node.config_mut()
        .add_action(SemanticsAction::Click, Rc::new(|_| {}));
    let mut owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = owner.insert(node);
    owner.set_root(Some(root));
    ui_runtime
        .presentations
        .get(second)
        .expect("the ui_runtime hosts the second presentation")
        .pipeline()
        .with_mut(|pipeline| pipeline.set_semantics_owner(Some(owner)));
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let element = flui_protocol::ElementId::from_u64(AccessibilityNodeId::from(render_id).as_u64())
        .expect("an accessibility id is non-zero");
    let mut reply = agent
        .act_and_read(
            ActionRequest::new(element, ActionName::Invoke),
            Snapshot::Delta,
        )
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    assert!(reply.try_take().is_none(), "the reading is owed");

    assert!(ui_runtime.close_presentation_entered(second));
    let outcome = reply
        .try_take()
        .expect("closing answered what it owed")
        .expect("the action ran before the window closed");
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    assert!(outcome.readback_failed, "the tree could not be read back");
    assert_eq!(outcome.snapshot, None);
}

//! The raster executor's command protocol, on the testing backend
//! (ADR-0305 §3–§6): no GPU, no window.

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use flui_engine::{
    DeviceHealth, EngineError, ExecutorHealth, PresentDisposition, RasterCommand, RasterExecutor,
    RasterSendError, RasterSender, RasterVerdict, SendReceipt, SlotChange, SlotConfig, SlotFailure,
    SlotId, SlotPhase, SlotSettlement, SlotTarget, SlotVisibility,
    raster_executor::LIFECYCLE_CAPACITY,
    testing::{ScriptedAnswer, ScriptedSlots, SlotCall},
};
use flui_foundation::{
    FrameEpoch, FrameStamp, GpuResourceGeneration, ManualClock, MonotonicClock,
    PresentationAddress, PresentationId, SurfaceGeneration, UiRuntimeId,
    geometry::{DeviceSize, Rect},
    memory::MemoryPressureLevel,
    panic::FirstPanic,
};
use flui_layer::{
    AnnotatedRegionLayer, CanvasLayer, DamageRect, DamageRegion, FullReason, Layer, LayerTree,
    Region, ResyncReason, Scene, SceneSnapshot,
};

/// An executor over the testing backend, its sender, a clone of the backend
/// and the count of host wakes.
struct Rig {
    executor: RasterExecutor<ScriptedSlots>,
    sender: RasterSender,
    backend: ScriptedSlots,
    wakes: Arc<AtomicUsize>,
    /// The clock the device's rebuilds are paced on.
    clock: ManualClock,
}

fn rig() -> Rig {
    let clock = ManualClock::new();
    rig_on(std::rc::Rc::new(clock.clone()), clock)
}

/// A rig whose executor reads `paced`, a clock over `clock`'s timeline.
fn rig_on(paced: std::rc::Rc<dyn MonotonicClock>, clock: ManualClock) -> Rig {
    let backend = ScriptedSlots::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let (executor, sender) = RasterExecutor::new(backend.clone(), paced, move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    Rig {
        executor,
        sender,
        backend,
        wakes,
        clock,
    }
}

fn address(presentation: usize) -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(presentation),
    }
}

/// A slot the executor attached, with what its frames are stamped with.
#[derive(Clone, Copy)]
struct Attached {
    slot: SlotId,
    address: PresentationAddress,
    generation: SurfaceGeneration,
}

fn attach_command(slot: SlotId, address: PresentationAddress) -> RasterCommand {
    RasterCommand::Attach {
        slot,
        address,
        target: SlotTarget::Offscreen {
            size: DeviceSize::new(8, 8),
            format: wgpu::TextureFormat::Rgba8Unorm,
        },
        config: SlotConfig::new(),
    }
}

/// Mint, attach and turn: a slot ready for frames.
fn attached(rig: &mut Rig, presentation: usize) -> Attached {
    let slot = rig.sender.mint_slot().expect("a slot id is left");
    let address = address(presentation);
    let receipt = rig
        .sender
        .send(attach_command(slot, address))
        .expect("the attach is accepted");
    let SendReceipt::Configured(generation) = receipt else {
        panic!("a positive size configures the slot: {receipt:?}");
    };
    let _ = rig.executor.turn();
    assert_eq!(phase(&rig.sender, slot), SlotPhase::Attached);
    Attached {
        slot,
        address,
        generation,
    }
}

fn scene() -> Scene {
    Scene::new(LayerTree::new(Layer::from(CanvasLayer::new())))
}

fn snapshot(at: Attached, epoch: u64, damage: DamageRegion, scene: Scene) -> SceneSnapshot {
    let mut frame_epoch = FrameEpoch::ZERO;
    for _ in 0..epoch {
        frame_epoch = frame_epoch.next();
    }
    SceneSnapshot::new(
        FrameStamp::new(
            at.address,
            frame_epoch,
            at.generation,
            GpuResourceGeneration::ZERO,
        ),
        damage,
        scene,
    )
}

fn frame(at: Attached, epoch: u64) -> RasterCommand {
    RasterCommand::Frame {
        slot: at.slot,
        snapshot: snapshot(at, epoch, DamageRegion::Unchanged, scene()),
    }
}

fn drain(sender: &RasterSender, slot: SlotId) -> Vec<SlotSettlement> {
    let mut settled = Vec::new();
    sender.drain_settlements(slot, &mut settled);
    settled
}

fn verdicts(sender: &RasterSender, slot: SlotId) -> Vec<(u64, RasterVerdict)> {
    drain(sender, slot)
        .into_iter()
        .map(|settlement| (settlement.epoch.get(), settlement.verdict))
        .collect()
}

fn phase(sender: &RasterSender, slot: SlotId) -> SlotPhase {
    sender.record(slot).expect("the owner holds the slot").phase
}

/// The full repaint a slot owes after a frame of it failed or it was hidden.
const RESET: DamageRegion = DamageRegion::Full(FullReason::Resync(ResyncReason::RasterReset));

fn partial(left: f64, top: f64, right: f64, bottom: f64) -> DamageRegion {
    let rect = DamageRect::try_covering(Rect::from_ltrb(left, top, right, bottom), (100, 100))
        .expect("a finite rect")
        .expect("a rect on the surface");
    DamageRegion::Region(Region::new(rect))
}

/// The lifecycle queue holds 64 commands; the 65th comes back whole, and
/// every accepted one is applied by the next turn.
#[test]
fn a_full_lifecycle_queue_returns_the_command() {
    let mut rig = rig();
    let accepted: Vec<SlotId> = (1..=LIFECYCLE_CAPACITY)
        .map(|presentation| {
            let slot = rig.sender.mint_slot().expect("a slot id is left");
            let _ = rig
                .sender
                .send(attach_command(slot, address(presentation)))
                .expect("the queue has room");
            slot
        })
        .collect();
    let overflow = rig.sender.mint_slot().expect("a slot id is left");
    let refused = rig
        .sender
        .send(attach_command(overflow, address(LIFECYCLE_CAPACITY + 1)))
        .expect_err("the queue is full");
    let RasterSendError::Full(command) = refused else {
        panic!("a full queue answers Full: {refused:?}");
    };
    assert!(
        matches!(*command, RasterCommand::Attach { slot, .. } if slot == overflow),
        "the refused attach comes back whole"
    );
    assert_eq!(
        rig.sender.record(overflow),
        None,
        "a refused attach leaves no slot behind"
    );

    let applied = rig.executor.turn();
    assert_eq!(applied.commands, LIFECYCLE_CAPACITY);
    assert!(
        accepted
            .iter()
            .all(|slot| phase(&rig.sender, *slot) == SlotPhase::Attached),
        "every accepted attach is applied"
    );
    let _ = rig
        .sender
        .send(*command)
        .expect("sent again after the turn, it is accepted");
    let _ = rig.executor.turn();
    assert_eq!(phase(&rig.sender, overflow), SlotPhase::Attached);
}

/// A detached slot's id is refused for every command, and the index it
/// frees comes back under a new id the old one never names.
#[test]
fn a_stale_slot_id_is_refused_after_detach() {
    let mut rig = rig();
    let old = attached(&mut rig, 1);
    let _ = rig
        .sender
        .send(RasterCommand::Detach { slot: old.slot })
        .expect("the detach is accepted");
    let refused = rig
        .sender
        .send(frame(old, 1))
        .expect_err("a detaching slot takes no frame");
    assert!(matches!(refused, RasterSendError::StaleSlot(_)));
    let _ = rig.executor.turn();
    assert_eq!(phase(&rig.sender, old.slot), SlotPhase::Detached);

    let reused = rig.sender.mint_slot().expect("a slot id is left");
    assert_eq!(reused.index(), old.slot.index(), "the index is reused");
    assert_ne!(reused, old.slot, "under a new id");
    let _ = rig
        .sender
        .send(attach_command(reused, address(2)))
        .expect("the new id attaches");
    let _ = rig.executor.turn();

    for command in [
        frame(old, 2),
        RasterCommand::Configure {
            slot: old.slot,
            change: SlotChange::Size(DeviceSize::new(4, 4)),
        },
        RasterCommand::Detach { slot: old.slot },
        attach_command(old.slot, address(1)),
    ] {
        let refused = rig.sender.send(command).expect_err("the old id is stale");
        assert!(
            matches!(refused, RasterSendError::StaleSlot(_)),
            "{refused:?}"
        );
    }
    assert_eq!(phase(&rig.sender, reused), SlotPhase::Attached);
    assert_eq!(rig.backend.renders(), [], "nothing rendered for the old id");
}

thread_local! {
    /// What a [`SendsOnDrop`] sends when it drops, and whether its send was
    /// accepted.
    static RESEND: RefCell<Option<(RasterSender, Attached)>> = const { RefCell::new(None) };
    static RESENT: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A value whose destructor sends a frame to the slot its scene was for.
#[derive(Debug)]
struct SendsOnDrop;

impl Drop for SendsOnDrop {
    fn drop(&mut self) {
        if let Some((sender, at)) = RESEND.with_borrow_mut(Option::take) {
            let accepted = sender.send(frame(at, 99)).is_ok();
            RESENT.set(Some(accepted));
        }
    }
}

/// A scene whose destructor sends to the same slot.
fn scene_that_sends_on_drop() -> Scene {
    Scene::new(LayerTree::new(Layer::from(AnnotatedRegionLayer::new(
        Rect::from_ltrb(0.0, 0.0, 1.0, 1.0),
        Arc::new(SendsOnDrop),
    ))))
}

/// A scene's destructor is user code: a superseded scene (dropped by the
/// owner's send) and a settled one (dropped by the executor's turn) are
/// dropped with the inbox lock released, so a destructor that sends to the
/// same slot is accepted instead of waiting on the lock.
///
/// Red-check: drop either scene inside its `inbox.lock()` block and the
/// destructor's send waits on the lock it is called under.
#[test]
fn a_superseded_scene_is_dropped_after_the_inbox_lock_is_released() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);

    // Superseded at send.
    let _ = rig
        .sender
        .send(RasterCommand::Frame {
            slot: at.slot,
            snapshot: snapshot(
                at,
                1,
                DamageRegion::Full(FullReason::FirstFrame),
                scene_that_sends_on_drop(),
            ),
        })
        .expect("the first frame is accepted");
    RESEND.set(Some((rig.sender.clone(), at)));
    let _ = rig
        .sender
        .send(frame(at, 2))
        .expect("the superseding frame is accepted");
    assert_eq!(
        RESENT.take(),
        Some(true),
        "the superseded scene's destructor sent"
    );
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(99, RasterVerdict::Presented)],
        "the destructor's frame superseded the second and settled"
    );

    // Settled by the turn.
    let _ = rig
        .sender
        .send(RasterCommand::Frame {
            slot: at.slot,
            snapshot: snapshot(
                at,
                3,
                DamageRegion::Full(FullReason::FirstFrame),
                scene_that_sends_on_drop(),
            ),
        })
        .expect("the frame is accepted");
    RESEND.set(Some((rig.sender.clone(), at)));
    let _ = rig.executor.turn();
    assert_eq!(
        RESENT.take(),
        Some(true),
        "the settled scene's destructor sent"
    );
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(3, RasterVerdict::Presented)]
    );
    assert_eq!(
        rig.sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .in_flight,
        1,
        "the destructor's frame is in flight"
    );
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(99, RasterVerdict::Presented)]
    );
}

/// An inline slot holds one undrained settlement: while it is undrained the
/// executor starts no frame of the slot, and the frame waits in the inbox.
#[test]
fn a_full_settlement_ring_holds_the_next_frame() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    let _ = rig.sender.send(frame(at, 2)).expect("accepted");
    let held = rig.executor.turn();
    assert_eq!(held.settled, 0, "the ring is full");
    assert_eq!(rig.backend.renders().len(), 1, "the second frame waits");
    let record = rig
        .sender
        .record(at.slot)
        .expect("the owner holds the slot");
    assert_eq!((record.in_flight, record.undrained), (1, 1));

    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::Presented)]
    );
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::Presented)]
    );
    assert_eq!(rig.backend.renders().len(), 2);
}

/// Shutting down settles every pending frame `Failed`, those of a slot whose
/// attach is still queued included, and every later send comes back `Gone`.
#[test]
fn a_stopped_executor_settles_every_pending_frame_failed() {
    let mut rig = rig();
    let shown = attached(&mut rig, 1);
    let queued_slot = rig.sender.mint_slot().expect("a slot id is left");
    let SendReceipt::Configured(generation) = rig
        .sender
        .send(attach_command(queued_slot, address(2)))
        .expect("accepted")
    else {
        panic!("a positive size configures the slot");
    };
    let queued = Attached {
        slot: queued_slot,
        address: address(2),
        generation,
    };
    let _ = rig.sender.send(frame(shown, 1)).expect("accepted");
    let _ = rig.sender.send(frame(queued, 1)).expect("accepted");

    rig.executor.shut_down();
    assert_eq!(rig.executor.health(), ExecutorHealth::ShutDown);
    assert_eq!(rig.sender.health(), ExecutorHealth::ShutDown);
    for at in [shown, queued] {
        assert_eq!(verdicts(&rig.sender, at.slot), [(1, RasterVerdict::Failed)]);
        let record = rig
            .sender
            .record(at.slot)
            .expect("the owner holds the slot");
        assert_eq!((record.phase, record.in_flight), (SlotPhase::Detached, 0));
    }
    assert_eq!(
        rig.wakes.load(Ordering::SeqCst),
        1,
        "the stop woke the host for its settlements"
    );
    assert!(
        rig.backend.calls().contains(&SlotCall::Detach(shown.slot)),
        "the attached slot is taken down"
    );

    for command in [
        frame(shown, 2),
        RasterCommand::Trim(MemoryPressureLevel::Critical),
        attach_command(
            rig.sender.mint_slot().expect("a slot id is left"),
            address(3),
        ),
    ] {
        let refused = rig.sender.send(command).expect_err("stopped");
        assert!(matches!(refused, RasterSendError::Gone(_)), "{refused:?}");
    }
    assert_eq!(
        rig.executor.turn().commands,
        0,
        "a stopped executor does nothing"
    );
}

/// A lifecycle command sent while the stop runs (here from the host's wake,
/// which the stop calls for its settlements) comes back `Gone` with its
/// command, and its slot id is the owner's again: no command is accepted into
/// a queue the stop has already drained (ADR-0305 §9).
#[test]
fn a_command_sent_during_the_stop_comes_back_gone() {
    let owner: std::rc::Rc<std::cell::OnceCell<RasterSender>> = std::rc::Rc::default();
    let answers: std::rc::Rc<RefCell<Vec<Result<SendReceipt, RasterSendError>>>> =
        std::rc::Rc::default();
    let (mut executor, sender) = RasterExecutor::new(
        ScriptedSlots::new(),
        std::rc::Rc::new(flui_foundation::SystemClock),
        {
            let owner = std::rc::Rc::clone(&owner);
            let answers = std::rc::Rc::clone(&answers);
            move || {
                let sender = owner.get().expect("the owner's sender is set");
                let slot = sender.mint_slot().expect("a slot id is left");
                answers
                    .borrow_mut()
                    .push(sender.send(attach_command(slot, address(9))));
            }
        },
    );
    let queued = sender.mint_slot().expect("a slot id is left");
    let SendReceipt::Configured(generation) = sender
        .send(attach_command(queued, address(1)))
        .expect("accepted")
    else {
        panic!("a positive size configures the slot");
    };
    let _ = sender
        .send(frame(
            Attached {
                slot: queued,
                address: address(1),
                generation,
            },
            1,
        ))
        .expect("accepted");
    assert!(owner.set(sender).is_ok());

    executor.shut_down();
    let answers = answers.borrow();
    assert_eq!(answers.len(), 1, "the stop woke the host once");
    assert!(
        matches!(&answers[0], Err(RasterSendError::Gone(command)) if matches!(**command, RasterCommand::Attach { .. })),
        "{:?}",
        answers[0]
    );
    let sender = owner.get().expect("set above");
    assert_eq!(
        sender.record(queued).map(|record| record.phase),
        Some(SlotPhase::Detached),
        "the attach queued before the stop is answered"
    );
}

/// A later frame supersedes a pending one: one render, of the survivor, with
/// the superseded frame's damage folded in, and one settlement naming it.
#[test]
fn latest_frame_wins_and_superseded_damage_folds_into_the_survivor() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    let _ = verdicts(&rig.sender, at.slot);

    for (epoch, damage) in [
        (2, partial(0.0, 0.0, 10.0, 10.0)),
        (3, partial(20.0, 20.0, 30.0, 30.0)),
    ] {
        let _ = rig
            .sender
            .send(RasterCommand::Frame {
                slot: at.slot,
                snapshot: snapshot(at, epoch, damage, scene()),
            })
            .expect("accepted");
    }
    assert_eq!(
        rig.sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .in_flight,
        1,
        "a superseded frame is not counted twice"
    );
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(3, RasterVerdict::Presented)]
    );
    assert_eq!(
        rig.backend.renders().last(),
        Some(&(
            at.slot,
            partial(20.0, 20.0, 30.0, 30.0).union(partial(0.0, 0.0, 10.0, 10.0))
        )),
        "the survivor repaints both frames' damage"
    );
}

/// A frame renders only against the generation the slot's configuration
/// has: one stamped zero, or before a resize the executor applied, settles
/// `SurfaceStale` without reaching the backend.
#[test]
fn stale_surface_generation_is_rejected_before_render() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let receipt = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(16, 16)),
        })
        .expect("accepted");
    let SendReceipt::Configured(resized) = receipt else {
        panic!("a positive size mints a generation: {receipt:?}");
    };
    assert!(resized > at.generation);
    let unstamped = Attached {
        generation: SurfaceGeneration::ZERO,
        ..at
    };
    for stale in [at, unstamped] {
        let _ = rig.sender.send(frame(stale, 1)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(1, RasterVerdict::SurfaceStale)]
        );
    }
    assert_eq!(
        rig.backend.renders(),
        [],
        "no stale frame reached the backend"
    );

    let current = Attached {
        generation: resized,
        ..at
    };
    let _ = rig.sender.send(frame(current, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::Presented)]
    );
    assert_eq!(
        rig.backend.renders(),
        [(at.slot, DamageRegion::Full(FullReason::FirstFrame))],
        "a resized slot redraws in full"
    );

    let unchanged = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(0, 16)),
        })
        .expect("accepted");
    assert_eq!(unchanged, SendReceipt::Unconfigured);
    assert_eq!(
        rig.sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .required_generation,
        resized,
        "a zero side mints nothing"
    );
}

/// Trims coalesce to the strongest level and apply before the turn's frames.
#[test]
fn trims_coalesce_to_the_strongest_level_and_apply_before_the_frame() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    for level in [
        MemoryPressureLevel::Moderate,
        MemoryPressureLevel::Critical,
        MemoryPressureLevel::Moderate,
    ] {
        let _ = rig
            .sender
            .send(RasterCommand::Trim(level))
            .expect("accepted");
    }
    let _ = rig.executor.turn();
    let calls = rig.backend.calls();
    let after_attach = &calls[1..];
    assert_eq!(
        after_attach,
        [
            SlotCall::Reclaim(MemoryPressureLevel::Critical),
            SlotCall::Render(at.slot, DamageRegion::Full(FullReason::FirstFrame)),
        ]
    );
}

/// Each answer of the backend settles the frame by its own verdict, a failed
/// or unshown frame makes the next one repaint in full, and a lost surface
/// mints the generation the owner restamps with.
#[test]
fn every_render_outcome_settles_its_frame() {
    struct Case {
        answer: ScriptedAnswer,
        verdict: RasterVerdict,
        next_damage: DamageRegion,
        restamp: bool,
    }
    let small = partial(0.0, 0.0, 4.0, 4.0);
    let cases = [
        Case {
            answer: ScriptedAnswer::Answer(PresentDisposition::Presented),
            verdict: RasterVerdict::Presented,
            next_damage: small,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Answer(PresentDisposition::NoDamage),
            verdict: RasterVerdict::NoPresent,
            next_damage: small,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Answer(PresentDisposition::NotShown),
            verdict: RasterVerdict::NotShown,
            next_damage: RESET,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Fail(EngineError::Timeout),
            verdict: RasterVerdict::Retry,
            next_damage: RESET,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Fail(EngineError::DeviceLost),
            verdict: RasterVerdict::DeviceLost,
            next_damage: RESET,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Fail(EngineError::SurfaceLost),
            verdict: RasterVerdict::SurfaceStale,
            next_damage: RESET,
            restamp: true,
        },
        Case {
            answer: ScriptedAnswer::Fail(EngineError::UnsupportedMaskShader),
            verdict: RasterVerdict::Failed,
            next_damage: RESET,
            restamp: false,
        },
        Case {
            answer: ScriptedAnswer::Panic,
            verdict: RasterVerdict::Retry,
            next_damage: RESET,
            restamp: false,
        },
    ];
    for case in cases {
        let mut rig = rig();
        let at = attached(&mut rig, 1);
        // A first presented frame, so the case's frame is not the slot's
        // first full redraw.
        let _ = rig.sender.send(frame(at, 1)).expect("accepted");
        let _ = rig.executor.turn();
        let _ = verdicts(&rig.sender, at.slot);
        let panicked = matches!(case.answer, ScriptedAnswer::Panic);
        rig.backend.script_render(case.answer);
        let _ = rig
            .sender
            .send(RasterCommand::Frame {
                slot: at.slot,
                snapshot: snapshot(at, 2, small, scene()),
            })
            .expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(2, case.verdict)],
            "{:?}",
            case.verdict
        );
        let failure = rig.sender.take_failure(at.slot);
        assert_eq!(
            matches!(failure, Some(SlotFailure::Panicked(_))),
            panicked,
            "a panic, and only a panic, goes into the record: {failure:?}"
        );
        let required = rig
            .sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .required_generation;
        assert_eq!(
            required != at.generation,
            case.restamp,
            "{:?}",
            case.verdict
        );
        let next = Attached {
            generation: required,
            ..at
        };
        let _ = rig
            .sender
            .send(RasterCommand::Frame {
                slot: at.slot,
                snapshot: snapshot(next, 3, small, scene()),
            })
            .expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(3, RasterVerdict::Presented)],
            "the next frame renders after {:?}",
            case.verdict
        );
        assert_eq!(
            rig.backend.renders().last(),
            Some(&(at.slot, case.next_damage)),
            "{:?}",
            case.verdict
        );
    }
}

/// An attach the backend refuses or panics in leaves the slot `Refused`
/// with its failure: a frame sent before the refusal settles `Failed`, later
/// commands are refused, and a detach frees its id.
#[test]
fn a_refused_attach_settles_its_pending_frame_failed() {
    for answer in [
        ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 8,
            height: 8,
        }),
        ScriptedAnswer::Panic,
    ] {
        let mut rig = rig();
        let panicked = matches!(answer, ScriptedAnswer::Panic);
        rig.backend.script_attach(answer);
        let slot = rig.sender.mint_slot().expect("a slot id is left");
        let SendReceipt::Configured(generation) = rig
            .sender
            .send(attach_command(slot, address(1)))
            .expect("accepted")
        else {
            panic!("a positive size configures the slot");
        };
        let at = Attached {
            slot,
            address: address(1),
            generation,
        };
        let _ = rig
            .sender
            .send(frame(at, 1))
            .expect("accepted before the refusal");
        let _ = rig.executor.turn();
        assert_eq!(phase(&rig.sender, slot), SlotPhase::Refused);
        assert_eq!(verdicts(&rig.sender, slot), [(1, RasterVerdict::Failed)]);
        let failure = rig.sender.take_failure(slot);
        assert_eq!(
            matches!(failure, Some(SlotFailure::Panicked(_))),
            panicked,
            "{failure:?}"
        );
        assert!(failure.is_some());
        let refused = rig.sender.send(frame(at, 2)).expect_err("refused slot");
        assert!(matches!(refused, RasterSendError::StaleSlot(_)));

        let _ = rig
            .sender
            .send(RasterCommand::Detach { slot })
            .expect("a refused slot detaches");
        let _ = rig.executor.turn();
        assert_eq!(phase(&rig.sender, slot), SlotPhase::Detached);
        assert_eq!(
            rig.sender.mint_slot().expect("a slot id is left").index(),
            slot.index(),
            "its index is free again"
        );
    }
}

/// A slot that is not shown renders nothing: its frames settle `NotShown`,
/// and its first frame once shown again repaints in full.
#[test]
fn a_slot_that_is_not_shown_settles_not_shown_and_redraws_in_full_when_shown() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    let _ = verdicts(&rig.sender, at.slot);
    for visibility in [
        SlotVisibility::Hidden,
        SlotVisibility::Minimized,
        SlotVisibility::Occluded,
    ] {
        let _ = rig
            .sender
            .send(RasterCommand::Configure {
                slot: at.slot,
                change: SlotChange::Shown(visibility),
            })
            .expect("accepted");
        let _ = rig.sender.send(frame(at, 2)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(2, RasterVerdict::NotShown)],
            "{visibility:?}"
        );
    }
    assert_eq!(rig.backend.renders().len(), 1, "no hidden frame rendered");
    let _ = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Shown(SlotVisibility::Shown),
        })
        .expect("accepted");
    let _ = rig
        .sender
        .send(RasterCommand::Frame {
            slot: at.slot,
            snapshot: snapshot(at, 3, partial(0.0, 0.0, 2.0, 2.0), scene()),
        })
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(3, RasterVerdict::Presented)]
    );
    assert_eq!(rig.backend.renders().last(), Some(&(at.slot, RESET)));
}

/// The host is woken once per turn that settled a frame, and a wake that
/// panics is resumed after the turn committed: the settlement is there and
/// the next turn runs.
#[test]
fn a_panicking_host_wake_is_resumed_after_the_turn_committed() {
    let backend = ScriptedSlots::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let (executor, sender) = RasterExecutor::new(
        backend.clone(),
        std::rc::Rc::new(flui_foundation::SystemClock),
        move || {
            assert!(
                counted.fetch_add(1, Ordering::SeqCst) != 0,
                "the first wake panics"
            );
        },
    );
    let mut rig = Rig {
        executor,
        sender,
        backend,
        wakes,
        clock: ManualClock::new(),
    };
    let at = attached(&mut rig, 1);
    assert_eq!(
        rig.wakes.load(Ordering::SeqCst),
        0,
        "an attach settles nothing"
    );
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let mut caught = FirstPanic::new();
    let turn = caught.contain(|| rig.executor.turn());
    assert!(
        turn.is_none() && caught.into_payload().is_some(),
        "the wake's panic is resumed"
    );
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::Presented)]
    );
    let _ = rig.sender.send(frame(at, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::Presented)]
    );
    assert_eq!(rig.wakes.load(Ordering::SeqCst), 2);
}

/// A resize the backend refuses for now or panics in is held in the slot's
/// record, and stays pending: frames stamped with the minted generation settle
/// `SurfaceStale` until a later turn applies it, and then render.
#[test]
fn a_failed_resize_is_held_in_the_record_and_retried_next_turn() {
    for answer in [
        ScriptedAnswer::Fail(EngineError::Timeout),
        ScriptedAnswer::Panic,
    ] {
        let mut rig = rig();
        let panicked = matches!(answer, ScriptedAnswer::Panic);
        let at = attached(&mut rig, 1);
        rig.backend.script_resize(answer);
        let SendReceipt::Configured(resized) = rig
            .sender
            .send(RasterCommand::Configure {
                slot: at.slot,
                change: SlotChange::Size(DeviceSize::new(16, 16)),
            })
            .expect("accepted")
        else {
            panic!("a positive size mints a generation");
        };
        let current = Attached {
            generation: resized,
            ..at
        };
        let _ = rig.sender.send(frame(current, 1)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(1, RasterVerdict::SurfaceStale)]
        );
        let failure = rig.sender.take_failure(at.slot);
        match failure {
            Some(SlotFailure::Panicked(_)) => assert!(panicked),
            Some(SlotFailure::Refused(_)) => assert!(!panicked),
            None => panic!("the failed resize is held in the record"),
        }

        let _ = rig.sender.send(frame(current, 2)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(2, RasterVerdict::Presented)],
            "the next turn retries the resize"
        );
        let resizes = rig
            .backend
            .calls()
            .into_iter()
            .filter(|call| matches!(call, SlotCall::Resize(..)))
            .count();
        assert_eq!(resizes, 2);
    }
}

/// Counts the `WARN` events of the executor's resize refusals.
#[derive(Default)]
struct CountsResizeWarnings(Arc<AtomicUsize>);

impl tracing::Subscriber for CountsResizeWarnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 |= format!("{value:?}").contains("resize refused");
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if *event.metadata().level() == tracing::Level::WARN && message.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A size the backend always refuses (over the device's limit) is not retried:
/// the record holds why, frames stamped for it settle `SurfaceStale`, and the
/// refusal warns once however many turns and sends repeat it. A later size the
/// backend accepts configures the slot again.
///
/// Red-check: retry every refused resize, as the executor did before it told
/// transient refusals from lasting ones, and the backend sees the size on every
/// turn and the warning repeats.
#[test]
fn a_size_the_backend_always_refuses_is_not_retried_and_warns_once() {
    let warnings = Arc::new(AtomicUsize::new(0));
    let subscriber = CountsResizeWarnings(Arc::clone(&warnings));
    tracing::subscriber::with_default(subscriber, || {
        let mut rig = rig();
        let at = attached(&mut rig, 1);
        let refused_size = DeviceSize::new(1 << 20, 16);
        let refusal = || {
            ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
                width: 1 << 20,
                height: 16,
            })
        };
        rig.backend.script_resize(refusal());
        rig.backend.script_resize(refusal());
        let resize = |rig: &Rig, size| {
            let receipt = rig
                .sender
                .send(RasterCommand::Configure {
                    slot: at.slot,
                    change: SlotChange::Size(size),
                })
                .expect("accepted");
            let SendReceipt::Configured(generation) = receipt else {
                panic!("a positive size mints a generation: {receipt:?}");
            };
            Attached { generation, ..at }
        };
        let refused = resize(&rig, refused_size);
        for epoch in 1..=3 {
            let _ = rig.sender.send(frame(refused, epoch)).expect("accepted");
            let _ = rig.executor.turn();
            assert_eq!(
                verdicts(&rig.sender, at.slot),
                [(epoch, RasterVerdict::SurfaceStale)]
            );
        }
        assert!(matches!(
            rig.sender.take_failure(at.slot),
            Some(SlotFailure::Refused(EngineError::InvalidTargetSize { .. }))
        ));
        let resizes = |rig: &Rig| {
            rig.backend
                .calls()
                .into_iter()
                .filter(|call| matches!(call, SlotCall::Resize(..)))
                .count()
        };
        assert_eq!(resizes(&rig), 1, "a lasting refusal is not retried");

        // The owner asks for the same size again: tried once more, not warned.
        let again = resize(&rig, refused_size);
        let _ = rig.sender.send(frame(again, 4)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(4, RasterVerdict::SurfaceStale)]
        );
        assert_eq!(resizes(&rig), 2);
        assert_eq!(
            warnings.load(Ordering::SeqCst),
            1,
            "one warning for the size"
        );

        let accepted = resize(&rig, DeviceSize::new(16, 16));
        let _ = rig.sender.send(frame(accepted, 5)).expect("accepted");
        let _ = rig.executor.turn();
        assert_eq!(
            verdicts(&rig.sender, at.slot),
            [(5, RasterVerdict::Presented)]
        );
    });
}

/// A subscriber that panics at every event.
struct PanicsAtEvents;

impl tracing::Subscriber for PanicsAtEvents {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {
        panic!("the subscriber panics");
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A `tracing` subscriber is user code: one that panics while the executor
/// reports a refused attach is resumed after the turn, with the slot already
/// `Refused` and its pending frame settled.
#[test]
fn a_panicking_subscriber_cannot_strand_a_refused_slot() {
    let mut rig = rig();
    rig.backend
        .script_attach(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 8,
            height: 8,
        }));
    let slot = rig.sender.mint_slot().expect("a slot id is left");
    let SendReceipt::Configured(generation) = rig
        .sender
        .send(attach_command(slot, address(1)))
        .expect("accepted")
    else {
        panic!("a positive size configures the slot");
    };
    let at = Attached {
        slot,
        address: address(1),
        generation,
    };
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let mut caught = FirstPanic::new();
    let turn = tracing::subscriber::with_default(PanicsAtEvents, || {
        caught.contain(|| rig.executor.turn())
    });
    assert!(
        turn.is_none() && caught.into_payload().is_some(),
        "the subscriber's panic is resumed"
    );
    assert_eq!(phase(&rig.sender, slot), SlotPhase::Refused);
    assert_eq!(verdicts(&rig.sender, slot), [(1, RasterVerdict::Failed)]);
}

thread_local! {
    /// The sender a payload's destructor mints through, and what it got.
    static MINTER: RefCell<Option<RasterSender>> = const { RefCell::new(None) };
    static REMINTED: Cell<bool> = const { Cell::new(false) };
}

/// A panic payload whose destructor mints a slot through [`MINTER`].
struct MintsOnDrop;

impl Drop for MintsOnDrop {
    fn drop(&mut self) {
        let sender = MINTER.with_borrow(Clone::clone);
        if let Some(sender) = sender {
            REMINTED.set(sender.mint_slot().is_ok());
        }
    }
}

/// A backend whose attach panics with a [`MintsOnDrop`] payload.
struct PanicsAtAttach;

impl flui_engine::SlotBackend for PanicsAtAttach {
    fn attach(&mut self, _: SlotId, _: SlotTarget, _: SlotConfig) -> flui_engine::EngineResult<()> {
        std::panic::panic_any(MintsOnDrop)
    }

    fn resize(&mut self, _: SlotId, _: DeviceSize) -> flui_engine::EngineResult<()> {
        Ok(())
    }

    fn render(
        &mut self,
        _: SlotId,
        _: &Scene,
        _: DamageRegion,
    ) -> flui_engine::EngineResult<PresentDisposition> {
        Ok(PresentDisposition::Presented)
    }

    fn release(&mut self, _: SlotId) {}

    fn restore(
        &mut self,
        _: SlotId,
        _: flui_engine::UnconfiguredSurface,
    ) -> flui_engine::EngineResult<()> {
        Ok(())
    }

    fn set_display_color(
        &mut self,
        _: SlotId,
        _: flui_platform::DisplayColor,
    ) -> flui_engine::EngineResult<()> {
        Ok(())
    }

    fn set_latency(&mut self, _: SlotId, _: flui_engine::FrameLatency) {}

    fn detach(&mut self, _: SlotId) {}

    fn reclaim(&mut self, _: MemoryPressureLevel) {}

    fn device_lost(&self) -> bool {
        false
    }

    fn rebuild_device(
        &mut self,
    ) -> flui_engine::EngineResult<Vec<(SlotId, flui_engine::EngineError)>> {
        Ok(Vec::new())
    }

    fn rebuilds_inline(&self) -> bool {
        true
    }
}

/// A panic payload the owner never took is dropped when its detached slot's
/// index is collected, with the owner's slot table released: its destructor
/// can mint through the same sender.
#[test]
fn an_untaken_panic_payload_is_dropped_outside_the_slot_table() {
    let (mut executor, sender) = RasterExecutor::new(
        PanicsAtAttach,
        std::rc::Rc::new(flui_foundation::SystemClock),
        || {},
    );
    let slot = sender.mint_slot().expect("a slot id is left");
    let _ = sender
        .send(attach_command(slot, address(1)))
        .expect("accepted");
    let _ = executor.turn();
    assert_eq!(phase(&sender, slot), SlotPhase::Refused);
    let _ = sender
        .send(RasterCommand::Detach { slot })
        .expect("a refused slot detaches");
    let _ = executor.turn();
    MINTER.set(Some(sender.clone()));
    REMINTED.set(false);
    let minted = sender.mint_slot().expect("a slot id is left");
    MINTER.set(None);
    assert!(REMINTED.get(), "the payload's destructor minted a slot");
    assert_eq!(
        minted.index(),
        slot.index(),
        "the collected index is reused"
    );
}

/// An error whose `Debug` and `Display` panic: a backend error can carry
/// user code.
struct PanicsWhenFormatted;

impl std::fmt::Debug for PanicsWhenFormatted {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        panic!("the error's Debug panics")
    }
}

impl std::fmt::Display for PanicsWhenFormatted {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        panic!("the error's Display panics")
    }
}

impl std::error::Error for PanicsWhenFormatted {}

fn unformattable() -> EngineError {
    EngineError::SurfaceCreation(Box::new(PanicsWhenFormatted))
}

/// The executor never formats a failure it hands to the owner: an attach or
/// resize refused with an error that panics when formatted still commits the
/// refusal, settles the pending frame and holds the refusal in the record.
#[test]
fn a_refusal_whose_error_panics_when_formatted_still_commits() {
    let mut rig = rig();
    rig.backend
        .script_attach(ScriptedAnswer::Fail(unformattable()));
    let slot = rig.sender.mint_slot().expect("a slot id is left");
    let SendReceipt::Configured(generation) = rig
        .sender
        .send(attach_command(slot, address(1)))
        .expect("accepted")
    else {
        panic!("a positive size configures the slot");
    };
    let refused = Attached {
        slot,
        address: address(1),
        generation,
    };
    let _ = rig.sender.send(frame(refused, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(phase(&rig.sender, slot), SlotPhase::Refused);
    assert_eq!(verdicts(&rig.sender, slot), [(1, RasterVerdict::Failed)]);
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::SurfaceCreation(_)))
    ));

    let at = attached(&mut rig, 2);
    rig.backend
        .script_resize(ScriptedAnswer::Fail(unformattable()));
    let SendReceipt::Configured(resized) = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(16, 16)),
        })
        .expect("accepted")
    else {
        panic!("a positive size mints a generation");
    };
    let current = Attached {
        generation: resized,
        ..at
    };
    let _ = rig.sender.send(frame(current, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::SurfaceStale)]
    );
    assert!(
        matches!(
            rig.sender.take_failure(at.slot),
            Some(SlotFailure::Refused(EngineError::SurfaceCreation(_)))
        ),
        "the refused resize is held in the record, unformatted"
    );
}

/// A surface lost while a resize is still pending keeps the resize's
/// barrier: a frame stamped with the generation the owner is told to use
/// does not render into the old-sized target until a resize succeeds.
#[test]
fn a_surface_loss_keeps_the_barrier_of_a_pending_resize() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(16, 16)),
        })
        .expect("accepted");
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::Timeout));
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceLost));
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::SurfaceStale)]
    );

    let required = Attached {
        generation: rig
            .sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .required_generation,
        ..at
    };
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::Timeout));
    let _ = rig.sender.send(frame(required, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::SurfaceStale)],
        "no frame renders before the resize it is stamped for"
    );
    assert_eq!(
        rig.backend.renders().len(),
        1,
        "only the first frame rendered"
    );

    let _ = rig.sender.send(frame(required, 3)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(3, RasterVerdict::Presented)],
        "the resize that succeeds authorises the frame"
    );
}

/// A surface lost after the backend refused a size keeps that size's barrier,
/// as a pending one does: the generation the loss mints is not authorised
/// while the slot keeps its old configuration.
///
/// Red-check: let a surface loss authorise its generation when no size is
/// pending, ignoring a refused one, and the frame renders.
#[test]
fn a_surface_loss_keeps_the_barrier_of_a_refused_size() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(1 << 20, 16)),
        })
        .expect("accepted");
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 1 << 20,
            height: 16,
        }));
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceLost));
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::SurfaceStale)]
    );
    let required = Attached {
        generation: rig
            .sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .required_generation,
        ..at
    };
    let _ = rig.sender.send(frame(required, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::SurfaceStale)],
        "no frame renders laid out for a size the slot does not have"
    );
}

/// The slot's frame as the owner stamps it now: with the generation its
/// record requires.
fn restamped(rig: &Rig, at: Attached) -> Attached {
    Attached {
        generation: rig
            .sender
            .record(at.slot)
            .expect("the owner holds the slot")
            .required_generation,
        ..at
    }
}

fn rebuilds(backend: &ScriptedSlots) -> usize {
    backend
        .calls()
        .iter()
        .filter(|call| **call == SlotCall::RebuildDevice)
        .count()
}

/// A rebuilt device refuses every frame stamped before it on its generation,
/// as a restore does, and the next frame repaints in full.
#[test]
fn a_device_rebuild_refuses_the_frames_stamped_before_it_and_redraws_in_full() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::Presented)]
    );
    let wakes = rig.wakes.load(Ordering::SeqCst);

    rig.backend.lose_device();
    let _ = rig.sender.send(frame(at, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), 1, "the first rebuild runs at once");
    assert_eq!(rig.sender.device_health(), DeviceHealth::Healthy);
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::SurfaceStale)],
        "a frame stamped before the rebuild is refused"
    );
    assert!(
        rig.wakes.load(Ordering::SeqCst) > wakes,
        "the owner is woken"
    );

    let _ = rig
        .sender
        .send(frame(restamped(&rig, at), 3))
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(3, RasterVerdict::Presented)]
    );
    assert_eq!(
        rig.backend.renders().last(),
        Some(&(
            at.slot,
            DamageRegion::Full(FullReason::Resync(ResyncReason::RasterReset))
        )),
        "the first frame on the new device repaints in full"
    );
}

/// A size still pending when the device is rebuilt keeps its barrier: frames
/// laid out for a size the slot does not have are not rendered.
#[test]
fn a_device_rebuild_keeps_the_barrier_of_a_refused_size() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    let _ = rig
        .sender
        .send(RasterCommand::Configure {
            slot: at.slot,
            change: SlotChange::Size(DeviceSize::new(1 << 20, 16)),
        })
        .expect("accepted");
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 1 << 20,
            height: 16,
        }));
    rig.backend.lose_device();
    let _ = rig.executor.turn();
    assert_eq!(rig.sender.device_health(), DeviceHealth::Healthy);
    let _ = rig
        .sender
        .send(frame(restamped(&rig, at), 1))
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::SurfaceStale)],
        "the rebuild authorises no frame laid out for the refused size"
    );
}

/// A failed rebuild settles the frame `DeviceLost`, leaves its error for the
/// owner, and the next one waits out the backoff on the executor's clock
/// (ADR-0267).
#[test]
fn a_failed_rebuild_is_reported_and_the_next_waits_out_its_backoff() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    rig.backend.lose_device();
    rig.backend
        .script_rebuild(ScriptedAnswer::Fail(EngineError::GpuUnavailable));
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::DeviceLost)]
    );
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::Rebuilding { attempts: 1 }
    );
    assert!(matches!(
        rig.sender.take_device_failure(),
        Some(EngineError::GpuUnavailable)
    ));
    assert!(rig.sender.take_device_failure().is_none(), "taken once");
    let retry_at = rig
        .sender
        .device_retry_at()
        .expect("a failed rebuild arms the next");

    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), 1, "no rebuild before the deadline");
    rig.clock
        .advance(retry_at.duration_since(MonotonicClock::now(&rig.clock)));
    let _ = rig
        .sender
        .send(frame(restamped(&rig, at), 2))
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), 2);
    assert_eq!(rig.sender.device_health(), DeviceHealth::Healthy);
    assert_eq!(rig.sender.device_retry_at(), None);
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::SurfaceStale)]
    );
}

/// A rebuild that blocks before it fails arms the next from when it ended,
/// so a slow driver is not asked again the moment it answers.
#[test]
fn a_slow_failed_rebuild_waits_its_backoff_from_when_it_ended() {
    let mut rig = rig();
    let _at = attached(&mut rig, 1);
    rig.backend.lose_device();
    rig.backend
        .rebuilds_take(&rig.clock, Duration::from_millis(500));
    rig.backend
        .script_rebuild(ScriptedAnswer::Fail(EngineError::GpuUnavailable));
    let _ = rig.executor.turn();
    let retry_at = rig
        .sender
        .device_retry_at()
        .expect("a failed rebuild arms the next");
    assert_eq!(
        retry_at.duration_since(MonotonicClock::now(&rig.clock)),
        Duration::from_millis(16),
        "the first wait runs from the attempt's end"
    );
    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), 1, "no rebuild before the deadline");
}

/// A rebuild that keeps failing at the backoff's ceiling is given up: frames
/// settle `DeviceLost` and no rebuild runs, however long, until the owner
/// sends `RetryDevice`; then the device renders again (ADR-0267).
#[test]
fn a_given_up_rebuild_holds_until_the_owner_sends_retry_device() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    rig.backend.lose_device();
    for _ in 0..64 {
        rig.backend
            .script_rebuild(ScriptedAnswer::Fail(EngineError::GpuUnavailable));
    }
    let mut turns = 0;
    while !matches!(rig.sender.device_health(), DeviceHealth::GivenUp { .. }) {
        assert!(turns < 64, "the rebuild is given up");
        let _ = rig.executor.turn();
        rig.clock.advance(Duration::from_secs(1));
        turns += 1;
    }
    let given_up = rebuilds(&rig.backend);
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::GivenUp {
            attempts: u32::try_from(given_up).expect("a short streak")
        }
    );
    assert_eq!(rig.sender.device_retry_at(), None, "nothing to wake for");
    rig.clock.advance(Duration::from_secs(3600));
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), given_up, "no rebuild of its own");
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::DeviceLost)]
    );

    let _ = rig
        .sender
        .send(RasterCommand::RetryDevice)
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        rebuilds(&rig.backend),
        given_up + 1,
        "the asked-for rebuild runs at once"
    );
    // The scripted failures are not spent: the asked-for rebuild failed too
    // and is paced again, not given up.
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::Rebuilding { attempts: 1 }
    );
}
/// A rebuild that panics once its new device is in place leaves the backend
/// rendering: the slots are restamped before the turn serves them, so a frame
/// stamped before the loss is refused, and the panic is resumed once the turn
/// committed. Without the restamp the frame renders on the half-rebuilt
/// device.
#[test]
fn a_rebuild_that_panics_with_its_device_in_place_restamps_every_slot() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    rig.backend.lose_device();
    rig.backend.script_rebuild(ScriptedAnswer::Panic);
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let mut caught = FirstPanic::new();
    let turn = caught.contain(|| rig.executor.turn());
    assert!(
        turn.is_none() && caught.into_payload().is_some(),
        "the rebuild's panic is resumed"
    );
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::SurfaceStale)],
        "a frame stamped before the loss is refused"
    );
    assert_eq!(rig.sender.device_health(), DeviceHealth::Healthy);
    let _ = rig
        .sender
        .send(frame(restamped(&rig, at), 2))
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::Presented)]
    );
}

/// A loss the turn's own frame meets is published at once, with its rebuild
/// due now, so the owner runs the next turn without waiting for a later one
/// to see it.
#[test]
fn a_loss_met_by_a_frame_is_published_with_its_rebuild_due_now() {
    let mut rig = rig();
    let at = attached(&mut rig, 1);
    rig.backend.lose_device_in_next_render();
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(1, RasterVerdict::DeviceLost)]
    );
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::Rebuilding { attempts: 0 }
    );
    assert_eq!(
        rig.sender.device_retry_at(),
        Some(MonotonicClock::now(&rig.clock)),
        "the first rebuild is due now"
    );
    assert_eq!(rebuilds(&rig.backend), 0, "the next turn rebuilds");
    let _ = rig.executor.turn();
    assert_eq!(rebuilds(&rig.backend), 1);
    assert_eq!(rig.sender.device_health(), DeviceHealth::Healthy);
}

/// A clock over a manual timeline that panics on every read once armed: the
/// owner's code failing where the executor reads the time.
#[derive(Debug)]
struct ArmedClock {
    clock: ManualClock,
    armed: std::rc::Rc<std::cell::Cell<bool>>,
}

impl MonotonicClock for ArmedClock {
    fn now(&self) -> std::time::Instant {
        assert!(!self.armed.get(), "injected clock panic");
        MonotonicClock::now(&self.clock)
    }
}

/// A clock that panics while a frame loses the device does not stall
/// recovery: the loss is still published with a rebuild due, the owner is
/// woken, and the clock's panic is resumed after the turn.
#[test]
fn a_panicking_clock_still_publishes_a_loss_and_wakes_the_owner() {
    let clock = ManualClock::new();
    let armed = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut rig = rig_on(
        std::rc::Rc::new(ArmedClock {
            clock: clock.clone(),
            armed: std::rc::Rc::clone(&armed),
        }),
        clock,
    );
    let at = attached(&mut rig, 1);
    let woken = rig.wakes.load(Ordering::SeqCst);
    rig.backend.lose_device_in_next_render();
    let _ = rig.sender.send(frame(at, 1)).expect("accepted");
    armed.set(true);
    let mut caught = FirstPanic::new();
    let turn = caught.contain(|| rig.executor.turn());
    armed.set(false);
    assert!(
        turn.is_none() && caught.into_payload().is_some(),
        "the clock's panic is resumed"
    );
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::Rebuilding { attempts: 0 }
    );
    assert!(rig.sender.device_retry_at().is_some(), "a rebuild is due");
    assert!(
        rig.wakes.load(Ordering::SeqCst) > woken,
        "the owner is woken"
    );
}

/// A backend that cannot rebuild where the executor runs (the web) is given
/// up at the first due attempt instead of walking the backoff.
#[test]
fn a_backend_that_cannot_rebuild_inline_gives_up_at_once() {
    let mut rig = rig();
    let _at = attached(&mut rig, 1);
    rig.backend.cannot_rebuild_inline();
    rig.backend.lose_device();
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.device_health(),
        DeviceHealth::GivenUp { attempts: 0 }
    );
    assert_eq!(rebuilds(&rig.backend), 0, "no attempt is made");
    assert_eq!(rig.sender.device_retry_at(), None);
    assert!(matches!(
        rig.sender.take_device_failure(),
        Some(EngineError::GpuUnavailable)
    ));
}

/// An offscreen slot whose target the rebuilt device cannot make holds why in
/// its record and asks for no surface, which only a window can be given; the
/// other slots render.
#[test]
fn an_offscreen_slot_the_rebuilt_device_refuses_holds_why_and_asks_no_surface() {
    let mut rig = rig();
    let refused = attached(&mut rig, 1);
    let kept = attached(&mut rig, 2);
    rig.backend.lose_device();
    rig.backend.refuse_on_rebuild(
        refused.slot,
        EngineError::InvalidTargetSize {
            width: 8,
            height: 8,
        },
    );
    let _ = rig.executor.turn();
    assert!(
        !rig.sender.record(refused.slot).expect("held").wants_surface,
        "an offscreen slot takes no restore"
    );
    assert!(matches!(
        rig.sender.take_failure(refused.slot),
        Some(SlotFailure::Refused(EngineError::InvalidTargetSize { .. }))
    ));
    assert!(!rig.sender.record(kept.slot).expect("held").wants_surface);
    let _ = rig
        .sender
        .send(frame(restamped(&rig, kept), 1))
        .expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, kept.slot),
        [(1, RasterVerdict::Presented)]
    );
}

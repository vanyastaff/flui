//! The raster executor's command protocol, on the testing backend
//! (ADR-0305 §3–§6): no GPU, no window.

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_engine::{
    EngineError, ExecutorHealth, PresentDisposition, RasterCommand, RasterExecutor,
    RasterSendError, RasterSender, RasterVerdict, SendReceipt, SlotChange, SlotConfig, SlotFailure,
    SlotId, SlotPhase, SlotSettlement, SlotTarget, SlotVisibility,
    raster_executor::LIFECYCLE_CAPACITY,
    testing::{ScriptedAnswer, ScriptedSlots, SlotCall},
};
use flui_foundation::{
    FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
    SurfaceGeneration, UiRuntimeId,
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
}

fn rig() -> Rig {
    let backend = ScriptedSlots::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let (executor, sender) = RasterExecutor::new(backend.clone(), move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    Rig {
        executor,
        sender,
        backend,
        wakes,
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
    let (executor, sender) = RasterExecutor::new(backend.clone(), move || {
        assert!(
            counted.fetch_add(1, Ordering::SeqCst) != 0,
            "the first wake panics"
        );
    });
    let mut rig = Rig {
        executor,
        sender,
        backend,
        wakes,
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

/// A resize the backend refuses or panics in is held in the slot's record,
/// and stays pending: frames stamped with the minted generation settle
/// `SurfaceStale` until a later turn applies it, and then render.
#[test]
fn a_failed_resize_is_held_in_the_record_and_retried_next_turn() {
    for answer in [
        ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 16,
            height: 16,
        }),
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
    fn attach(&mut self, _: SlotId, _: SlotTarget) -> flui_engine::EngineResult<()> {
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

    fn detach(&mut self, _: SlotId) {}

    fn reclaim(&mut self, _: MemoryPressureLevel) {}
}

/// A panic payload the owner never took is dropped when its detached slot's
/// index is collected, with the owner's slot table released: its destructor
/// can mint through the same sender.
#[test]
fn an_untaken_panic_payload_is_dropped_outside_the_slot_table() {
    let (mut executor, sender) = RasterExecutor::new(PanicsAtAttach, || {});
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
/// refusal, settles the pending frame and keeps the resize pending.
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
    let _ = rig.sender.send(frame(current, 2)).expect("accepted");
    let _ = rig.executor.turn();
    assert_eq!(
        verdicts(&rig.sender, at.slot),
        [(2, RasterVerdict::Presented)],
        "the resize stayed pending and was retried"
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

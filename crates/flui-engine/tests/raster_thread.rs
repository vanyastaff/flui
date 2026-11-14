//! The raster executor on its own thread (ADR-0305 §9, §10), on the testing
//! backend: no GPU, no window.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use flui_engine::{
    DETACH_DEADLINE, ExecutorFailure, ExecutorHandle, ExecutorHealth, RasterCommand,
    RasterExecutor, RasterSendError, RasterSender, RasterVerdict, SendReceipt, SlotConfig, SlotId,
    SlotPhase, SlotSettlement, SlotTarget, testing::ScriptedSlots,
};
use flui_foundation::{
    FrameEpoch, FrameStamp, ManualClock, PresentationAddress, PresentationId, SurfaceGeneration,
    UiRuntimeId, geometry::DeviceSize,
};
use flui_layer::{CanvasLayer, DamageRegion, Layer, LayerTree, Scene, SceneSnapshot};

/// How long a test waits for the raster thread before it calls the wait a
/// failure: far above a turn, far below a hung run.
const PATIENCE: Duration = Duration::from_secs(10);

/// A raster thread over the testing backend, its sender, a clone of the
/// backend and the owner's wakes.
struct Thread {
    handle: ExecutorHandle,
    sender: RasterSender,
    backend: ScriptedSlots,
    woken: mpsc::Receiver<()>,
}

fn spawn_with(wake: impl Fn() + Send + 'static) -> (ExecutorHandle, RasterSender, ScriptedSlots) {
    let backend = ScriptedSlots::new();
    let (handle, sender) =
        RasterExecutor::spawn(backend.clone(), Arc::new(ManualClock::new()), wake)
            .expect("the raster thread starts");
    (handle, sender, backend)
}

fn spawn() -> Thread {
    let (wake, woken) = mpsc::channel();
    let (handle, sender, backend) = spawn_with(move || {
        // The test may have stopped listening.
        let _told = wake.send(());
    });
    Thread {
        handle,
        sender,
        backend,
        woken,
    }
}

/// Poll `ready` until it holds, failing the test after [`PATIENCE`].
fn eventually(what: &str, mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < PATIENCE,
            "the raster thread never {what}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A slot the thread attached, with what its frames are stamped with.
#[derive(Clone, Copy)]
struct Attached {
    slot: SlotId,
    address: PresentationAddress,
    generation: SurfaceGeneration,
}

fn attach(sender: &RasterSender) -> Attached {
    let slot = sender.mint_slot().expect("a slot id is left");
    let address = PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    };
    let receipt = sender
        .send(RasterCommand::Attach {
            slot,
            address,
            target: SlotTarget::Offscreen {
                size: DeviceSize::new(8, 8),
                format: wgpu::TextureFormat::Rgba8Unorm,
            },
            config: SlotConfig::new(),
        })
        .expect("the attach is accepted");
    let SendReceipt::Configured(generation) = receipt else {
        panic!("a positive size configures the slot: {receipt:?}");
    };
    eventually("attached the slot", || {
        sender
            .record(slot)
            .is_some_and(|record| record.phase == SlotPhase::Attached)
    });
    Attached {
        slot,
        address,
        generation,
    }
}

fn frame(at: Attached, epoch: u64) -> RasterCommand {
    let mut frame_epoch = FrameEpoch::ZERO;
    for _ in 0..epoch {
        frame_epoch = frame_epoch.next();
    }
    RasterCommand::Frame {
        slot: at.slot,
        snapshot: SceneSnapshot::new(
            FrameStamp::new(at.address, frame_epoch, at.generation),
            DamageRegion::Unchanged,
            Scene::new(LayerTree::new(Layer::from(CanvasLayer::new()))),
        ),
    }
}

/// The settlements the thread wrote for `slot`, waiting on the owner's wake
/// for the first.
fn settled(thread: &Thread, slot: SlotId) -> Vec<SlotSettlement> {
    let started = Instant::now();
    let mut settled = Vec::new();
    while thread.sender.drain_settlements(slot, &mut settled) == 0 {
        let left = PATIENCE.saturating_sub(started.elapsed());
        thread
            .woken
            .recv_timeout(left)
            .expect("the raster thread wakes its owner once a frame settles");
    }
    settled
}

/// A send wakes the thread, its frame settles into the owner's record and the
/// owner is woken; a stop takes the slot down and later sends come back.
///
/// Red-check: without the ring in `RasterSender::send` the thread waits for a
/// ring that never comes, and the attach is never applied.
#[test]
fn a_send_wakes_the_raster_thread_and_its_frame_settles_for_the_owner() {
    let mut thread = spawn();
    let at = attach(&thread.sender);
    for epoch in 1..=3 {
        thread
            .sender
            .send(frame(at, epoch))
            .expect("the frame is accepted");
        let settled = settled(&thread, at.slot);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].verdict, RasterVerdict::Presented);
    }
    assert_eq!(
        thread
            .handle
            .shut_down(DETACH_DEADLINE)
            .map(|()| thread.handle.health()),
        Ok(ExecutorHealth::ShutDown)
    );
    assert_eq!(
        thread.sender.record(at.slot).map(|record| record.phase),
        Some(SlotPhase::Detached),
        "the stop takes every slot down"
    );
    assert!(matches!(
        thread.sender.send(frame(at, 4)),
        Err(RasterSendError::Gone(_))
    ));
}

/// A slot on the raster thread holds two settlements undrained, so one frame
/// presents while the owner builds the next (ADR-0305 §5).
///
/// Red-check: at the inline depth the second frame waits for a drain that
/// never comes.
#[test]
fn a_raster_thread_slot_renders_two_frames_before_the_owner_drains() {
    let thread = spawn();
    let at = attach(&thread.sender);
    thread.sender.send(frame(at, 1)).expect("accepted");
    eventually("rendered the first frame", || {
        thread
            .sender
            .record(at.slot)
            .is_some_and(|record| record.undrained == 1)
    });
    thread.sender.send(frame(at, 2)).expect("accepted");
    eventually("rendered the second frame undrained", || {
        thread
            .sender
            .record(at.slot)
            .is_some_and(|record| record.undrained == 2)
    });
    assert_eq!(thread.backend.renders().len(), 2);
}

/// A detach the thread does not answer in time marks the executor died:
/// every send comes back, and the owner can take why, as it takes a device's
/// failure (ADR-0305 §9).
///
/// Red-check: without `ExecutorHandle::die` the executor still reads
/// `Running` and the frame is accepted for a thread that cannot draw it.
#[test]
fn a_raster_thread_that_does_not_answer_a_detach_dies_at_its_deadline() {
    let thread = spawn();
    let at = attach(&thread.sender);
    let hold = thread.backend.hold_next_render();
    thread.sender.send(frame(at, 1)).expect("accepted");
    assert!(hold.began_within(PATIENCE), "the thread starts the render");
    thread
        .sender
        .send(RasterCommand::Detach { slot: at.slot })
        .expect("the detach is accepted");
    let waited = Duration::from_millis(50);
    assert_eq!(
        thread.handle.wait_detached(&thread.sender, at.slot, waited),
        Err(ExecutorFailure::Wedged { waited })
    );
    assert_eq!(thread.handle.health(), ExecutorHealth::Died);
    assert_eq!(thread.sender.health(), ExecutorHealth::Died);
    assert!(matches!(
        thread.sender.send(frame(at, 2)),
        Err(RasterSendError::Gone(_))
    ));
    assert_eq!(
        thread.sender.take_executor_failure(),
        Some(ExecutorFailure::Wedged { waited })
    );
    // The thread ends once its render answers: dropping the handle waits for it.
    hold.release();
}

/// A turn's panic outside a slot's command (here the host's wake) is
/// contained on the raster thread and reaches the owner as a failure it
/// reports; the thread serves on.
///
/// Red-check: without the containment in `serve` the thread unwinds, and the
/// second frame is never rendered.
#[test]
fn a_panic_in_the_hosts_wake_reaches_the_owner_and_the_thread_serves_on() {
    let panicked = Arc::new(AtomicBool::new(false));
    let (wake, woken) = mpsc::channel();
    let first = Arc::clone(&panicked);
    let (handle, sender, backend) = spawn_with(move || {
        assert!(
            first.swap(true, Ordering::SeqCst),
            "the first wake fails on purpose"
        );
        let _told = wake.send(());
    });
    let thread = Thread {
        handle,
        sender,
        backend,
        woken,
    };
    let at = attach(&thread.sender);
    thread.sender.send(frame(at, 1)).expect("accepted");
    let mut failure = None;
    eventually("handed the wake's panic over", || {
        failure = thread.sender.take_executor_failure();
        failure.is_some()
    });
    let Some(ExecutorFailure::TurnPanicked { message }) = failure else {
        panic!("a turn's panic is reported as one: {failure:?}");
    };
    assert!(
        message.contains("the first wake fails on purpose"),
        "the panic's own text reaches the owner: {message}"
    );
    assert_eq!(thread.sender.health(), ExecutorHealth::Running);
    let _first = settled(&thread, at.slot);
    thread.sender.send(frame(at, 2)).expect("accepted");
    assert_eq!(
        settled(&thread, at.slot)
            .iter()
            .map(|settlement| settlement.verdict)
            .collect::<Vec<_>>(),
        vec![RasterVerdict::Presented]
    );
}

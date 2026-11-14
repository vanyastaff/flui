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
    DETACH_DEADLINE, DeviceHealth, EngineError, ExecutorFailure, ExecutorHandle, ExecutorHealth,
    RasterCommand, RasterExecutor, RasterSendError, RasterSender, RasterVerdict, SendReceipt,
    SlotConfig, SlotId, SlotPhase, SlotSettlement, SlotTarget,
    testing::{ScriptedAnswer, ScriptedSlots, SlotCall},
};
use flui_foundation::{
    FrameEpoch, FrameStamp, ManualClock, PresentationAddress, PresentationId, SurfaceGeneration,
    UiRuntimeId, geometry::DeviceSize, memory::MemoryPressureLevel,
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
/// presents while the owner builds the next (ADR-0305 §5); a third waits for
/// the owner's drain, which wakes the thread to render it.
///
/// Red-check: at the inline depth the second frame waits for a drain that
/// never comes; without the ring in `drain_settlements` the third frame waits
/// for a send that never comes. That red-check rests on the pause after the
/// trim: a drain that lands inside the turn holding the third frame lets
/// that turn render it; the assertions hold either way.
#[test]
fn a_raster_thread_slot_renders_two_frames_before_the_owner_drains() {
    let thread = spawn();
    let at = attach(&thread.sender);
    let undrained = |count: usize| {
        thread
            .sender
            .record(at.slot)
            .is_some_and(|record| record.undrained == count)
    };
    thread.sender.send(frame(at, 1)).expect("accepted");
    eventually("rendered the first frame", || undrained(1));
    thread.sender.send(frame(at, 2)).expect("accepted");
    eventually("rendered the second frame undrained", || undrained(2));
    thread.sender.send(frame(at, 3)).expect("accepted");
    // A trim sent after the frame is reclaimed in a turn that started after
    // the frame was in the inbox; that turn holds the frame behind the full
    // ring. The pause lets it end, so the drain's ring is the only one left.
    thread
        .sender
        .send(RasterCommand::Trim(MemoryPressureLevel::Moderate))
        .expect("accepted");
    eventually("turned after the third frame", || {
        thread
            .backend
            .calls()
            .iter()
            .any(|call| matches!(call, SlotCall::Reclaim(_)))
    });
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        thread.backend.renders().len(),
        2,
        "a full ring holds the frame"
    );
    let mut drained = Vec::new();
    assert_eq!(thread.sender.drain_settlements(at.slot, &mut drained), 2);
    eventually("rendered the held frame once drained", || undrained(1));
    assert_eq!(thread.backend.renders().len(), 3);
}

/// A lost device is rebuilt when its backoff ends, with no send to wake the
/// thread: the thread waits for the rebuild's due time, not only for a ring.
///
/// Red-check: with the thread waiting for a ring alone, the device stays
/// `Rebuilding` after its first failed rebuild.
#[test]
fn a_raster_thread_rebuilds_a_lost_device_when_its_backoff_ends() {
    let (wake, woken) = mpsc::channel();
    let backend = ScriptedSlots::new();
    let (handle, sender) = RasterExecutor::spawn(
        backend.clone(),
        Arc::new(flui_foundation::clock::SystemClock),
        move || {
            let _told = wake.send(());
        },
    )
    .expect("the raster thread starts");
    let thread = Thread {
        handle,
        sender,
        backend,
        woken,
    };
    let at = attach(&thread.sender);
    thread
        .backend
        .script_rebuild(ScriptedAnswer::Fail(EngineError::GpuUnavailable));
    thread.backend.lose_device();
    // One send, so the thread turns and meets the loss.
    thread.sender.send(frame(at, 1)).expect("accepted");
    eventually("rebuilt the device after its backoff", || {
        thread.sender.device_health() == DeviceHealth::Healthy
            && thread
                .backend
                .calls()
                .iter()
                .filter(|call| matches!(call, SlotCall::RebuildDevice))
                .count()
                == 2
    });
}

/// A detach the thread does not answer in time marks the executor died:
/// every send comes back, the frames still pending settle failed, the owner
/// can take why, as it takes a device's failure, and dropping the handle no
/// longer waits for the wedged thread (ADR-0305 §9).
///
/// Red-check: without `ExecutorHandle::die` the executor still reads
/// `Running`; without the owner's settlement after the death the pending
/// frame never settles; with the handle's drop waiting again it takes
/// another `DETACH_DEADLINE`.
#[test]
fn a_raster_thread_that_does_not_answer_a_detach_dies_at_its_deadline() {
    let thread = spawn();
    let at = attach(&thread.sender);
    let hold = thread.backend.hold_next_render();
    thread.sender.send(frame(at, 1)).expect("accepted");
    assert!(hold.began_within(PATIENCE), "the thread starts the render");
    // Pending behind the held render, in the slot's inbox.
    thread.sender.send(frame(at, 2)).expect("accepted");
    let waited = Duration::from_millis(50);
    assert_eq!(
        thread.handle.detach(&thread.sender, at.slot, waited),
        Err(ExecutorFailure::Wedged { waited })
    );
    assert_eq!(thread.handle.health(), ExecutorHealth::Died);
    assert_eq!(thread.sender.health(), ExecutorHealth::Died);
    assert!(matches!(
        thread.sender.send(frame(at, 3)),
        Err(RasterSendError::Gone(_))
    ));
    let mut settled = Vec::new();
    thread.sender.drain_settlements(at.slot, &mut settled);
    assert_eq!(
        settled
            .iter()
            .map(|settlement| settlement.verdict)
            .collect::<Vec<_>>(),
        vec![RasterVerdict::Failed],
        "the pending frame settles failed, since the thread no longer can"
    );
    assert_eq!(
        thread.sender.take_executor_failure(),
        Some(ExecutorFailure::Wedged { waited })
    );
    let asked = Instant::now();
    assert_eq!(
        thread
            .handle
            .detach(&thread.sender, at.slot, DETACH_DEADLINE),
        Err(ExecutorFailure::Wedged { waited }),
        "a dead executor answers the cause it died of"
    );
    assert!(
        asked.elapsed() < DETACH_DEADLINE / 2,
        "a dead executor answers at once"
    );
    let Thread { handle, .. } = thread;
    let dropped = Instant::now();
    drop(handle);
    assert!(
        dropped.elapsed() < DETACH_DEADLINE / 2,
        "a dead executor's handle does not wait for its thread again"
    );
    // The thread stops once its render answers.
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

/// A panic in a turn that settles nothing (here a trim's reclaim) still wakes
/// the owner once the failure is there to take, since the turn's own wake
/// never ran.
///
/// Red-check: without the wake after `hand_over` in `serve`, no wake comes and
/// the owner never learns of the panic until something else settles.
#[test]
fn a_panic_in_a_turn_that_settles_nothing_wakes_the_owner() {
    let thread = spawn();
    let _at = attach(&thread.sender);
    thread.backend.script_reclaim(ScriptedAnswer::Panic);
    thread
        .sender
        .send(RasterCommand::Trim(MemoryPressureLevel::Critical))
        .expect("accepted");
    thread
        .woken
        .recv_timeout(PATIENCE)
        .expect("the owner is woken for the turn's panic");
    let failure = thread.sender.take_executor_failure();
    let Some(ExecutorFailure::TurnPanicked { message }) = failure else {
        panic!("the reclaim's panic is reported: {failure:?}");
    };
    assert!(message.contains("scripted reclaim panic"), "{message}");
    assert_eq!(thread.sender.health(), ExecutorHealth::Running);
}

/// The owner's clock, panicking on every read and counting the reads.
#[derive(Debug, Default)]
struct BrokenClock {
    reads: std::sync::atomic::AtomicUsize,
}

impl flui_foundation::clock::MonotonicClock for BrokenClock {
    fn now(&self) -> web_time::Instant {
        self.reads.fetch_add(1, Ordering::SeqCst);
        panic!("the clock fails on purpose");
    }

    fn epoch(&self) -> web_time::Instant {
        panic!("the clock fails on purpose");
    }
}

/// A clock that panics is the owner's failure, not the thread's: the first
/// read is contained and reported, the thread serves, and a lost device's
/// rebuild, which the clock can no longer pace, does not spin the thread on
/// it. The owner is woken once a failure is held, the first read's included.
///
/// Red-check: without the containment of the first read the thread never
/// builds its executor, and the attach is never applied; without the wake
/// for a failure held before the first turn, the owner is never woken; with
/// `rebuild_due_in` answering a zero wait for a clock that panics, the thread
/// turns in a loop and the reads climb.
#[test]
fn a_clock_that_panics_is_reported_and_does_not_spin_the_raster_thread() {
    let clock = Arc::new(BrokenClock::default());
    let (wake, woken) = mpsc::channel();
    let backend = ScriptedSlots::new();
    let (handle, sender) =
        RasterExecutor::spawn(backend.clone(), Arc::clone(&clock) as _, move || {
            let _told = wake.send(());
        })
        .expect("the raster thread starts");
    let thread = Thread {
        handle,
        sender,
        backend,
        woken,
    };
    let at = attach(&thread.sender);
    thread
        .woken
        .recv_timeout(PATIENCE)
        .expect("the owner is woken for the first read's panic");
    let failure = thread.sender.take_executor_failure();
    let Some(ExecutorFailure::TurnPanicked { message }) = failure else {
        panic!("the clock's panic is reported: {failure:?}");
    };
    assert!(message.contains("the clock fails on purpose"), "{message}");
    // The first rebuild fails, so the next one waits on a backoff the clock
    // would pace.
    thread
        .backend
        .script_rebuild(ScriptedAnswer::Fail(EngineError::GpuUnavailable));
    thread.backend.lose_device();
    thread.sender.send(frame(at, 1)).expect("accepted");
    eventually("published the loss", || {
        thread.sender.device_health() != DeviceHealth::Healthy
    });
    // Time for the turn that published the loss to end and the thread to
    // wait; a thread that spins reads the clock thousands of times by then.
    std::thread::sleep(Duration::from_millis(50));
    let settled = clock.reads.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    let later = clock.reads.load(Ordering::SeqCst);
    assert!(
        later <= settled + 2,
        "the thread waits for a ring, not on the clock: {settled} reads, then {later}"
    );
    assert_eq!(thread.sender.health(), ExecutorHealth::Running);
}

/// A turn's panic the owner has not taken yet does not hide a later death:
/// the owner takes the panic, then the cause the executor died of, once.
///
/// Red-check: with the death read only from the held failure, the second take
/// answers `None` and the owner never learns the executor died.
#[test]
fn a_held_panic_does_not_hide_the_executors_death() {
    let thread = spawn();
    let at = attach(&thread.sender);
    thread.backend.script_reclaim(ScriptedAnswer::Panic);
    thread
        .sender
        .send(RasterCommand::Trim(MemoryPressureLevel::Critical))
        .expect("accepted");
    thread
        .woken
        .recv_timeout(PATIENCE)
        .expect("the owner is woken for the turn's panic");
    let hold = thread.backend.hold_next_render();
    thread.sender.send(frame(at, 1)).expect("accepted");
    assert!(hold.began_within(PATIENCE), "the thread starts the render");
    let waited = Duration::from_millis(50);
    assert_eq!(
        thread.handle.detach(&thread.sender, at.slot, waited),
        Err(ExecutorFailure::Wedged { waited })
    );
    assert!(matches!(
        thread.sender.take_executor_failure(),
        Some(ExecutorFailure::TurnPanicked { .. })
    ));
    assert_eq!(
        thread.sender.take_executor_failure(),
        Some(ExecutorFailure::Wedged { waited })
    );
    assert_eq!(thread.sender.take_executor_failure(), None, "reported once");
    let Thread { handle, .. } = thread;
    drop(handle);
    hold.release();
}

/// A panic while the thread stops (here a slot's detach) settles nothing, yet
/// wakes the owner, and reaches it as the turns' panics do.
///
/// Red-check: without the wake on the stop path the owner is never woken for
/// it.
#[test]
fn a_panic_in_the_stop_wakes_the_owner_and_reaches_it() {
    let mut thread = spawn();
    let _at = attach(&thread.sender);
    thread.backend.script_detach(ScriptedAnswer::Panic);
    assert_eq!(thread.handle.shut_down(DETACH_DEADLINE), Ok(()));
    thread
        .woken
        .recv_timeout(PATIENCE)
        .expect("the owner is woken for the stop's panic");
    let failure = thread.sender.take_executor_failure();
    let Some(ExecutorFailure::TurnPanicked { message }) = failure else {
        panic!("the detach's panic is reported: {failure:?}");
    };
    assert!(message.contains("scripted detach panic"), "{message}");
}

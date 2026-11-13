//! A window slot's commands on the testing backend (ADR-0305 §2, §3, §6):
//! surfaces from wgpu's no-op backend, so no GPU and no window.

use std::sync::Arc;

use flui_engine::{
    EngineError, FrameLatency, RasterCommand, RasterExecutor, RasterSender, RasterVerdict,
    SendReceipt, SlotChange, SlotConfig, SlotFailure, SlotId, SlotPhase, SlotSize, SlotTarget,
    SurfaceFactory, SurfaceHold, UnconfiguredSurface,
    testing::{
        AttachedTarget, FakeDisplay, FakeWindow, ScriptedAnswer, ScriptedSlots, SlotCall,
        noop_surface_factory,
    },
};
use flui_foundation::{
    FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
    SurfaceGeneration, UiRuntimeId, geometry::DeviceSize,
};
use flui_layer::{CanvasLayer, DamageRegion, FullReason, Layer, LayerTree, Scene, SceneSnapshot};
use flui_platform::{ColorGamut, DisplayColor, DisplayConnection};

struct Rig {
    executor: RasterExecutor<ScriptedSlots>,
    sender: RasterSender,
    backend: ScriptedSlots,
    factory: SurfaceFactory,
}

fn rig() -> Rig {
    rig_on(std::rc::Rc::new(flui_foundation::SystemClock))
}

fn rig_on(clock: std::rc::Rc<dyn flui_foundation::MonotonicClock>) -> Rig {
    let backend = ScriptedSlots::new();
    let (executor, sender) = RasterExecutor::new(backend.clone(), clock, || {});
    let display: Arc<dyn DisplayConnection> = Arc::new(FakeDisplay);
    Rig {
        executor,
        sender,
        backend,
        factory: noop_surface_factory(display),
    }
}

impl Rig {
    fn surface(&self) -> UnconfiguredSurface {
        self.factory
            .create(Arc::new(FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window")
    }

    fn send(&self, command: RasterCommand) -> SendReceipt {
        self.sender.send(command).expect("accepted")
    }

    fn frame(&self, slot: SlotId, generation: SurfaceGeneration, epoch: u64) -> SendReceipt {
        let mut frame_epoch = FrameEpoch::ZERO;
        for _ in 0..epoch {
            frame_epoch = frame_epoch.next();
        }
        self.send(RasterCommand::Frame {
            slot,
            snapshot: SceneSnapshot::new(
                FrameStamp::new(
                    address(),
                    frame_epoch,
                    generation,
                    GpuResourceGeneration::ZERO,
                ),
                DamageRegion::Unchanged,
                Scene::new(LayerTree::new(Layer::from(CanvasLayer::new()))),
            ),
        })
    }

    fn verdicts(&self, slot: SlotId) -> Vec<(u64, RasterVerdict)> {
        let mut settled = Vec::new();
        self.sender.drain_settlements(slot, &mut settled);
        settled
            .into_iter()
            .map(|settlement| (settlement.epoch.get(), settlement.verdict))
            .collect()
    }

    /// A window slot attached and sized: its id and the generation its
    /// frames are stamped with.
    fn sized_window(&mut self) -> (SlotId, SurfaceGeneration) {
        let slot = self.sender.mint_slot().expect("a slot id is left");
        let attached = self.send(RasterCommand::Attach {
            slot,
            address: address(),
            target: SlotTarget::Window(self.surface()),
            config: SlotConfig::new(),
        });
        assert_eq!(
            attached,
            SendReceipt::Unconfigured,
            "a window attaches without a size"
        );
        let SendReceipt::Configured(generation) = self.send(RasterCommand::Configure {
            slot,
            change: SlotChange::Size(DeviceSize::new(32, 16)),
        }) else {
            panic!("a positive size configures the window");
        };
        let _ = self.executor.turn();
        assert_eq!(
            self.sender.record(slot).expect("held").phase,
            SlotPhase::Attached
        );
        (slot, generation)
    }
}

fn address() -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

/// A window slot attaches unconfigured: a frame stamped before its first size
/// is refused on its generation, and the size the owner sends in the same turn
/// as the attach configures it, so the frame stamped with that size presents
/// in full.
#[test]
fn a_window_slot_attaches_unconfigured_and_its_first_size_configures_it() {
    let mut rig = rig();
    let (slot, generation) = rig.sized_window();
    assert_eq!(
        rig.frame(slot, SurfaceGeneration::ZERO, 1),
        SendReceipt::Accepted
    );
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::SurfaceStale)]);
    let _ = rig.frame(slot, generation, 2);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(2, RasterVerdict::Presented)]);
    let calls = rig.backend.calls();
    assert_eq!(
        calls[..2],
        [
            SlotCall::Attach(slot, AttachedTarget::Window),
            SlotCall::Resize(slot, DeviceSize::new(32, 16)),
        ],
        "the attach applies before the size the same turn carries"
    );
    assert!(
        calls.contains(&SlotCall::Render(
            slot,
            DamageRegion::Full(FullReason::FirstFrame)
        )),
        "the first frame repaints in full: {calls:?}"
    );
}

/// A released window keeps its slot and generation; a restore mints the next
/// generation at send, so a frame stamped before it is refused and the one
/// stamped after it presents in full. A restore the backend refuses is held
/// in the record and its generation still guards older frames.
///
/// Red-check: let a restore keep the generation and the frame stamped before
/// it renders against the new surface.
#[test]
fn a_frame_stamped_before_a_restore_is_refused_on_its_generation() {
    let mut rig = rig();
    let (slot, before) = rig.sized_window();
    // The first frame pays the first frame's full repaint.
    let _ = rig.frame(slot, before, 0);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(0, RasterVerdict::Presented)]);
    assert_eq!(
        rig.send(RasterCommand::Release { slot }),
        SendReceipt::Accepted
    );
    let SendReceipt::Configured(after) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore of a sized window mints a generation");
    };
    assert_ne!(after, before);
    assert_eq!(
        rig.sender.record(slot).expect("held").required_generation,
        after
    );
    let _ = rig.frame(slot, before, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::SurfaceStale)]);
    let _ = rig.frame(slot, after, 2);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(2, RasterVerdict::Presented)]);
    let calls = rig.backend.calls();
    let release = calls
        .iter()
        .position(|call| *call == SlotCall::Release(slot))
        .expect("the release reached the backend");
    let restore = calls
        .iter()
        .position(|call| *call == SlotCall::Restore(slot))
        .expect("the restore reached the backend");
    assert!(release < restore, "in the order sent: {calls:?}");
    assert_eq!(
        calls[restore + 1..],
        [SlotCall::Render(
            slot,
            DamageRegion::Full(FullReason::Resync(flui_layer::ResyncReason::RasterReset))
        )],
        "the restored surface repaints in full what the release lost"
    );

    rig.backend
        .script_restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface));
    let SendReceipt::Configured(refused) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore mints a generation");
    };
    let _ = rig.frame(slot, after, 3);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(3, RasterVerdict::SurfaceStale)]);
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::IncompatibleSurface))
    ));
    assert_ne!(refused, after);
    let _ = rig.frame(slot, refused, 4);
    let _ = rig.executor.turn();
    assert_eq!(
        rig.verdicts(slot),
        [(4, RasterVerdict::Presented)],
        "a frame stamped after the refused restore reaches the backend, which answers for the released slot"
    );
}

/// The display colour and the latency coalesce in the slot's inbox: the
/// backend sees each kind's latest value once, before the frame of the same
/// turn, and a colour change repaints the frame in full.
#[test]
fn a_windows_colour_and_latency_reach_the_backend_once_a_turn() {
    let mut rig = rig();
    let (slot, generation) = rig.sized_window();
    let _ = rig.frame(slot, generation, 0);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(0, RasterVerdict::Presented)]);
    let first = DisplayColor::standard(ColorGamut::Srgb);
    let latest = DisplayColor::standard(ColorGamut::DisplayP3);
    for change in [
        SlotChange::Color(first),
        SlotChange::Latency(FrameLatency::Low),
        SlotChange::Color(latest),
        SlotChange::Latency(FrameLatency::Balanced),
    ] {
        assert_eq!(
            rig.send(RasterCommand::Configure { slot, change }),
            SendReceipt::Accepted
        );
    }
    let _ = rig.frame(slot, generation, 1);
    let before = rig.backend.calls().len();
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::Presented)]);
    assert_eq!(
        rig.backend.calls()[before..],
        [
            SlotCall::Latency(slot, FrameLatency::Balanced),
            SlotCall::DisplayColor(slot, latest),
            SlotCall::Render(slot, DamageRegion::Full(FullReason::SurfaceChanged)),
        ]
    );
}

/// A window slot that dropped a surface lost for good asks its owner for a
/// new one in its record: the frame settles not shown, the next generation is
/// minted so frames stamped before the loss are refused, and the restore the
/// owner sends clears the ask.
///
/// Red-check: classify `SurfaceReplacementNeeded` as any other failure and the
/// record never asks for a surface.
#[test]
fn a_dropped_surface_is_asked_of_the_owner_and_a_restore_answers_it() {
    let mut rig = rig();
    let (slot, before) = rig.sized_window();
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceReplacementNeeded));
    let _ = rig.frame(slot, before, 0);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(0, RasterVerdict::NotShown)]);
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(
        record.surface.hold,
        SurfaceHold::Wanted,
        "the record asks the owner for a surface"
    );
    assert_ne!(record.required_generation, before);
    let _ = rig.frame(slot, before, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::SurfaceStale)]);

    let SendReceipt::Configured(restored) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore of a sized window mints a generation");
    };
    let _ = rig.frame(slot, restored, 2);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(2, RasterVerdict::Presented)]);
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Held
    );
}

/// A restore keeps the barrier of a size the backend refused: the restored
/// surface is configured at the size the slot has, so a frame stamped after
/// the restore, laid out for the refused size, is refused on its generation
/// until a size the backend accepts arrives.
///
/// Red-check: let the restore authorise its generation whatever size is
/// pending or refused, and the frame renders into the old configuration.
#[test]
fn a_restore_keeps_the_barrier_of_a_refused_size() {
    let mut rig = rig();
    let (slot, _) = rig.sized_window();
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 1 << 20,
            height: 16,
        }));
    let SendReceipt::Configured(refused) = rig.send(RasterCommand::Configure {
        slot,
        change: SlotChange::Size(DeviceSize::new(1 << 20, 16)),
    }) else {
        panic!("a positive size mints a generation");
    };
    let _ = rig.executor.turn();
    let _refusal = rig.sender.take_failure(slot);
    let SendReceipt::Configured(restored) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore mints a generation");
    };
    assert_ne!(restored, refused);
    let _ = rig.frame(slot, restored, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::SurfaceStale)]);

    let SendReceipt::Configured(accepted) = rig.send(RasterCommand::Configure {
        slot,
        change: SlotChange::Size(DeviceSize::new(64, 32)),
    }) else {
        panic!("a positive size mints a generation");
    };
    let _ = rig.frame(slot, accepted, 2);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(2, RasterVerdict::Presented)]);
}

/// A [`ScriptedSlots`] whose next render sends a restore through the owner's
/// sender before it answers, as an owner woken inside the frame would.
struct RestoresDuringRender {
    inner: ScriptedSlots,
    restore: std::rc::Rc<std::cell::RefCell<Option<(RasterSender, UnconfiguredSurface)>>>,
}

impl flui_engine::SlotBackend for RestoresDuringRender {
    fn attach(
        &mut self,
        slot: SlotId,
        target: SlotTarget,
        config: SlotConfig,
    ) -> flui_engine::EngineResult<()> {
        self.inner.attach(slot, target, config)
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> flui_engine::EngineResult<()> {
        self.inner.resize(slot, size)
    }

    fn release(&mut self, slot: SlotId) {
        self.inner.release(slot);
    }

    fn restore(
        &mut self,
        slot: SlotId,
        surface: UnconfiguredSurface,
    ) -> flui_engine::EngineResult<()> {
        self.inner.restore(slot, surface)
    }

    fn set_display_color(
        &mut self,
        slot: SlotId,
        color: DisplayColor,
    ) -> flui_engine::EngineResult<()> {
        self.inner.set_display_color(slot, color)
    }

    fn set_latency(&mut self, slot: SlotId, latency: FrameLatency) {
        self.inner.set_latency(slot, latency);
    }

    fn apply_textures(
        &mut self,
        slot: SlotId,
        updates: flui_layer::TextureUpdates,
    ) -> flui_engine::EngineResult<()> {
        self.inner.apply_textures(slot, updates)
    }

    fn render(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        damage: DamageRegion,
    ) -> flui_engine::EngineResult<flui_engine::PresentDisposition> {
        let queued = self.restore.borrow_mut().take();
        if let Some((sender, surface)) = queued {
            let _receipt = sender
                .send(RasterCommand::Restore { slot, surface })
                .expect("accepted");
        }
        self.inner.render(slot, scene, damage)
    }

    fn detach(&mut self, slot: SlotId) {
        self.inner.detach(slot);
    }

    fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        self.inner.reclaim(level);
    }

    fn device_lost(&self) -> bool {
        self.inner.device_lost()
    }

    fn rebuild_device(
        &mut self,
    ) -> flui_engine::EngineResult<Vec<(flui_engine::SlotId, flui_engine::EngineError)>> {
        self.inner.rebuild_device()
    }

    fn rebuilds_inline(&self) -> bool {
        self.inner.rebuilds_inline()
    }
}

/// A restore sent from inside a frame that then loses its surface carries the
/// generation minted at its send, older than the one the loss mints after it:
/// applying it must not rewind the slot to that older generation, or every
/// frame stamped with the record's required generation settles stale for good.
///
/// Red-check: let the restore authorise its send-time generation whatever was
/// minted since, and the frame stamped with the required generation settles
/// `SurfaceStale`.
#[test]
fn a_restore_queued_before_a_surface_loss_does_not_rewind_the_generation() {
    let inner = ScriptedSlots::new();
    let restore = std::rc::Rc::new(std::cell::RefCell::new(None));
    let backend = RestoresDuringRender {
        inner: inner.clone(),
        restore: std::rc::Rc::clone(&restore),
    };
    let (executor, sender) = RasterExecutor::new(
        backend,
        std::rc::Rc::new(flui_foundation::SystemClock),
        || {},
    );
    let display: Arc<dyn DisplayConnection> = Arc::new(FakeDisplay);
    let factory = noop_surface_factory(display);
    let surface = || {
        factory
            .create(Arc::new(FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window")
    };
    let mut executor = executor;
    let slot = sender.mint_slot().expect("a slot id is left");
    let _ = sender
        .send(RasterCommand::Attach {
            slot,
            address: address(),
            target: SlotTarget::Window(surface()),
            config: SlotConfig::new(),
        })
        .expect("accepted");
    let SendReceipt::Configured(sized) = sender
        .send(RasterCommand::Configure {
            slot,
            change: SlotChange::Size(DeviceSize::new(32, 16)),
        })
        .expect("accepted")
    else {
        panic!("a positive size configures the window");
    };
    let _ = executor.turn();

    *restore.borrow_mut() = Some((sender.clone(), surface()));
    inner.script_render(ScriptedAnswer::Fail(EngineError::SurfaceLost));
    let frame = |generation, epoch| {
        let mut frame_epoch = FrameEpoch::ZERO;
        for _ in 0..epoch {
            frame_epoch = frame_epoch.next();
        }
        RasterCommand::Frame {
            slot,
            snapshot: SceneSnapshot::new(
                FrameStamp::new(
                    address(),
                    frame_epoch,
                    generation,
                    GpuResourceGeneration::ZERO,
                ),
                DamageRegion::Unchanged,
                Scene::new(LayerTree::new(Layer::from(CanvasLayer::new()))),
            ),
        }
    };
    let drain = || {
        let mut settled = Vec::new();
        sender.drain_settlements(slot, &mut settled);
        settled
            .into_iter()
            .map(|settlement| (settlement.epoch.get(), settlement.verdict))
            .collect::<Vec<_>>()
    };
    let _ = sender.send(frame(sized, 0)).expect("accepted");
    let _ = executor.turn();
    assert_eq!(drain(), [(0, RasterVerdict::SurfaceStale)]);

    // The restore is applied this turn; the frame is stamped as the record
    // says, after the loss.
    let required = sender.record(slot).expect("held").required_generation;
    let _ = sender.send(frame(required, 1)).expect("accepted");
    let _ = executor.turn();
    assert_eq!(drain(), [(1, RasterVerdict::Presented)]);
}

/// A window the rebuilt device cannot present to drops its surface: its
/// record asks the owner for another, with why, and a restore answers it.
#[test]
fn a_window_the_rebuilt_device_cannot_present_to_asks_for_a_new_surface() {
    let mut rig = rig();
    let (slot, _) = rig.sized_window();
    rig.backend.lose_device();
    rig.backend
        .refuse_on_rebuild(slot, EngineError::IncompatibleSurface);
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Wanted,
        "the owner makes a new surface"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::IncompatibleSurface))
    ));
    let SendReceipt::Configured(restored) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore of a sized window mints a generation");
    };
    let _ = rig.frame(slot, restored, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::Presented)]);
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Held
    );
}

/// The record tells a new surface the backend refused from a surface dropped
/// after a restore it took: the owner gives up on a frame only in the first
/// case, since in the second the next restore can draw it.
#[test]
fn a_refused_restore_is_told_apart_from_a_surface_dropped_after_one() {
    let mut rig = rig();
    let (slot, before) = rig.sized_window();
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceReplacementNeeded));
    let _ = rig.frame(slot, before, 0);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(0, RasterVerdict::NotShown)]);
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(record.surface.hold, SurfaceHold::Wanted);

    rig.backend
        .script_restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface));
    let _ = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    });
    let _ = rig.executor.turn();
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(
        record.surface.hold,
        SurfaceHold::Refused,
        "the refused restore is named"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::IncompatibleSurface))
    ));

    // The next restore is taken, and the frame of the same turn drops the
    // new surface again.
    let SendReceipt::Configured(restored) = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    }) else {
        panic!("a restore of a sized window mints a generation");
    };
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceReplacementNeeded));
    let _ = rig.frame(slot, restored, 1);
    let _ = rig.executor.turn();
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(
        record.surface.hold,
        SurfaceHold::Wanted,
        "a surface dropped after a taken restore is not a refusal"
    );
    let _ = rig.verdicts(slot);

    // A refused restore, then one that panics: the newest was not refused.
    rig.backend
        .script_restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface));
    let _ = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    });
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Refused
    );
    let _ = rig.sender.take_failure(slot);
    rig.backend.script_restore(ScriptedAnswer::Panic);
    let _ = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    });
    let _ = rig.executor.turn();
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(
        record.surface.hold,
        SurfaceHold::Wanted,
        "a panicked restore is not a refusal"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Panicked(_))
    ));
}

/// A restore over the device's size limit keeps its surface, as a rebuild
/// does: the slot waits for a size that fits, not for another surface, and
/// that size draws.
#[test]
fn a_restore_over_the_size_limit_keeps_its_surface() {
    let mut rig = rig();
    let (slot, before) = rig.sized_window();
    rig.backend
        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceReplacementNeeded));
    let _ = rig.frame(slot, before, 0);
    let _ = rig.executor.turn();
    let _ = rig.verdicts(slot);
    rig.backend
        .script_restore(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 32,
            height: 16,
        }));
    let _ = rig.send(RasterCommand::Restore {
        slot,
        surface: rig.surface(),
    });
    let _ = rig.executor.turn();
    let record = rig.sender.record(slot).expect("held");
    assert_eq!(
        record.surface.hold,
        SurfaceHold::Held,
        "the surface is kept"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::InvalidTargetSize { .. }))
    ));
    let SendReceipt::Configured(resized) = rig.send(RasterCommand::Configure {
        slot,
        change: SlotChange::Size(DeviceSize::new(16, 8)),
    }) else {
        panic!("a positive size mints a generation");
    };
    let _ = rig.frame(slot, resized, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::Presented)]);
}

/// A window over the rebuilt device's size limit keeps its surface: its
/// record holds why and asks for no new surface, and a size that fits
/// renders again.
#[test]
fn a_window_over_the_rebuilt_device_limit_waits_for_a_size_that_fits() {
    let mut rig = rig();
    let (slot, _) = rig.sized_window();
    rig.backend.lose_device();
    rig.backend.refuse_on_rebuild(
        slot,
        EngineError::InvalidTargetSize {
            width: 32,
            height: 16,
        },
    );
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Held,
        "the surface is kept"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::InvalidTargetSize { .. }))
    ));
    let SendReceipt::Configured(resized) = rig.send(RasterCommand::Configure {
        slot,
        change: SlotChange::Size(DeviceSize::new(16, 8)),
    }) else {
        panic!("a positive size mints a generation");
    };
    let _ = rig.frame(slot, resized, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::Presented)]);
}

/// A clock that panics once its timeline passes `after`: the owner's code
/// failing on a read the executor makes.
#[derive(Debug)]
struct PanicsPast {
    clock: flui_foundation::ManualClock,
    after: std::time::Duration,
}

impl flui_foundation::MonotonicClock for PanicsPast {
    fn now(&self) -> std::time::Instant {
        assert!(self.clock.elapsed() <= self.after, "injected clock panic");
        flui_foundation::MonotonicClock::now(&self.clock)
    }
}

/// A rebuild that succeeds is committed before the executor reads the
/// owner's clock again: a clock that panics once the slow attempt has ended
/// cannot drop the refusal the attempt answered, so the window still asks for
/// a new surface.
#[test]
fn a_rebuild_refusal_is_committed_before_the_clock_is_read_again() {
    let clock = flui_foundation::ManualClock::new();
    let mut rig = rig_on(std::rc::Rc::new(PanicsPast {
        clock: clock.clone(),
        after: std::time::Duration::from_secs(1),
    }));
    let (slot, _) = rig.sized_window();
    rig.backend.lose_device();
    rig.backend
        .rebuilds_take(&clock, std::time::Duration::from_secs(2));
    rig.backend
        .refuse_on_rebuild(slot, EngineError::IncompatibleSurface);
    let mut caught = flui_foundation::panic::FirstPanic::new();
    let _ = caught.contain(|| rig.executor.turn());
    if let Some(payload) = caught.into_payload() {
        flui_foundation::panic::discard_payload(payload);
    }
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.hold,
        SurfaceHold::Wanted,
        "the owner makes a new surface"
    );
    assert!(matches!(
        rig.sender.take_failure(slot),
        Some(SlotFailure::Refused(EngineError::IncompatibleSurface))
    ));
}

/// A size the lost device refused is the rebuilt device's to answer: the
/// record shows it pending again, the rebuilt device takes it, and the frame
/// laid out for it presents.
#[test]
fn a_size_the_lost_device_refused_is_tried_on_the_rebuilt_one() {
    let mut rig = rig();
    let (slot, _) = rig.sized_window();
    let wide = DeviceSize::new(64, 32);
    let _ = rig.send(RasterCommand::Configure {
        slot,
        change: SlotChange::Size(wide),
    });
    rig.backend
        .script_resize(ScriptedAnswer::Fail(EngineError::InvalidTargetSize {
            width: 64,
            height: 32,
        }));
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.size,
        SlotSize::Refused(wide)
    );
    rig.backend.lose_device();
    let _ = rig.executor.turn();
    assert_eq!(
        rig.sender.record(slot).expect("held").surface.size,
        SlotSize::Applied(wide),
        "the rebuilt device took the size"
    );
    let required = rig.sender.record(slot).expect("held").required_generation;
    let _ = rig.frame(slot, required, 1);
    let _ = rig.executor.turn();
    assert_eq!(rig.verdicts(slot), [(1, RasterVerdict::Presented)]);
}

/// Whether the device came back from a rebuild that configured every slot
/// or from one that panicked with its device in place, a size the device
/// before it refused is asked of the new one: the record never holds the
/// new device to an older device's refusal.
#[test]
fn a_rebuilt_device_answers_the_size_the_one_before_it_refused() {
    for panics in [false, true] {
        let mut rig = rig();
        let (slot, _) = rig.sized_window();
        let size = DeviceSize::new(32, 16);
        rig.backend.lose_device();
        rig.backend.refuse_on_rebuild(
            slot,
            EngineError::InvalidTargetSize {
                width: 32,
                height: 16,
            },
        );
        let _ = rig.executor.turn();
        let record = rig.sender.record(slot).expect("held");
        assert_eq!(
            (record.surface.hold, record.surface.size),
            (SurfaceHold::Held, SlotSize::Refused(size)),
            "the surface is kept, unconfigured at its size"
        );
        rig.backend.lose_device();
        if panics {
            rig.backend.script_rebuild(ScriptedAnswer::Panic);
        }
        let mut caught = flui_foundation::panic::FirstPanic::new();
        let turn = caught.contain(|| rig.executor.turn());
        assert_eq!(
            turn.is_none() && caught.into_payload().is_some(),
            panics,
            "a rebuild's panic is resumed"
        );
        assert_eq!(
            rig.sender.record(slot).expect("held").surface.size,
            SlotSize::Applied(size),
            "the second device took the size (panicked rebuild: {panics})"
        );
    }
}

/// One step of a row of [`each_surface_event_reaches_the_record`].
enum Step {
    /// The owner sends this size.
    Size(DeviceSize),
    /// The next resize answers this way.
    Resize(ScriptedAnswer),
    /// A frame whose render needs a new surface, which the turn drops.
    DropSurface,
    /// The owner restores a surface, which the backend answers this way.
    Restore(ScriptedAnswer),
    /// The device is lost; its rebuild refuses the slot with this error, if
    /// one is given.
    LoseDevice(Option<EngineError>),
    /// The executor turns.
    Turn,
}

/// The table of the slot surface model (ADR-0305 §6): each row drives a sized
/// window slot through the executor's commands and the backend's answers,
/// and reads the state its record publishes.
#[test]
fn each_surface_event_reaches_the_record() {
    use SlotSize::{Applied, Pending, Refused};
    use Step::{DropSurface, LoseDevice, Resize, Restore, Size, Turn};
    let sized = DeviceSize::new(32, 16);
    let wide = DeviceSize::new(64, 32);
    let over = |size: DeviceSize| EngineError::InvalidTargetSize {
        width: size.width.unsigned_abs(),
        height: size.height.unsigned_abs(),
    };
    let rows: Vec<(&str, Vec<Step>, SurfaceHold, SlotSize)> = vec![
        (
            "a size sent",
            vec![Size(wide)],
            SurfaceHold::Held,
            Pending(wide),
        ),
        (
            "a size taken",
            vec![Size(wide), Turn],
            SurfaceHold::Held,
            Applied(wide),
        ),
        (
            "a size refused for good",
            vec![Resize(ScriptedAnswer::Fail(over(wide))), Size(wide), Turn],
            SurfaceHold::Held,
            Refused(wide),
        ),
        (
            "a size refused for now",
            vec![
                Resize(ScriptedAnswer::Fail(EngineError::SurfaceLost)),
                Size(wide),
                Turn,
            ],
            SurfaceHold::Held,
            Pending(wide),
        ),
        (
            "a resize that panics",
            vec![Resize(ScriptedAnswer::Panic), Size(wide), Turn],
            SurfaceHold::Held,
            Pending(wide),
        ),
        (
            "a refused size, then another",
            vec![
                Resize(ScriptedAnswer::Fail(over(wide))),
                Size(wide),
                Turn,
                Size(sized),
                Turn,
            ],
            SurfaceHold::Held,
            Applied(sized),
        ),
        (
            "a dropped surface",
            vec![DropSurface],
            SurfaceHold::Wanted,
            Applied(sized),
        ),
        (
            "a restore taken",
            vec![
                DropSurface,
                Restore(ScriptedAnswer::Answer(
                    flui_engine::PresentDisposition::Presented,
                )),
            ],
            SurfaceHold::Held,
            Applied(sized),
        ),
        (
            "a restore refused",
            vec![
                DropSurface,
                Restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface)),
            ],
            SurfaceHold::Refused,
            Applied(sized),
        ),
        (
            "a restore that panics after one refused",
            vec![
                DropSurface,
                Restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface)),
                Restore(ScriptedAnswer::Panic),
            ],
            SurfaceHold::Wanted,
            Applied(sized),
        ),
        (
            "a refused restore's surface dropped again after a taken one",
            vec![
                DropSurface,
                Restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface)),
                Restore(ScriptedAnswer::Answer(
                    flui_engine::PresentDisposition::Presented,
                )),
                DropSurface,
            ],
            SurfaceHold::Wanted,
            Applied(sized),
        ),
        (
            "a restore over the size limit",
            vec![DropSurface, Restore(ScriptedAnswer::Fail(over(sized)))],
            SurfaceHold::Held,
            Refused(sized),
        ),
        (
            "a restore over the limit at a size the owner has left",
            vec![
                DropSurface,
                Size(wide),
                Restore(ScriptedAnswer::Fail(over(sized))),
            ],
            SurfaceHold::Held,
            Applied(wide),
        ),
        (
            "a rebuilt device that cannot present to the window",
            vec![LoseDevice(Some(EngineError::IncompatibleSurface)), Turn],
            SurfaceHold::Wanted,
            Applied(sized),
        ),
        (
            "a rebuilt device under the window's size",
            vec![LoseDevice(Some(over(sized))), Turn],
            SurfaceHold::Held,
            Refused(sized),
        ),
        (
            "a rebuilt device under the window's size, behind a failure the owner has not taken",
            vec![
                Resize(ScriptedAnswer::Panic),
                Size(wide),
                Turn,
                Size(sized),
                Turn,
                LoseDevice(Some(over(sized))),
                Turn,
            ],
            SurfaceHold::Held,
            Refused(sized),
        ),
        (
            "a refused size on a rebuilt device",
            vec![
                Resize(ScriptedAnswer::Fail(over(wide))),
                Size(wide),
                Turn,
                LoseDevice(None),
                Turn,
            ],
            SurfaceHold::Held,
            Applied(wide),
        ),
    ];
    for (row, steps, hold, size) in rows {
        let mut rig = rig();
        let (slot, _) = rig.sized_window();
        let mut epoch = 0;
        for step in steps {
            match step {
                Size(size) => {
                    let _ = rig.send(RasterCommand::Configure {
                        slot,
                        change: SlotChange::Size(size),
                    });
                }
                Resize(answer) => rig.backend.script_resize(answer),
                DropSurface => {
                    rig.backend
                        .script_render(ScriptedAnswer::Fail(EngineError::SurfaceReplacementNeeded));
                    let required = rig.sender.record(slot).expect("held").required_generation;
                    epoch += 1;
                    let _ = rig.frame(slot, required, epoch);
                    turn_contained(&mut rig);
                    let _ = rig.verdicts(slot);
                }
                Restore(answer) => {
                    rig.backend.script_restore(answer);
                    let _ = rig.send(RasterCommand::Restore {
                        slot,
                        surface: rig.surface(),
                    });
                    turn_contained(&mut rig);
                }
                LoseDevice(refusal) => {
                    rig.backend.lose_device();
                    if let Some(error) = refusal {
                        rig.backend.refuse_on_rebuild(slot, error);
                    }
                }
                Turn => turn_contained(&mut rig),
            }
        }
        let record = rig.sender.record(slot).expect("held");
        assert_eq!(
            (record.surface.hold, record.surface.size),
            (hold, size),
            "{row}"
        );
    }
}

/// Turn the executor, discarding a panic the row scripted.
fn turn_contained(rig: &mut Rig) {
    let mut caught = flui_foundation::panic::FirstPanic::new();
    let _ = caught.contain(|| rig.executor.turn());
    if let Some(payload) = caught.into_payload() {
        flui_foundation::panic::discard_payload(payload);
    }
}

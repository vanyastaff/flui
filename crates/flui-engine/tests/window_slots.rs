//! A window slot's commands on the testing backend (ADR-0305 §2, §3, §6):
//! surfaces from wgpu's no-op backend, so no GPU and no window.

use std::sync::Arc;

use flui_engine::{
    EngineError, FrameLatency, RasterCommand, RasterExecutor, RasterSender, RasterVerdict,
    SendReceipt, SlotChange, SlotConfig, SlotFailure, SlotId, SlotPhase, SlotTarget,
    SurfaceFactory, UnconfiguredSurface,
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
    let backend = ScriptedSlots::new();
    let (executor, sender) = RasterExecutor::new(backend.clone(), || {});
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

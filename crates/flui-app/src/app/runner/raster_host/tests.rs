//! The raster host over the engine's scripted slots: what a window's frames,
//! surface changes and lent scenes ask of its slot (ADR-0305).

use std::sync::Arc;

use flui_engine::{
    DeviceHealth, EngineError, SurfaceHold,
    testing::{FakeWindow, ScriptedAnswer, ScriptedSlots, SlotCall},
};
use flui_foundation::{
    PresentationAddress, PresentationId, UiRuntimeId,
    geometry::{Offset, Rect},
};
use flui_layer::{DamageMode, DamageRegion, ScenePublication};
use flui_runtime::sink::SubmitVerdict;

use super::{RasterFactory, RasterHost, RestorePace, damage_mode_from};

const SIZE: (u32, u32) = (640, 480);

fn address() -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

/// A host over scripted slots with one window attached, unsized.
fn host() -> (RasterHost, ScriptedSlots) {
    let slots = ScriptedSlots::new();
    let mut host = RasterHost::new(RasterFactory::Scripted(slots.clone()));
    host.attach_target(
        Arc::new(FakeWindow::new(1)),
        address(),
        &crate::app::AppConfig::default(),
        None,
        None,
    )
    .expect("the scripted backend attaches any window");
    (host, slots)
}

/// The record's surface state of the window's slot.
fn hold(host: &RasterHost) -> SurfaceHold {
    let placed = host.placed.as_ref().expect("placed");
    let slot = placed.slots.get(&address()).expect("attached");
    placed.sender.record(slot.slot).expect("held").surface.hold
}

fn restores(slots: &ScriptedSlots) -> usize {
    slots
        .calls()
        .iter()
        .filter(|call| matches!(call, SlotCall::Restore(_)))
        .count()
}

fn renders(slots: &ScriptedSlots) -> Vec<DamageRegion> {
    slots
        .renders()
        .into_iter()
        .map(|(_, damage)| damage)
        .collect()
}

fn ten_by_ten_chunk(
    owner_id: usize,
    version: flui_layer::ContentVersion,
) -> Arc<flui_layer::Chunk> {
    let mut canvas = flui_painting::Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
    );
    Arc::new(flui_layer::Chunk::new(
        flui_foundation::RenderId::new(owner_id),
        0,
        version,
        canvas.finish(),
    ))
}

/// A retained scene: a root slotting one 10x10 boundary placed at `at`.
struct TestScene {
    owner: flui_layer::SceneOwner,
    child: flui_layer::SceneNodeId,
}

const PRODUCER_SURFACE: flui_layer::SurfaceKey = flui_layer::SurfaceKey {
    size: SIZE,
    generation: flui_foundation::SurfaceGeneration::ZERO,
};

impl TestScene {
    fn new(at: Offset<f64>) -> Self {
        use flui_layer::{BoundaryContent, SceneOp};
        let mut owner = flui_layer::SceneOwner::new();
        let child;
        {
            let mut writer = owner.writer();
            let root = writer.current().root();
            child = writer.mint_node();
            let version = writer.next_content_version().expect("a version");
            let mut leaf = BoundaryContent::builder();
            leaf.chunk(ten_by_ten_chunk(2, version));
            writer.push(SceneOp::Publish {
                node: child,
                content: Arc::new(leaf.finish()),
            });
            let mut top = BoundaryContent::builder();
            top.slot(child);
            writer.push(SceneOp::Publish {
                node: root,
                content: Arc::new(top.finish()),
            });
            writer.push(SceneOp::Place {
                node: child,
                placement: flui_layer::Placement::try_new(at).expect("finite"),
            });
        }
        owner.apply(PRODUCER_SURFACE).expect("the first version");
        Self { owner, child }
    }

    fn publication(&self) -> ScenePublication {
        self.owner.publication()
    }

    fn acknowledge(&mut self, id: flui_layer::SceneVersionId) {
        drop(self.owner.completed(Some(id)));
    }

    /// Republishes the child with fresh content at `at`.
    fn republish_child(&mut self, at: Offset<f64>, owner_id: usize) {
        use flui_layer::{BoundaryContent, SceneOp};
        {
            let mut writer = self.owner.writer();
            let version = writer.next_content_version().expect("a version");
            let mut leaf = BoundaryContent::builder();
            leaf.chunk(ten_by_ten_chunk(owner_id, version));
            writer.push(SceneOp::Publish {
                node: self.child,
                content: Arc::new(leaf.finish()),
            });
            writer.push(SceneOp::Place {
                node: self.child,
                placement: flui_layer::Placement::try_new(at).expect("finite"),
            });
        }
        self.owner.apply(PRODUCER_SURFACE).expect("a patch");
    }

    fn apply_empty_patch(&mut self) {
        let _ = self.owner.writer();
        self.owner.apply(PRODUCER_SURFACE).expect("an empty patch");
    }

    /// Submit the newest version and acknowledge it once shown.
    fn submit(&mut self, host: &mut RasterHost) -> SubmitVerdict {
        let publication = self.publication();
        let id = publication.scene.id();
        let verdict = host.submit(address(), SIZE, publication);
        if matches!(verdict, SubmitVerdict::Presented | SubmitVerdict::NoPresent) {
            self.acknowledge(id);
        }
        verdict
    }
}

const AT: Offset<f64> = Offset::new(100.0, 50.0);

/// A boundary that changed reaches the executor as the rectangle it covers;
/// only the window's first frame is full.
fn a_changed_boundary_reaches_the_backend_as_a_dirty_rect() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    scene.republish_child(AT, 3);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    let expected =
        flui_layer::DamageRect::try_covering(Rect::from_xywh(100.0, 50.0, 10.0, 10.0), SIZE)
            .expect("finite")
            .expect("on the surface");
    let renders = renders(&slots);
    assert!(matches!(renders[0], DamageRegion::Full(_)), "{renders:?}");
    assert!(
        matches!(renders[1], DamageRegion::Region(region) if region.bounds() == expected),
        "{renders:?}"
    );
}

/// A version with nothing changed since the one shown reaches the executor
/// as unchanged, for the backend to skip.
fn an_unchanged_scene_reaches_the_backend_unchanged() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    scene.apply_empty_patch();
    assert!(matches!(
        scene.publication().damage,
        DamageRegion::Unchanged
    ));
    let _ = scene.submit(&mut host);
    assert_eq!(renders(&slots)[1], DamageRegion::Unchanged);
}

/// `FLUI_DAMAGE` turns damage off with `off`, in any case and with spaces
/// around it, and nothing else does, a typo included.
fn the_damage_variable_selects_the_mode() {
    for off in ["off", "OFF", " Off "] {
        assert_eq!(damage_mode_from(Some(off)), DamageMode::Off, "{off:?}");
    }
    for on in [
        None,
        Some(""),
        Some("on"),
        Some("ON"),
        Some("of"),
        Some("0"),
    ] {
        assert_eq!(damage_mode_from(on), DamageMode::default(), "{on:?}");
    }
}

/// With damage off every frame reaches the executor full.
fn damage_off_sends_every_frame_full_and_retains_nothing() {
    let (mut host, slots) = host();
    host.placed
        .as_mut()
        .expect("placed")
        .slots
        .get_mut(&address())
        .expect("attached")
        .base
        .set_mode(DamageMode::Off);
    let mut scene = TestScene::new(AT);
    for frame in 0..3_usize {
        assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
        scene.republish_child(AT, frame + 10);
    }
    let renders = renders(&slots);
    assert_eq!(renders.len(), 3);
    assert!(
        renders
            .iter()
            .all(|damage| matches!(damage, DamageRegion::Full(_))),
        "{renders:?}"
    );
}

/// A publication whose base the window never showed cannot apply its
/// damage: it draws full.
fn a_base_mismatch_renders_full() {
    let (mut host, slots) = host();
    // The window shows a version of another lineage.
    let mut other = TestScene::new(AT);
    assert_eq!(other.submit(&mut host), SubmitVerdict::Presented);

    // The owner acknowledges a version the window never drew, then
    // publishes a patch against it.
    let mut scene = TestScene::new(AT);
    let first = scene.publication().scene.id();
    scene.acknowledge(first);
    scene.republish_child(AT, 20);
    let publication = scene.publication();
    assert_eq!(publication.base, Some(first));
    assert!(
        matches!(publication.damage, DamageRegion::Region(_)),
        "the owner's damage is a patch"
    );
    assert_eq!(
        host.submit(address(), SIZE, publication),
        SubmitVerdict::Presented
    );
    let renders = renders(&slots);
    assert!(
        matches!(renders[1], DamageRegion::Full(_)),
        "the mismatched base draws full, not {renders:?}"
    );
}

#[test]
fn raster_host_damage_matrix() {
    crate::table_test::run_table(
        "raster_host_damage_matrix",
        &[
            (
                "a_changed_boundary_reaches_the_backend_as_a_dirty_rect",
                a_changed_boundary_reaches_the_backend_as_a_dirty_rect as fn(),
            ),
            (
                "an_unchanged_scene_reaches_the_backend_unchanged",
                an_unchanged_scene_reaches_the_backend_unchanged as fn(),
            ),
            (
                "the_damage_variable_selects_the_mode",
                the_damage_variable_selects_the_mode as fn(),
            ),
            (
                "damage_off_sends_every_frame_full_and_retains_nothing",
                damage_off_sends_every_frame_full_and_retains_nothing as fn(),
            ),
            (
                "a_base_mismatch_renders_full",
                a_base_mismatch_renders_full as fn(),
            ),
        ],
    );
}

/// A surface the platform takes away is released before the call returns,
/// and the window's frames draw nothing and make no surface of their own
/// until the platform gives it back, when one is made at once.
///
/// Red-check: let a frame restore a released surface (drop `Released` from
/// `HostSlot::needs_surface`'s `available` gate) and the frame after the
/// release makes one over a window the platform has taken away.
#[test]
fn a_released_surface_waits_for_the_platform_and_comes_back_when_it_does() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);

    assert!(!host.set_surface(address(), false));
    assert_eq!(hold(&host), SurfaceHold::Released);
    assert!(
        slots
            .calls()
            .iter()
            .any(|call| matches!(call, SlotCall::Release(_))),
        "the release reached the backend before the call returned"
    );
    scene.republish_child(AT, 4);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::NotShown);
    assert_eq!(
        restores(&slots),
        0,
        "no surface while the platform has none"
    );
    assert_eq!(renders(&slots).len(), 1, "nothing is drawn while released");

    assert!(
        host.set_surface(address(), true),
        "a surface given back is on its way"
    );
    assert_eq!(restores(&slots), 1);
    assert_eq!(hold(&host), SurfaceHold::Held);
    scene.republish_child(AT, 5);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
}

/// A window whose native surface is not there yet (an Android `Resume`
/// before its `InitWindow`) is taken as released by the platform: its
/// frames do not ask the window for a surface until the platform reports
/// one.
///
/// Red-check: keep the window available after `SurfaceTargetUnavailable`
/// and every frame asks the window for its surface again.
#[test]
fn a_window_with_no_native_surface_waits_for_the_platform() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    let _ = host.set_surface(address(), false);
    let target = Arc::new(CountingNoSurface::default());
    host.placed
        .as_mut()
        .expect("placed")
        .slots
        .get_mut(&address())
        .expect("attached")
        .surface_target = Arc::clone(&target) as Arc<dyn flui_platform::SurfaceTarget>;
    assert!(!host.set_surface(address(), true));
    let asked = target.asked();
    assert!(asked > 0, "the platform's report asks the window once");
    for epoch in 0..3 {
        // A pace that has run out does not bring the attempt back either.
        host.placed
            .as_mut()
            .expect("placed")
            .slots
            .get_mut(&address())
            .expect("attached")
            .restore = RestorePace::default();
        scene.republish_child(AT, 6 + epoch);
        assert_eq!(scene.submit(&mut host), SubmitVerdict::NotShown);
    }
    assert_eq!(target.asked(), asked, "frames do not ask the window again");
    assert_eq!(restores(&slots), 0);
    assert_eq!(hold(&host), SurfaceHold::Released);
    assert_eq!(host.surface_retry_at(), None, "nothing to wake for");
}

/// A surface the backend refuses is asked for again at the window's pace,
/// not on every frame, and the pace is a wake deadline.
///
/// Red-check: drop the `due` check from `restore_surface` and the frame
/// right after the refusal sends another surface.
#[test]
fn a_refused_surface_is_asked_again_at_its_pace() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    let _ = host.set_surface(address(), false);
    slots.script_restore(ScriptedAnswer::Fail(EngineError::IncompatibleSurface));
    let refused_at = web_time::Instant::now();
    assert!(!host.set_surface(address(), true));
    assert_eq!(hold(&host), SurfaceHold::Refused);
    assert_eq!(restores(&slots), 1);

    scene.republish_child(AT, 7);
    assert_ne!(scene.submit(&mut host), SubmitVerdict::Presented);
    assert_eq!(restores(&slots), 1, "the next attempt waits out its pace");
    let due = host
        .surface_retry_at()
        .expect("the pace is a wake deadline");
    assert!(
        due > refused_at,
        "the next attempt is paced after the refusal"
    );

    // The pace has run out.
    host.placed
        .as_mut()
        .expect("placed")
        .slots
        .get_mut(&address())
        .expect("attached")
        .restore = RestorePace::default();
    scene.republish_child(AT, 8);
    let _ = scene.submit(&mut host);
    assert_eq!(restores(&slots), 2);
    assert_eq!(hold(&host), SurfaceHold::Held);
}

/// The window's return asks a device the executor gave up on to rebuild.
///
/// Red-check: drop the `RetryDevice` from `set_surface` and the given-up
/// device is not tried again: no second failure is published.
#[test]
fn a_window_given_back_retries_a_given_up_device() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    slots.cannot_rebuild_inline();
    slots.lose_device();
    host.service(address());
    let placed = host.placed.as_ref().expect("placed");
    assert!(matches!(
        placed.sender.device_health(),
        DeviceHealth::GivenUp { .. }
    ));
    assert!(placed.sender.take_device_failure().is_some());

    let _ = host.set_surface(address(), false);
    host.service(address());
    assert!(
        host.placed
            .as_ref()
            .expect("placed")
            .sender
            .take_device_failure()
            .is_none(),
        "a given-up device holds"
    );
    let _ = host.set_surface(address(), true);
    assert!(
        host.placed
            .as_ref()
            .expect("placed")
            .sender
            .take_device_failure()
            .is_some(),
        "the window's return tried the device again"
    );
}

/// A lent scene draws at the window's size outside its frames, and the
/// window's next frame draws in full: the screen shows a scene no
/// publication described.
///
/// Red-check: answer `false` from `render_lent` before it lends the scene
/// and no lent draw reaches the backend. The full repaint after it is the
/// executor's, red-checked by flui-engine's
/// `a_lent_scene_draws_once_and_the_next_frame_draws_in_full`.
#[test]
fn a_lent_scene_draws_and_the_next_frame_draws_in_full() {
    let (mut host, slots) = host();
    let mut scene = TestScene::new(AT);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    let lent = flui_layer::Scene::of_version(scene.publication().scene);
    assert!(host.render_lent(address(), SIZE, &lent, true));
    assert!(slots.calls().contains(&SlotCall::Lent(
        host.placed.as_ref().expect("placed").slots[&address()].slot,
        true
    )));
    scene.republish_child(AT, 9);
    assert_eq!(scene.submit(&mut host), SubmitVerdict::Presented);
    let renders = renders(&slots);
    assert!(
        matches!(renders.last(), Some(DamageRegion::Full(_))),
        "{renders:?}"
    );
}

/// A window whose native surface does not exist, counting the times it is
/// asked for one.
#[derive(Default)]
struct CountingNoSurface(std::sync::atomic::AtomicUsize);

impl raw_window_handle::HasWindowHandle for CountingNoSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl raw_window_handle::HasDisplayHandle for CountingNoSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl flui_platform::SurfaceTarget for CountingNoSurface {
    fn pre_present_notify(&self, _: flui_platform::SwapChainStatistics) {}
}

impl CountingNoSurface {
    fn asked(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

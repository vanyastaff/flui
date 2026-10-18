//! When a presentation asks its display for the highest refresh rate, and when
//! it lets go (ADR-0236): the demand the runtime decides and the edges it
//! publishes to the host through the owner's effects.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use flui_animation::AnimationController;
use flui_foundation::geometry::Offset;
use flui_foundation::{ManualClock, MonotonicClock, PresentationAddress};
use flui_interaction::events::{PointerKind, make_down_event, make_up_event};
use flui_platform_api::PlatformInput;
use flui_runtime::frame_rate_demand::{FRAME_RATE_HOLD, FrameRateDemand};
use flui_runtime::owner::{OwnerEffects, OwnerHost, PresentationDispatcher};
use flui_runtime::pump::SampledClock;
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

struct Sink;
impl flui_runtime::sink::FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (160, 80)
    }
    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

/// An owner whose frames pump the runtime at the manual clock's instant and
/// whose host records every demand edge it is told about.
struct Effects {
    clock: ManualClock,
    edges: RefCell<Vec<(PresentationAddress, FrameRateDemand)>>,
}

impl OwnerEffects for Effects {
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn finish_install(
        &self,
        _: PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn cancel_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) {
        panic!("no pending install");
    }
    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }
    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        let _ = runtime.pump(&mut SampledClock(self.clock.now()), &mut Sink);
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn frame_rate_demand_changed(&self, address: PresentationAddress, demand: FrameRateDemand) {
        self.edges.borrow_mut().push((address, demand));
    }
    fn request_continuation(&self) -> bool {
        true
    }
}

struct Rig {
    owner: OwnerHost,
    address: PresentationAddress,
    effects: Effects,
    window: PresentationDispatcher,
    controller: flui_animation::DrivenController,
    _native_window: Arc<dyn flui_platform_api::PlatformWindow>,
}

impl Rig {
    /// A mounted, resumed presentation on a clock the test advances, with one
    /// animation controller registered that has not started.
    fn new() -> Self {
        let clock = ManualClock::new();
        let window = crate::owner_publication::window();
        // A native host owns the window; the runtime holds it weakly.
        let native_window = Arc::clone(window.window());
        let runtime = UiRuntime::new(
            window,
            1.0,
            RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(AtomicBool::new(false)),
                Arc::new(flui_platform_api::InMemoryClipboard::new()),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("manual-clock runtime");
        runtime.synchronize_window_snapshot(
            runtime.presentation_id(),
            flui_platform_api::WindowExecutionState::Running,
            true,
            true,
        );
        runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
        runtime
            .attach_root_widget_with_size(&flui_widgets::SizedBox::new(100.0, 100.0), 100.0, 100.0)
            .expect("root");
        let controller =
            AnimationController::builder(Duration::from_secs(10)).build_on(Some(&runtime.vsync()));
        let owner = OwnerHost::new();
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        let window = owner.presentation_dispatcher(address).expect("window");
        let rig = Self {
            owner,
            address,
            effects: Effects {
                clock,
                edges: RefCell::new(Vec::new()),
            },
            window,
            controller,
            _native_window: native_window,
        };
        // Let the start-up geometry query settle, so the only deadline left is
        // the one under test.
        rig.advance(Duration::from_secs(1));
        rig.frame();
        assert_eq!(
            rig.next_wake()
                .map(|at| at.saturating_duration_since(rig.effects.clock.now())),
            None,
            "a quiet presentation has no deadline"
        );
        rig
    }

    fn frame(&self) {
        self.owner
            .frame_dispatcher(self.address)
            .expect("frame authority")
            .deliver(&self.effects)
            .expect("frame");
    }

    fn advance(&self, by: Duration) {
        self.effects.clock.advance(by);
    }

    fn touch(&self, down: bool) {
        let at = Offset::new(10.0, 10.0);
        let event = if down {
            make_down_event(at, PointerKind::Touch)
        } else {
            make_up_event(at, PointerKind::Touch)
        }
        .expect("a finite pointer event");
        self.window
            .input(PlatformInput::Pointer(event), &self.effects)
            .expect("input");
    }

    fn demands(&self) -> Vec<FrameRateDemand> {
        self.effects
            .edges
            .borrow()
            .iter()
            .map(|(address, demand)| {
                assert_eq!(*address, self.address, "the edge names its presentation");
                *demand
            })
            .collect()
    }

    fn next_wake(&self) -> Option<web_time::Instant> {
        self.owner.next_wake().expect("owner is idle")
    }

    fn pending_work(&self) -> bool {
        let seen = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&seen);
        self.window
            .test_callback(
                Box::new(move |runtime| *slot.borrow_mut() = Some(runtime.has_pending_work())),
                &self.effects,
            )
            .expect("read the runtime");
        seen.take().expect("read synchronously")
    }
}

fn animation_demand_raises_the_hint() {
    let rig = Rig::new();
    rig.frame();
    assert_eq!(rig.demands(), [], "a still screen asks for nothing");
    rig.controller.controller().forward().expect("start");
    rig.advance(Duration::from_millis(16));
    rig.frame();
    assert_eq!(rig.demands(), [FrameRateDemand::Active]);
    rig.advance(Duration::from_millis(16));
    rig.frame();
    assert_eq!(
        rig.demands(),
        [FrameRateDemand::Active],
        "a running animation publishes the edge once, not once per frame"
    );
}

fn active_pointer_contact_raises_the_hint() {
    let rig = Rig::new();
    rig.touch(true);
    assert_eq!(
        rig.demands(),
        [FrameRateDemand::Active],
        "a finger on the screen asks before any frame is produced"
    );
    rig.advance(FRAME_RATE_HOLD * 4);
    rig.frame();
    assert_eq!(
        rig.demands(),
        [FrameRateDemand::Active],
        "a contact that is still down never lets go"
    );
}

fn static_screen_releases_the_hint_after_the_hold_via_a_wake_deadline() {
    let rig = Rig::new();
    rig.touch(true);
    rig.advance(Duration::from_millis(40));
    rig.touch(false);
    let released_at = rig.effects.clock.now();
    assert_eq!(
        rig.demands(),
        [FrameRateDemand::Active],
        "the hold keeps the rate through the lift"
    );
    assert_eq!(
        rig.next_wake(),
        Some(released_at + FRAME_RATE_HOLD),
        "no frame will come on a static screen, so the hold is a wake deadline"
    );
    assert!(!rig.pending_work(), "the hold is not due yet");

    rig.advance(FRAME_RATE_HOLD.saturating_sub(Duration::from_millis(1)));
    assert!(!rig.pending_work(), "one millisecond short of the hold");
    rig.advance(Duration::from_millis(1));
    assert!(
        rig.pending_work(),
        "a due hold is work, or the frame closure skips and the loop spins"
    );
    rig.frame();
    assert_eq!(
        rig.demands(),
        [FrameRateDemand::Active, FrameRateDemand::Idle]
    );
    assert_eq!(rig.next_wake(), None, "the release leaves no deadline");
    assert!(!rig.pending_work());
}

#[test]
fn frame_rate_demand_contract() {
    crate::table_test::run_table(
        "frame_rate_demand_contract",
        &[
            (
                "animation_demand_raises_the_hint",
                animation_demand_raises_the_hint as fn(),
            ),
            (
                "active_pointer_contact_raises_the_hint",
                active_pointer_contact_raises_the_hint as fn(),
            ),
            (
                "static_screen_releases_the_hint_after_the_hold_via_a_wake_deadline",
                static_screen_releases_the_hint_after_the_hold_via_a_wake_deadline as fn(),
            ),
        ],
    );
}

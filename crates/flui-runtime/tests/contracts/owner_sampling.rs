//! Scheduling observations never execute an application's deadline query.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock, PresentationAddress, UiRuntimeId};
use flui_interaction::{arena::GestureArenaMember, ids::PointerId};
use flui_runtime::owner::{
    Delivery, InstallInitialization, InstallToken, InitializationOutcome, OwnerEffects, OwnerHost,
    RecoveryState,
};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

struct DeadlineQuery {
    calls: Rc<Cell<usize>>,
    due: web_time::Instant,
}

impl GestureArenaMember for DeadlineQuery {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}

    fn deadline(&self) -> Option<web_time::Instant> {
        self.calls.set(self.calls.get() + 1);
        Some(self.due)
    }
}

struct Effects {
    query_calls: Rc<Cell<usize>>,
    probe_runtime: Cell<bool>,
    runtime_observation: RefCell<Option<RuntimeObservation>>,
}

struct RuntimeObservation {
    queries_before: usize,
    queries_after: usize,
    wakes: Vec<Option<web_time::Instant>>,
    pending: Vec<bool>,
}

impl OwnerEffects for Effects {
    fn runtime_lifecycle(&self, _: UiRuntimeId, _: flui_scheduler::AppLifecycleState) {}

    fn runtimes_stopped(&self, _: RecoveryState) {}

    fn retire_host(&self, _: &[PresentationAddress], _: RecoveryState) {}

    fn commit_install(&self, _: InstallToken, _: RecoveryState) -> Option<InstallInitialization> {
        panic!("no pending installation");
    }

    fn finish_install(&self, _: PresentationAddress, _: InitializationOutcome, _: RecoveryState) {}

    fn cancel_install(&self, _: InstallToken, _: RecoveryState) {
        panic!("no pending installation");
    }

    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }

    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        if !self.probe_runtime.get() {
            return;
        }
        let queries_before = self.query_calls.get();
        let mut wakes = Vec::new();
        let mut pending = Vec::new();
        for _ in 0..3 {
            wakes.push(runtime.next_wake());
            pending.push(runtime.has_pending_work());
        }
        self.runtime_observation.replace(Some(RuntimeObservation {
            queries_before,
            queries_after: self.query_calls.get(),
            wakes,
            pending,
        }));
    }

    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}

    fn after_turn(&self, _: RecoveryState) {}

    fn request_continuation(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy)]
enum ObservationScope {
    Owner,
    Runtime,
}

fn scheduling_observation_case(scope: ObservationScope) {
    let clock = ManualClock::new();
    let due = clock.now() + Duration::from_secs(1);
    let runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock),
        ),
    )
    .expect("manual-clock runtime");
    let query_calls = Rc::new(Cell::new(0));
    let member = Rc::new(DeadlineQuery {
        calls: Rc::clone(&query_calls),
        due,
    });
    let _entry = runtime
        .gestures()
        .arena()
        .add(PointerId::new(NonZeroU64::MIN), &member);
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    let frame = owner.frame_dispatcher(address).expect("frame authority");
    let effects = Effects {
        query_calls: Rc::clone(&query_calls),
        probe_runtime: Cell::new(false),
        runtime_observation: RefCell::new(None),
    };

    // The completed owner operation may refresh its scheduling observation.
    // Getters must use that observation without executing the query again.
    assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
    match scope {
        ObservationScope::Owner => {
            let queries_before = query_calls.get();
            for _ in 0..3 {
                assert_eq!(owner.next_wake(), Ok(Some(due)));
            }
            assert_eq!(
                query_calls.get(),
                queries_before,
                "owner wake observation executed the external deadline query"
            );
        }
        ObservationScope::Runtime => {
            effects.probe_runtime.set(true);
            assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
            let observed = effects
                .runtime_observation
                .take()
                .expect("runtime getters observed during owner execution");
            assert_eq!(observed.wakes, [Some(due); 3]);
            assert_eq!(observed.pending, [true; 3]);
            assert_eq!(
                observed.queries_after, observed.queries_before,
                "runtime wake or pending observation executed the external deadline query"
            );
        }
    }
}

fn owner_wake_uses_the_committed_scheduling_observation() {
    scheduling_observation_case(ObservationScope::Owner);
}

fn runtime_wake_and_pending_use_the_committed_scheduling_observation() {
    scheduling_observation_case(ObservationScope::Runtime);
}

#[test]
fn owner_sampling_contract() {
    crate::table_test::run_table(
        "owner sampling",
        &[
            (
                "owner wake observation is inert",
                owner_wake_uses_the_committed_scheduling_observation,
            ),
            (
                "runtime scheduling observations are inert",
                runtime_wake_and_pending_use_the_committed_scheduling_observation,
            ),
        ],
    );
}

//! `InputTap` delivery (ADR-0215 §1): the records a runtime emits reach its
//! tap after each dispatch commits, every tap call is contained, losses travel
//! as `Gap` rows, and a recording is complete only once its `End` row is out.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::{IdExhausted, PresentationAddress};
use flui_interaction::{events::Code, testing::input::KeyEventBuilder};
use flui_platform_api::{PlatformInput, WindowExecutionState};
use flui_runtime::owner::{OwnerEffects, OwnerHost, PresentationDispatcher};
use flui_runtime::ui_runtime::UiRuntime;
use flui_runtime::{
    HostInput, InputRecord, InputTapHandle, InputTaps, RecordingIncomplete, TapFailureSite,
};

use crate::HostInputExt as _;

static_assertions::assert_not_impl_any!(InputTapHandle: Send, Sync);

struct Effects {
    continuations: Cell<usize>,
}

impl OwnerEffects for Effects {
    fn finish_install(
        &self,
        _: PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn frame(&self, _: PresentationAddress, _: &mut UiRuntime) {}
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
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
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn request_continuation(&self) -> bool {
        self.continuations.set(self.continuations.get() + 1);
        true
    }
}

/// A published runtime, its input authority and its tap registry.
struct Fixture {
    owner: Rc<RefCell<Option<OwnerHost>>>,
    input: PresentationDispatcher,
    effects: Rc<Effects>,
    taps: InputTaps,
}

impl Fixture {
    fn new() -> Self {
        let owner = OwnerHost::new();
        let runtime = crate::owner_publication::runtime();
        runtime.synchronize_window_snapshot(
            runtime.presentation_id(),
            WindowExecutionState::Running,
            true,
            true,
        );
        runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
        let taps = runtime.input_taps().clone();
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        let input = owner
            .presentation_dispatcher(address)
            .expect("input authority");
        Self {
            owner: Rc::new(RefCell::new(Some(owner))),
            input,
            effects: Rc::new(Effects {
                continuations: Cell::new(0),
            }),
            taps,
        }
    }

    fn key(&self, code: Code) {
        self.input
            .input(key(code), &*self.effects)
            .expect("the runtime admits the key");
    }
}

fn key(code: Code) -> PlatformInput {
    PlatformInput::Keyboard(KeyEventBuilder::new(code).build())
}

/// A row as these cases compare it.
fn describe(record: &InputRecord) -> String {
    match record {
        InputRecord::Input(row) => match &row.input {
            HostInput::Platform(PlatformInput::Keyboard(event)) => format!("key {:?}", event.code),
            other => format!("input {other:?}"),
        },
        InputRecord::Gap { rows } => format!("gap {rows}"),
        InputRecord::End { lost_rows } => format!("end {lost_rows}"),
        InputRecord::Decision(_) => "decision".to_owned(),
        InputRecord::Preferences(_) => "preferences".to_owned(),
        _ => "unknown".to_owned(),
    }
}

type Rows = Rc<RefCell<Vec<String>>>;

fn rows() -> Rows {
    Rc::new(RefCell::new(Vec::new()))
}

/// A tap that appends `prefix` and each row's description to `rows`.
fn writer(rows: &Rows, prefix: &'static str) -> impl Fn(&InputRecord) + 'static {
    let rows = Rc::clone(rows);
    move |record| {
        rows.borrow_mut()
            .push(format!("{prefix}{}", describe(record)));
    }
}

/// Drops with a panic, standing in for a tap capture whose destructor fails.
struct PanickingCapture(&'static str);

impl Drop for PanickingCapture {
    fn drop(&mut self) {
        std::panic::panic_any(self.0);
    }
}

fn a_tap_may_dispatch_reentrantly() {
    let fixture = Fixture::new();
    let recorded = rows();
    let reentrant = Cell::new(true);
    let reentrant = Rc::new(reentrant);
    let tap = {
        let recorded = Rc::clone(&recorded);
        let input = fixture.input.clone();
        let effects = Rc::clone(&fixture.effects);
        let reentrant = Rc::clone(&reentrant);
        move |record: &InputRecord| {
            recorded.borrow_mut().push(describe(record));
            if reentrant.get() && matches!(record, InputRecord::Input(_)) {
                // Every row admits another: only the owner's budget bounds it.
                let _ = input.input(key(Code::F2), &*effects);
            }
        }
    };
    let handle = fixture.taps.install(tap).expect("first registration");
    fixture.key(Code::F1);
    let seen = recorded.borrow().clone();
    assert_eq!(seen.first().map(String::as_str), Some("key F1"));
    assert!(
        seen.iter().skip(1).all(|row| row == "key F2"),
        "reentrant rows follow the row that admitted them: {seen:?}"
    );
    assert!(
        fixture.effects.continuations.get() > 0,
        "the dispatch returned and asked the owner for another turn"
    );
    reentrant.set(false);
    handle.finish().expect("complete recording");
    assert_eq!(recorded.borrow().last().map(String::as_str), Some("end 0"));
}

fn a_tap_replaced_mid_call_takes_effect_from_the_next_row() {
    let fixture = Fixture::new();
    let recorded = rows();
    let replacement: Rc<RefCell<Option<InputTapHandle>>> = Rc::new(RefCell::new(None));
    let first = {
        let recorded = Rc::clone(&recorded);
        let taps = fixture.taps.clone();
        let replacement = Rc::clone(&replacement);
        let second = writer(&recorded, "B:");
        let second = RefCell::new(Some(second));
        move |record: &InputRecord| {
            recorded
                .borrow_mut()
                .push(format!("A:{}", describe(record)));
            if let Some(second) = second.borrow_mut().take() {
                *replacement.borrow_mut() = Some(taps.install(second).expect("replacement"));
            }
        }
    };
    let stale = fixture.taps.install(first).expect("first registration");
    fixture.key(Code::F1);
    fixture.key(Code::F2);
    assert_eq!(*recorded.borrow(), ["A:key F1", "B:key F2"]);
    assert!(!stale.is_registered());
    drop(stale);
    let current = replacement.borrow_mut().take().expect("B registered");
    assert!(current.is_registered(), "a stale handle leaves the new tap");
    fixture.key(Code::F3);
    assert_eq!(*recorded.borrow(), ["A:key F1", "B:key F2", "B:key F3"]);
    current.finish().expect("complete recording");
}

fn a_panicking_tap_is_contained_and_the_first_failure_kept() {
    let fixture = Fixture::new();
    let recorded = rows();
    let tap = {
        let recorded = Rc::clone(&recorded);
        move |record: &InputRecord| {
            let row = describe(record);
            match row.as_str() {
                "key F1" => std::panic::panic_any("first tap failure"),
                "key F3" => std::panic::panic_any("later tap failure"),
                _ => recorded.borrow_mut().push(row),
            }
        }
    };
    let handle = fixture.taps.install(tap).expect("registration");
    fixture.key(Code::F1);
    fixture.key(Code::F2);
    fixture.key(Code::F3);
    assert!(handle.is_registered(), "the tap stays registered");
    let failure = fixture.taps.first_failure().expect("a contained failure");
    assert_eq!(failure.message.as_deref(), Some("first tap failure"));
    assert_eq!(failure.site, TapFailureSite::Row);
    handle.finish().expect("its gaps and end were delivered");
    assert_eq!(
        *recorded.borrow(),
        ["gap 1", "key F2", "gap 1", "end 2"],
        "each lost row is marked before the next one"
    );
}

fn tap_registration_refuses_at_generation_exhaustion() {
    let fixture = Fixture::new();
    fixture.taps.issue_generations_after(u64::MAX - 1);
    let last = fixture
        .taps
        .install(|_: &InputRecord| {})
        .expect("the terminal generation is issued once");
    for _ in 0..2 {
        assert_eq!(
            fixture.taps.install(|_: &InputRecord| {}).err(),
            Some(IdExhausted),
            "exhaustion is permanent, never a wrapped generation"
        );
    }
    assert!(last.is_registered(), "a refusal leaves the registered tap");
}

fn a_tap_that_installs_a_replacement_then_panics_retires_contained_and_the_new_tap_records_the_next_row()
 {
    let fixture = Fixture::new();
    let recorded = rows();
    let replacement: Rc<RefCell<Option<InputTapHandle>>> = Rc::new(RefCell::new(None));
    let outgoing = {
        let taps = fixture.taps.clone();
        let replacement = Rc::clone(&replacement);
        let incoming = RefCell::new(Some(writer(&recorded, "B:")));
        let capture = PanickingCapture("outgoing tap capture");
        move |_: &InputRecord| {
            let _keep = &capture;
            if let Some(incoming) = incoming.borrow_mut().take() {
                *replacement.borrow_mut() = Some(taps.install(incoming).expect("replacement"));
            }
            std::panic::panic_any("outgoing tap failure");
        }
    };
    let stale = fixture.taps.install(outgoing).expect("registration");
    fixture.key(Code::F1);
    fixture.key(Code::F2);
    let failure = fixture.taps.first_failure().expect("a contained failure");
    assert_eq!(
        (failure.site, failure.message.as_deref()),
        (TapFailureSite::Row, Some("outgoing tap failure")),
        "the capture's destructor did not replace the first failure"
    );
    assert_eq!(*recorded.borrow(), ["B:gap 1", "B:key F2"]);
    drop(stale);
    replacement
        .borrow_mut()
        .take()
        .expect("the incoming tap")
        .finish()
        .expect("complete recording");
    assert_eq!(
        recorded.borrow().last().map(String::as_str),
        Some("B:end 1")
    );
}

fn releasing_the_owner_during_a_tap_call_finishes_cleanly() {
    let fixture = Fixture::new();
    let recorded = rows();
    let tap = {
        let recorded = Rc::clone(&recorded);
        let owner = Rc::clone(&fixture.owner);
        move |record: &InputRecord| {
            recorded.borrow_mut().push(describe(record));
            drop(owner.borrow_mut().take());
        }
    };
    let handle = fixture.taps.install(tap).expect("registration");
    fixture.key(Code::F1);
    assert!(
        fixture.owner.borrow().is_none(),
        "the tap released the owner"
    );
    handle
        .finish()
        .expect("the registry outlives its owner's release");
    assert_eq!(*recorded.borrow(), ["key F1", "end 0"]);
}

fn a_panic_on_the_final_row_still_writes_its_gap() {
    let fixture = Fixture::new();
    let recorded = rows();
    let gap_failures = Rc::new(Cell::new(1));
    let tap = {
        let recorded = Rc::clone(&recorded);
        let gap_failures = Rc::clone(&gap_failures);
        move |record: &InputRecord| {
            let row = describe(record);
            if row == "key F2" {
                std::panic::panic_any("final row failure");
            }
            if matches!(record, InputRecord::Gap { .. }) && gap_failures.get() > 0 {
                // The immediate gap call fails too: the count stays pending.
                gap_failures.set(gap_failures.get() - 1);
                std::panic::panic_any("gap failure");
            }
            recorded.borrow_mut().push(row);
        }
    };
    let handle = fixture.taps.install(tap).expect("registration");
    fixture.key(Code::F1);
    fixture.key(Code::F2);
    assert_eq!(*recorded.borrow(), ["key F1"]);
    handle
        .finish()
        .expect("finishing offers the pending gap once more");
    assert_eq!(*recorded.borrow(), ["key F1", "gap 1", "end 1"]);
}

fn a_tap_that_fails_at_finish_reports_an_incomplete_recording() {
    // Refusing the end row.
    let fixture = Fixture::new();
    let handle = fixture
        .taps
        .install(|record: &InputRecord| {
            if matches!(record, InputRecord::End { .. }) {
                std::panic::panic_any("end failure");
            }
        })
        .expect("registration");
    fixture.key(Code::F1);
    assert_eq!(
        handle.finish(),
        Err(RecordingIncomplete { lost_rows: 0 }),
        "an undelivered end row"
    );
    assert!(!fixture.taps.is_recording(), "finishing unregistered it");

    // Refusing every gap: the end row is never offered after a lost gap.
    let fixture = Fixture::new();
    let recorded = rows();
    let tap = {
        let recorded = Rc::clone(&recorded);
        move |record: &InputRecord| {
            if matches!(record, InputRecord::Input(_) | InputRecord::Gap { .. }) {
                std::panic::panic_any("lost");
            }
            recorded.borrow_mut().push(describe(record));
        }
    };
    let handle = fixture.taps.install(tap).expect("registration");
    fixture.key(Code::F1);
    assert_eq!(handle.finish(), Err(RecordingIncomplete { lost_rows: 1 }));
    assert!(
        recorded.borrow().is_empty(),
        "no end row claims completeness"
    );
}

/// A tap that finishes its own recording inside a call ends the pass it is
/// part of: the rows that pass has not offered yet are lost, and the final
/// rows say so instead of claiming none were.
fn finishing_inside_a_tap_call_counts_the_rows_it_skips() {
    use flui_runtime::owner::WindowObservation;

    let runtime = Rc::new(crate::owner_publication::runtime());
    let address = PresentationAddress {
        ui_runtime_id: runtime.id(),
        presentation_id: runtime.presentation_id(),
    };
    let observe = move |runtime: &UiRuntime, observation| {
        runtime.apply_window_observation(address, observation, &|_, _| {});
    };
    let recorded = rows();
    let handle: Rc<RefCell<Option<InputTapHandle>>> = Rc::new(RefCell::new(None));
    let calls = Rc::new(Cell::new(0));
    let tap = {
        let recorded = Rc::clone(&recorded);
        let runtime = Rc::downgrade(&runtime);
        let handle = Rc::clone(&handle);
        let calls = Rc::clone(&calls);
        move |record: &InputRecord| {
            recorded.borrow_mut().push(describe(record));
            calls.set(calls.get() + 1);
            let runtime = runtime.upgrade().expect("the runtime outlives its tap");
            match calls.get() {
                // Two rows admitted during the first pass wait for the next.
                1 => {
                    for visible in [false, true] {
                        observe(&runtime, WindowObservation::Visibility(visible));
                    }
                }
                // The first row of that next pass stops the recording.
                2 => {
                    let handle = handle.borrow_mut().take().expect("registered");
                    assert_eq!(
                        handle.finish(),
                        Ok(()),
                        "the gap and the end row were delivered"
                    );
                }
                _ => {}
            }
        }
    };
    *handle.borrow_mut() = Some(runtime.input_taps().install(tap).expect("registration"));
    observe(&runtime, WindowObservation::Focus(true));
    // Its pass holds the two waiting rows and this one.
    observe(&runtime, WindowObservation::Hover(true));
    let seen = recorded.borrow().clone();
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert!(
        seen[..2].iter().all(|row| row.starts_with("input")),
        "{seen:?}"
    );
    assert_eq!(seen[2..], ["gap 2", "end 2"], "{seen:?}");
    assert!(
        !runtime.input_taps().is_recording(),
        "finishing unregistered it"
    );
}

/// An assistive request no handler ran for is recorded as unhandled, as a
/// key is, so a replay tells a dropped action from one that ran.
fn an_assistive_request_no_handler_ran_is_recorded_unhandled() {
    use flui_runtime::owner::InputOutcome;

    let runtime = crate::owner_publication::runtime();
    let outcomes = Rc::new(RefCell::new(Vec::new()));
    let tap = {
        let outcomes = Rc::clone(&outcomes);
        move |record: &InputRecord| {
            if let InputRecord::Input(row) = record
                && matches!(row.input, HostInput::Accessibility(_))
            {
                outcomes.borrow_mut().push(row.outcome);
            }
        }
    };
    let handle = runtime.input_taps().install(tap).expect("registration");
    let handled = runtime.dispatch_accessibility(
        runtime.presentation_id(),
        &accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(u64::MAX),
            data: None,
        },
    );
    assert!(!handled, "no node was ever published");
    handle.finish().expect("complete recording");
    assert_eq!(*outcomes.borrow(), [InputOutcome::Unhandled]);
}

#[test]
fn input_tap_delivery_contract() {
    crate::table_test::run_table(
        "input_tap_delivery_contract",
        &[
            (
                "a_tap_may_dispatch_reentrantly",
                a_tap_may_dispatch_reentrantly as fn(),
            ),
            (
                "a_tap_replaced_mid_call_takes_effect_from_the_next_row",
                a_tap_replaced_mid_call_takes_effect_from_the_next_row,
            ),
            (
                "a_panicking_tap_is_contained_and_the_first_failure_kept",
                a_panicking_tap_is_contained_and_the_first_failure_kept,
            ),
            (
                "tap_registration_refuses_at_generation_exhaustion",
                tap_registration_refuses_at_generation_exhaustion,
            ),
            (
                "a_tap_that_installs_a_replacement_then_panics_retires_contained_and_the_new_tap_records_the_next_row",
                a_tap_that_installs_a_replacement_then_panics_retires_contained_and_the_new_tap_records_the_next_row,
            ),
            (
                "releasing_the_owner_during_a_tap_call_finishes_cleanly",
                releasing_the_owner_during_a_tap_call_finishes_cleanly,
            ),
            (
                "a_panic_on_the_final_row_still_writes_its_gap",
                a_panic_on_the_final_row_still_writes_its_gap,
            ),
            (
                "a_tap_that_fails_at_finish_reports_an_incomplete_recording",
                a_tap_that_fails_at_finish_reports_an_incomplete_recording,
            ),
            (
                "finishing_inside_a_tap_call_counts_the_rows_it_skips",
                finishing_inside_a_tap_call_counts_the_rows_it_skips,
            ),
            (
                "an_assistive_request_no_handler_ran_is_recorded_unhandled",
                an_assistive_request_no_handler_ran_is_recorded_unhandled,
            ),
        ],
    );
}

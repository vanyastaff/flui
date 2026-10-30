//! Cancelled presentation attempts release closed numeric answer custody.

use std::{
    cell::Cell,
    num::NonZeroU64,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use flui_foundation::{ManualClock, TextSize};
use flui_interaction::gesture::GestureArenaMember;
use flui_painting::{TextSizing, TextSizingAdmissionError};
use flui_runtime::{
    sink::{FrameSink, SubmitVerdict},
    ui_runtime::{RuntimeHostServices, TextSizingSettlement, UiRuntime},
};
use flui_view::ViewExt as _;

struct Sink;

impl FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

struct FailingQuery(Rc<Cell<usize>>);

impl GestureArenaMember for FailingQuery {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}

    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
        self.0.set(self.0.get() + 1);
        std::panic::panic_any("captured attempt deadline query");
    }
}

fn abandoned_numeric_answers_are_released(reject_sampling: bool) {
    let mut clock = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime over actual headless capabilities");
    let (_policy, mut admission) = TextSizing::captured();
    admission.set_warm_capacity(0);
    let source = admission.source();
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), source.clone()));
    runtime
        .attach_root_widget_with_size(
            &flui_widgets::Column::new(vec![
                flui_widgets::Text::new("first captured answer")
                    .style(flui_painting::typography::TextStyle::default().with_font_size(16.0))
                    .boxed(),
                flui_widgets::Text::new("second missing answer")
                    .style(flui_painting::typography::TextStyle::default().with_font_size(20.0))
                    .boxed(),
            ]),
            400.0,
            400.0,
        )
        .expect("mount text with two distinct numeric requirements");
    let mut sink = Sink;
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    let mut initial = runtime.take_text_sizing_frontiers();
    assert_eq!(initial.len(), 1, "one actual preparation frontier");
    let initial = initial.pop().expect("missing first answer");
    let request = *initial
        .requests()
        .first()
        .expect("a real authored size request");
    let accepted = TextSize::try_new(request.size.value() * 1.5).expect("finite first answer");
    let replacement = TextSize::try_new(accepted.value() + 1.0).expect("distinct finite answer");
    admission
        .admit([(request, accepted)])
        .expect("admit first actual answer");
    assert!(runtime.settle_text_sizing(initial, TextSizingSettlement::Ready));
    clock.advance(Duration::from_millis(20));
    assert!(
        !runtime.pump(&mut clock, &mut sink).presented(),
        "the second missing answer retains this presentation attempt"
    );
    let mut pending = runtime.take_text_sizing_frontiers();
    assert_eq!(
        pending.len(),
        1,
        "the same attempt retains another frontier"
    );
    let pending = pending.pop().expect("second authored size remains missing");
    assert!(
        !pending.requests().contains(&request),
        "the attempt retained its first answer after the warm cache was disabled"
    );
    assert!(
        matches!(
            admission.admit([(request, replacement)]),
            Err(TextSizingAdmissionError::Conflict(_))
        ),
        "a live attempt still protects the admitted answer"
    );
    if reject_sampling {
        let calls = Rc::new(Cell::new(0));
        let member = Rc::new(FailingQuery(Rc::clone(&calls)));
        let entry = runtime.gestures().arena().add(
            flui_interaction::PointerId::new(NonZeroU64::new(99).expect("contact identity")),
            &member,
        );
        clock.advance(Duration::from_millis(20));
        assert!(!runtime.pump(&mut clock, &mut sink).presented());
        assert_eq!(
            calls.get(),
            1,
            "the real sampling query rejected the retained attempt"
        );
        assert!(
            !runtime.settle_text_sizing(pending, TextSizingSettlement::Ready),
            "the withdrawn receipt cannot revive its cancelled cohort"
        );
        drop(entry);
        drop(member);
    } else {
        assert!(runtime.withdraw_captured_text_sizing_for(runtime.presentation_id(), &source));
        assert!(
            !runtime.settle_text_sizing(pending, TextSizingSettlement::Ready),
            "ordinary withdrawal also revokes the old receipt"
        );
    }
    // The sink retains no scene. Destroy every render/text cache before probing
    // the source again, so only an abandoned cohort can still protect this value.
    drop(runtime);
    assert!(
        admission.admit([(request, replacement)]).is_ok(),
        "a cancelled numeric cohort must release its answer after all geometry retires"
    );
}

fn sampling_rejection_releases_abandoned_answers() {
    abandoned_numeric_answers_are_released(true);
}

fn ordinary_withdrawal_releases_abandoned_answers() {
    abandoned_numeric_answers_are_released(false);
}

#[test]
fn cancelled_text_attempt_releases_numeric_answers() {
    crate::table_test::run_table(
        "cancelled numeric text answers",
        &[
            (
                "sampling_rejection_releases_abandoned_answers",
                sampling_rejection_releases_abandoned_answers,
            ),
            (
                "ordinary_withdrawal_releases_abandoned_answers",
                ordinary_withdrawal_releases_abandoned_answers,
            ),
        ],
    );
}

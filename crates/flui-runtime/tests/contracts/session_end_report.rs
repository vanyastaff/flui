//! How a runner reports a panic its session end contained: the report's
//! subscriber is user code and is contained too, so the session end goes on
//! to its next step and the platform gets its answer.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_runtime::owner_turn::SessionEndStep;

/// A log subscriber that fails on every event, as a faulty one does.
struct FailsOnEveryEvent;

impl tracing::Subscriber for FailsOnEveryEvent {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {
        panic!("log subscriber failed");
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// Logging the panic of each step to a subscriber that fails returns: the
/// report does not unwind into the session end that called it.
///
/// Fails against a report that logs uncontained.
#[test]
fn a_step_report_contains_its_subscriber() {
    let _diagnostics = crate::scoped_diagnostics(FailsOnEveryEvent);
    for step in [
        SessionEndStep::Query,
        SessionEndStep::Notice,
        SessionEndStep::Close,
        SessionEndStep::Flush,
    ] {
        let payload: Box<dyn std::any::Any + Send> = Box::new("a step panicked");
        assert!(
            catch_unwind(AssertUnwindSafe(|| step.log(payload))).is_ok(),
            "{step:?}: the report returns"
        );
    }
}

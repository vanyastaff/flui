//! The `flui.<subsystem>` targets through the real `EnvFilter`: a directive selects a target and
//! everything under it.

use std::sync::{Arc, Mutex};

use flui_log::FilterConfig;
use tracing::Level;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::{Layer, Registry};

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<&'static str>>>);

impl<S: tracing::Subscriber> Layer<S> for Seen {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        struct Nothing;
        impl Visit for Nothing {
            fn record_debug(&mut self, _: &Field, _: &dyn core::fmt::Debug) {}
        }
        event.record(&mut Nothing);
        self.0
            .lock()
            .expect("BUG: the recorder's lock is never poisoned")
            .push(event.metadata().target());
    }
}

/// The targets of the framework, plus two the framework does not own, each emitting one TRACE
/// event, that `directives` admits.
fn admitted(directives: &str) -> Vec<&'static str> {
    let filter = FilterConfig::new(directives)
        .without_env_var()
        .env_filter()
        .expect("the directives parse");
    let seen = Seen::default();
    let subscriber = Registry::default().with(seen.clone().with_filter(filter));
    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(target: "flui.frame", Level::TRACE, "x");
        tracing::event!(target: "flui.frame.submit", Level::TRACE, "x");
        tracing::event!(target: "flui.tree", Level::TRACE, "x");
        tracing::event!(target: "flui.tree.reconcile", Level::TRACE, "x");
        tracing::event!(target: "flui.animation", Level::TRACE, "x");
        tracing::event!(target: "flui.animation.diagnostics", Level::TRACE, "x");
        tracing::event!(target: "flui.signals", Level::TRACE, "x");
        tracing::event!(target: "flui.a11y", Level::TRACE, "x");
        tracing::event!(target: "flui.input", Level::TRACE, "x");
        tracing::event!(target: "an_app", Level::TRACE, "x");
        tracing::event!(target: "flui_animation::controller", Level::TRACE, "x");
    });
    let mut out = seen
        .0
        .lock()
        .expect("BUG: the recorder's lock is never poisoned")
        .clone();
    out.sort_unstable();
    out
}

#[test]
fn a_contract_target_selects_itself_and_what_is_under_it() {
    assert_eq!(
        admitted("off,flui.frame=trace"),
        ["flui.frame", "flui.frame.submit"]
    );
    assert_eq!(
        admitted("off,flui.tree=trace"),
        ["flui.tree", "flui.tree.reconcile"]
    );
    assert_eq!(
        admitted("off,flui.tree.reconcile=trace"),
        ["flui.tree.reconcile"]
    );
    // A level on a parent target is a default the child's own directive overrides.
    assert_eq!(
        admitted("off,flui.animation=info,flui.animation.diagnostics=trace"),
        ["flui.animation.diagnostics"]
    );
}

#[test]
fn other_directives_are_left_alone() {
    assert_eq!(admitted("off"), Vec::<&str>::new());
    assert_eq!(admitted("off,an_app=trace"), ["an_app"]);
    // A module path is the target of an event that names none, and it is selected as before.
    assert_eq!(
        admitted("off,flui_animation=trace"),
        ["flui_animation::controller"]
    );
}

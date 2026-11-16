//! The composition root's own capability scope (ADR-0336): it acquires from
//! the run's registry with no window, runs what providers post back when the
//! owner asks, contains a panicking job, and refuses posts once retired.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::capability::{
    Abandoned, Capability, CapabilityRegistrar, OwnerPoster, OwnerRetired, ProviderContext,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_runtime::capability::{CapabilityRegistry, HostCapabilities};

/// A capability whose handle is a greeting: what these cases acquire.
enum Greeting {}

impl Capability for Greeting {
    type Handle = String;
    const NAME: &'static str = "greeting";
}

/// What the provider saw and kept on each acquisition.
#[derive(Default)]
struct Seen {
    had_window: Cell<Option<bool>>,
    poster: RefCell<Option<OwnerPoster>>,
}

/// A scope over a registry whose greeting provider records into `seen`,
/// with a wake that counts.
fn scope(seen: &Rc<Seen>) -> (HostCapabilities, Arc<AtomicUsize>) {
    let mut registrar = CapabilityRegistrar::new();
    let recorded = Rc::clone(seen);
    registrar.provide::<Greeting>(move |ctx: &ProviderContext<'_>| {
        recorded.had_window.set(Some(ctx.window().is_some()));
        recorded.poster.replace(Some(ctx.owner().clone()));
        Ok("hello".to_owned())
    });
    let registry = CapabilityRegistry::builder()
        .built_ins(registrar)
        .build()
        .expect("one provider");
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let scope = HostCapabilities::new(registry, move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    (scope, wakes)
}

/// The host acquires with no window, and a capability nothing provides is
/// refused as not registered, naming it.
fn a_host_scope_acquires_with_no_window() {
    let seen = Rc::default();
    let (scope, _) = scope(&seen);
    assert_eq!(scope.acquire::<Greeting>().as_deref(), Ok("hello"));
    assert_eq!(
        seen.had_window.get(),
        Some(false),
        "no window in the context"
    );

    enum Absent {}
    impl Capability for Absent {
        type Handle = ();
        const NAME: &'static str = "absent";
    }
    let refused: Unsupported = scope.acquire::<Absent>().expect_err("nothing provides it");
    assert_eq!(refused.reason, UnsupportedReason::NotRegistered);
    assert_eq!(refused.capability(), Some("absent"));
}

/// A provider's post wakes the owner and runs at `run_posted`, not inline;
/// a job that panics leaves the jobs after it to run, and the first panic is
/// returned.
fn a_host_scope_contains_a_panicking_job() {
    let seen = Rc::default();
    let (scope, wakes) = scope(&seen);
    let _ = scope.acquire::<Greeting>();
    let poster = seen
        .poster
        .borrow()
        .clone()
        .expect("the provider kept the poster");
    let ran = Arc::new(AtomicUsize::new(0));
    poster
        .post(|| panic!("a provider's job panics"))
        .expect("the scope takes the post");
    let counted = Arc::clone(&ran);
    poster
        .post(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })
        .expect("the scope takes the post");
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "each post woke the owner");
    assert_eq!(ran.load(Ordering::SeqCst), 0, "not run inline");
    assert!(scope.has_posted());

    let first = scope.run_posted();
    assert!(first.held().is_some(), "the panic is handed to the owner");
    flui_foundation::panic::discard_payload(first.into_payload().expect("held"));
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the job after it ran");
    assert!(!scope.has_posted());
}

/// Once retired, a post is refused, so a provider learns its owner is gone,
/// and a completion still pending ends as abandoned by its owner.
fn a_host_scope_refuses_posts_after_retirement() {
    let seen = Rc::default();
    let (scope, _) = scope(&seen);
    let _ = scope.acquire::<Greeting>();
    let poster = seen
        .poster
        .borrow()
        .clone()
        .expect("the provider kept the poster");
    let completions = {
        let mut registrar = CapabilityRegistrar::new();
        let kept = Rc::new(RefCell::new(None));
        let keep = Rc::clone(&kept);
        registrar.provide::<Greeting>(move |ctx: &ProviderContext<'_>| {
            keep.replace(Some(ctx.completions().pending::<u32>()));
            Ok(String::new())
        });
        let registry = CapabilityRegistry::builder()
            .built_ins(registrar)
            .build()
            .expect("one provider");
        let pending = HostCapabilities::new(registry, || {});
        let _ = pending.acquire::<Greeting>();
        let (completion, resolver) = kept.take().expect("the provider made a completion");
        assert!(!pending.retire().held().is_some(), "a quiet retirement");
        (pending, completion, resolver)
    };
    let (_pending, mut completion, _resolver) = completions;
    assert_eq!(completion.poll(), Some(Err(Abandoned::OwnerRetired)));

    assert!(!scope.retire().held().is_some());
    assert_eq!(poster.post(|| {}), Err(OwnerRetired));
    assert!(!scope.has_posted(), "a refused job is not queued");
}

#[test]
fn host_capabilities_contract() {
    crate::table_test::run_table(
        "host_capabilities_contract",
        &[
            (
                "a_host_scope_acquires_with_no_window",
                a_host_scope_acquires_with_no_window,
            ),
            (
                "a_host_scope_contains_a_panicking_job",
                a_host_scope_contains_a_panicking_job,
            ),
            (
                "a_host_scope_refuses_posts_after_retirement",
                a_host_scope_refuses_posts_after_retirement,
            ),
        ],
    );
}

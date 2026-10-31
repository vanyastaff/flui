//! The seam's owner-thread result type and poster (ADR-0152 §2, items 5 and
//! 6): what settles when, and what ends at retirement.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use flui_platform::{
    Unsupported, UnsupportedReason,
    capability::{
        Abandoned, Capability, CapabilityProvider, CompletionSource, OwnerJob, OwnerPoster,
        OwnerRetired, OwnerScope, PendingCompletions,
    },
};

/// A hand-run owner: posted jobs queue until [`Owner::turn`].
struct Owner {
    queue: Arc<Mutex<Option<Vec<OwnerJob>>>>,
    pending: PendingCompletions,
    source: CompletionSource,
}

impl Owner {
    fn new() -> Self {
        let queue = Arc::new(Mutex::new(Some(Vec::new())));
        let sink = Arc::clone(&queue);
        let poster = OwnerPoster::new(move |job| match sink.lock().expect("queue").as_mut() {
            Some(jobs) => {
                jobs.push(job);
                Ok(())
            }
            None => Err(OwnerRetired),
        });
        let pending = PendingCompletions::new();
        let source = CompletionSource::new(poster, pending.clone());
        Self {
            queue,
            pending,
            source,
        }
    }

    /// One owner turn: run what was posted before it began.
    fn turn(&self) {
        let jobs = std::mem::take(self.queue.lock().expect("queue").as_mut().expect("live"));
        let scope = OwnerScope::new(&self.pending);
        for job in jobs {
            job.run(&scope);
        }
    }

    /// The owner retires: queued jobs are dropped unrun, then pending
    /// completions end.
    fn retire(&self) {
        drop(self.queue.lock().expect("queue").take());
        if let Some(payload) = self.pending.retire() {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Records that it was dropped, to tell "dropped unrun" from "never dropped".
struct DropFlag(Rc<Cell<bool>>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[test]
fn a_resolved_completion_settles_at_the_next_owner_turn() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<u32>();
    let seen = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&seen);
    completion.on_complete(move |outcome| *sink.borrow_mut() = Some(outcome));

    resolver.resolve(7).expect("owner live");
    assert_eq!(
        *seen.borrow(),
        None,
        "resolve never runs the closure inline"
    );
    owner.turn();
    assert_eq!(*seen.borrow(), Some(Ok(7)));
    assert!(owner.pending.is_empty());
}

#[test]
fn a_result_from_another_thread_reaches_the_owner_through_the_poster() {
    let owner = Owner::new();
    let (mut completion, resolver) = owner.source.pending::<String>();
    std::thread::spawn(move || resolver.resolve("from the OS".to_owned()))
        .join()
        .expect("OS thread")
        .expect("owner live");
    assert_eq!(completion.poll(), None, "not settled before an owner turn");
    owner.turn();
    assert_eq!(completion.poll(), Some(Ok("from the OS".to_owned())));
    assert_eq!(completion.poll(), None, "a completion settles once");
}

#[test]
fn a_dropped_resolver_ends_its_completion() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<u32>();
    let seen = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&seen);
    completion.on_complete(move |outcome| *sink.borrow_mut() = Some(outcome));
    drop(resolver);
    owner.turn();
    assert_eq!(*seen.borrow(), Some(Err(Abandoned::ProviderDropped)));

    let (mut polled, resolver) = owner.source.pending::<u32>();
    drop(resolver);
    owner.turn();
    assert_eq!(polled.poll(), Some(Err(Abandoned::ProviderDropped)));
}

#[test]
fn retirement_ends_pending_completions_and_refuses_posts() {
    let owner = Owner::new();
    let (waiting, waiting_resolver) = owner.source.pending::<u32>();
    let ran = Rc::new(Cell::new(false));
    let dropped = Rc::new(Cell::new(false));
    let (flag, ran_in) = (DropFlag(Rc::clone(&dropped)), Rc::clone(&ran));
    waiting.on_complete(move |_| {
        let _flag = &flag;
        ran_in.set(true);
    });
    let (mut polled, _polled_resolver) = owner.source.pending::<u32>();
    // Answered, but the owner retires before the turn that would settle it.
    waiting_resolver.resolve(1).expect("owner live");

    owner.retire();
    assert!(
        dropped.get(),
        "the waiting closure is dropped at retirement"
    );
    assert!(!ran.get(), "and never runs");
    assert_eq!(polled.poll(), Some(Err(Abandoned::OwnerRetired)));
    assert_eq!(polled.poll(), None);

    let posted_ran = Arc::new(Mutex::new(false));
    let posted = Arc::clone(&posted_ran);
    assert_eq!(
        owner
            .source
            .poster()
            .post(move || *posted.lock().expect("flag") = true),
        Err(OwnerRetired)
    );
    assert!(!*posted_ran.lock().expect("flag"));

    let (mut late, late_resolver) = owner.source.pending::<u32>();
    assert_eq!(late.poll(), Some(Err(Abandoned::OwnerRetired)));
    assert_eq!(late_resolver.resolve(2), Err(OwnerRetired));
}

#[test]
fn a_completion_settled_before_retirement_keeps_its_value() {
    let owner = Owner::new();
    let (mut completion, resolver) = owner.source.pending::<u32>();
    resolver.resolve(5).expect("owner live");
    owner.turn();
    owner.retire();
    assert_eq!(completion.poll(), Some(Ok(5)));
}

#[test]
fn waiting_after_the_value_settled_still_runs_at_a_later_turn() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<u32>();
    resolver.resolve(3).expect("owner live");
    owner.turn();
    let seen = Rc::new(Cell::new(None));
    let sink = Rc::clone(&seen);
    completion.on_complete(move |outcome| sink.set(Some(outcome)));
    assert_eq!(seen.get(), None, "not inside on_complete");
    owner.turn();
    assert_eq!(seen.get(), Some(Ok(3)));
}

#[test]
fn the_future_adapter_wakes_when_the_value_settles() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<u32>();
    let mut future = pin!(completion.into_future());
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(future.as_mut().poll(&mut cx), Poll::Pending);
    resolver.resolve(9).expect("owner live");
    owner.turn();
    assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(9)));
}

enum Gauge {}

impl Capability for Gauge {
    type Handle = ();
    const NAME: &'static str = "gauge";
}

// The provider trait stays dyn-compatible.
const _: fn(&dyn CapabilityProvider<Gauge>) = |_| {};

#[test]
fn a_refusal_names_its_capability() {
    let refusal = Unsupported::of::<Gauge>(UnsupportedReason::NotOnThisPlatform);
    assert_eq!(refusal.capability(), Some("gauge"));
    assert_eq!(
        refusal.to_string(),
        "unsupported: gauge: not on this platform"
    );
    assert_eq!(
        Unsupported::new(UnsupportedReason::NotRegistered).capability(),
        None
    );
}

/// A leaf capability crate's whole surface, written against the contract
/// alone: its marker, its provider, its plugin, and an acquisition through
/// what the plugin registered.
mod leaf {
    use flui_platform::capability::{Capability, CapabilityRegistrar, Plugin, ProviderContext};
    use flui_platform::{Unsupported, UnsupportedReason};

    pub(super) enum Vibration {}

    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct VibrationHandle {
        pub(super) motors: u8,
    }

    impl Capability for Vibration {
        type Handle = VibrationHandle;
        const NAME: &'static str = "vibration";
    }

    pub(super) struct VibrationPlugin {
        pub(super) present: bool,
    }

    impl Plugin for VibrationPlugin {
        fn register(&self, registrar: &mut CapabilityRegistrar) {
            let present = self.present;
            registrar.provide::<Vibration>(move |_: &ProviderContext<'_>| {
                if present {
                    Ok(VibrationHandle { motors: 2 })
                } else {
                    Err(Unsupported::of::<Vibration>(
                        UnsupportedReason::NotOnThisDevice,
                    ))
                }
            });
        }
    }
}

#[test]
fn a_leaf_crate_declares_registers_and_acquires_on_the_contract_alone() {
    use leaf::{Vibration, VibrationHandle, VibrationPlugin};

    let owner = Owner::new();
    let acquire = |present: bool| {
        let mut registrar = flui_platform::capability::CapabilityRegistrar::new();
        flui_platform::capability::Plugin::register(&VibrationPlugin { present }, &mut registrar);
        let entries = registrar.into_entries();
        assert_eq!(entries.len(), 1, "one provider registered");
        assert_eq!(entries[0].name(), Vibration::NAME);
        assert_eq!(entries[0].capability(), std::any::TypeId::of::<Vibration>());
        let locals = flui_platform::capability::RuntimeLocals::new();
        let cx = flui_platform::capability::ProviderContext::new(
            owner.source.poster(),
            &owner.source,
            &locals,
            None,
        );
        entries[0].provide(&cx).map(|handle| {
            *handle
                .downcast::<VibrationHandle>()
                .expect("the provider answers its capability's handle")
        })
    };
    assert_eq!(acquire(true), Ok(VibrationHandle { motors: 2 }));
    let refusal = acquire(false).expect_err("absent");
    assert_eq!(refusal.capability(), Some("vibration"));
    assert_eq!(refusal.reason, UnsupportedReason::NotOnThisDevice);
}

/// Panics when dropped, as a capture with a faulty `Drop` does.
struct PanicOnDrop(&'static str);

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("{}", self.0);
    }
}

/// Counts its wakes.
struct CountingWaker(std::sync::atomic::AtomicUsize);

impl std::task::Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn a_capture_panicking_at_retirement_leaves_the_others_retired() {
    let owner = Owner::new();
    let (first, _first_answer) = owner.source.pending::<u32>();
    let capture = PanicOnDrop("capture dropped at retirement");
    first.on_complete(move |_| {
        let _ = &capture;
    });
    let (polled, _polled_answer) = owner.source.pending::<u32>();
    let (last, _last_answer) = owner.source.pending::<u32>();
    let dropped = Rc::new(Cell::new(false));
    let flag = DropFlag(Rc::clone(&dropped));
    last.on_complete(move |_| {
        let _ = &flag;
    });

    let woken = Arc::new(CountingWaker(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&woken));
    let mut future = pin!(polled.into_future());
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    );

    let payload = owner
        .pending
        .retire()
        .expect("the capture's panic is handed back");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("capture dropped at retirement")
    );
    assert_eq!(
        woken.0.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a later completion's future is still woken"
    );
    assert!(dropped.get(), "a later closure is still dropped unrun");
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(Abandoned::OwnerRetired))
    );
}

/// A panic payload whose own destructor panics.
struct PanickingPayload;

impl Drop for PanickingPayload {
    fn drop(&mut self) {
        panic!("payload dropped");
    }
}

/// Panics with a [`PanickingPayload`] when dropped.
struct PanicWithPanickingPayload;

impl Drop for PanicWithPanickingPayload {
    fn drop(&mut self) {
        std::panic::panic_any(PanickingPayload);
    }
}

#[test]
fn a_second_retirement_panic_is_retained_not_dropped() {
    let owner = Owner::new();
    let (first, _first_answer) = owner.source.pending::<u32>();
    let capture = PanicOnDrop("first capture dropped at retirement");
    first.on_complete(move |_| {
        let _ = &capture;
    });
    let (second, _second_answer) = owner.source.pending::<u32>();
    let panicking = PanicWithPanickingPayload;
    second.on_complete(move |_| {
        let _ = &panicking;
    });

    let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.pending.retire()))
        .expect("a later payload is not dropped inside the retirement");
    let payload = retired.expect("the first capture's panic is handed back");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("first capture dropped at retirement")
    );
}

#[test]
fn an_undrained_answer_is_kept_when_its_capture_panics_at_retirement() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<PanicOnDrop>();
    let capture = PanicOnDrop("capture dropped at retirement");
    completion.on_complete(move |_| {
        let _ = &capture;
    });
    resolver
        .resolve(PanicOnDrop("undrained answer dropped"))
        .expect("the owner is live");

    let payload = owner
        .pending
        .retire()
        .expect("the capture's panic is handed back");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("capture dropped at retirement")
    );
}

#[test]
fn an_undrained_answer_panicking_at_retirement_is_handed_back() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<PanicOnDrop>();
    completion.on_complete(|_| {});
    resolver
        .resolve(PanicOnDrop("undrained answer dropped"))
        .expect("the owner is live");

    let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.pending.retire()))
        .expect("the answer's panic does not escape the retirement");
    let payload = retired.expect("the answer's panic is handed back");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("undrained answer dropped")
    );
}

thread_local! {
    /// What a [`ReenterOnDrop`] waker runs when its last clone drops.
    static ON_WAKER_DROP: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

/// A waker whose destructor runs user code, as an executor's may.
struct ReenterOnDrop;

#[expect(
    clippy::manual_noop_waker,
    reason = "the waker's Drop re-enters the owner; Waker::noop has none"
)]
impl std::task::Wake for ReenterOnDrop {
    fn wake(self: Arc<Self>) {}
}

impl Drop for ReenterOnDrop {
    fn drop(&mut self) {
        if let Some(hook) = ON_WAKER_DROP.with(|hook| hook.borrow_mut().take()) {
            hook();
        }
    }
}

#[test]
fn a_replaced_waker_may_reenter_the_owner_when_it_drops() {
    let owner = Rc::new(Owner::new());
    let (completion, resolver) = owner.source.pending::<u32>();
    let mut future = pin!(completion.into_future());

    let reentering = Waker::from(Arc::new(ReenterOnDrop));
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&reentering)),
        Poll::Pending
    );
    // The completion now holds the last clone.
    drop(reentering);
    resolver.resolve(5).expect("owner live");
    let turning = Rc::clone(&owner);
    ON_WAKER_DROP.with(|hook| *hook.borrow_mut() = Some(Box::new(move || turning.turn())));

    let woken = Arc::new(CountingWaker(std::sync::atomic::AtomicUsize::new(0)));
    let next = Waker::from(Arc::clone(&woken));
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&next)),
        Poll::Ready(Ok(5)),
        "the answer settles in the replaced waker's drop, and this poll reads it"
    );
    assert!(
        ON_WAKER_DROP.with(|hook| hook.borrow().is_none()),
        "the replaced waker dropped"
    );
}

#[test]
fn a_panicking_callback_keeps_a_panicking_capture_and_the_owner_goes_on() {
    let owner = Owner::new();
    let (completion, resolver) = owner.source.pending::<u32>();
    let capture = PanicOnDrop("capture dropped during the callback's unwind");
    completion.on_complete(move |_| {
        let _ = &capture;
        panic!("callback");
    });
    resolver.resolve(1).expect("owner live");

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.turn()))
        .expect_err("the callback's panic reaches the owner");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"callback"));

    let (next, answer) = owner.source.pending::<u32>();
    let seen = Rc::new(Cell::new(None));
    let sink = Rc::clone(&seen);
    next.on_complete(move |outcome| sink.set(Some(outcome)));
    answer.resolve(2).expect("owner live");
    owner.turn();
    assert_eq!(seen.get(), Some(Ok(2)), "the owner keeps settling");
}

#[test]
fn a_panicking_posted_callback_keeps_a_panicking_capture() {
    let owner = Owner::new();
    let capture = PanicOnDrop("capture dropped during the posted callback's unwind");
    owner
        .source
        .poster()
        .post(move || {
            let _ = &capture;
            panic!("posted callback");
        })
        .expect("owner live");

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.turn()))
        .expect_err("the callback's panic reaches the owner");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"posted callback"));
}

thread_local! {
    /// What a [`cloning_waker`] runs the first time it is cloned.
    static ON_WAKER_CLONE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

/// A waker whose clone runs [`ON_WAKER_CLONE`], as an executor's clone may
/// re-enter the owner. Safe `Wake` cannot customise clone.
#[expect(
    unsafe_code,
    reason = "exercise an executor's clone through the public Future API"
)]
fn cloning_waker() -> Waker {
    use std::task::{RawWaker, RawWakerVTable};

    unsafe fn clone(data: *const ()) -> RawWaker {
        if let Some(hook) = ON_WAKER_CLONE.with(|hook| hook.borrow_mut().take()) {
            hook();
        }
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn noop(_: *const ()) {}
    // The data pointer is a dangling constant that nothing dereferences, so
    // clones and drops have nothing to count.
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: no vtable function dereferences `data`, and each is safe to call
    // from any thread any number of times.
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

#[test]
fn an_answer_settled_while_the_waker_is_cloned_is_read_by_that_poll() {
    let owner = Rc::new(Owner::new());
    let (completion, resolver) = owner.source.pending::<u32>();
    let mut future = pin!(completion.into_future());
    resolver.resolve(8).expect("owner live");
    let turning = Rc::clone(&owner);
    ON_WAKER_CLONE.with(|hook| *hook.borrow_mut() = Some(Box::new(move || turning.turn())));

    let waker = cloning_waker();
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(8)),
        "the clone settled the answer; this poll must not return Pending with no wake to come"
    );
}

/// A runtime-local slot that holds whatever the test puts in it.
struct Slot<T>(RefCell<Option<T>>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

#[test]
fn runtime_locals_keep_one_value_per_type_until_retirement() {
    use flui_platform::capability::RuntimeLocals;

    let locals = RuntimeLocals::new();
    let flags = locals.get_or_default::<Slot<DropFlag>>();
    assert!(
        Rc::ptr_eq(&flags, &locals.get_or_default::<Slot<DropFlag>>()),
        "every ask of one type is answered by one value"
    );
    let dropped = Rc::new(Cell::new(false));
    flags.0.replace(Some(DropFlag(Rc::clone(&dropped))));
    drop(flags);
    locals
        .get_or_default::<Slot<PanicOnDrop>>()
        .0
        .replace(Some(PanicOnDrop("a slot dropped at retirement")));

    let failure = locals
        .retire()
        .expect("the faulty slot's panic is returned");
    assert_eq!(
        failure.downcast_ref::<String>().map(String::as_str),
        Some("a slot dropped at retirement")
    );
    assert!(dropped.get(), "the other slot is dropped too");
    let after = locals.get_or_default::<Slot<DropFlag>>();
    assert!(
        !Rc::ptr_eq(&after, &locals.get_or_default::<Slot<DropFlag>>()),
        "a retired runtime keeps nothing"
    );
}

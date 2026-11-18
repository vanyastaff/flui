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
        Abandoned, Activity, ActivityBridge, Capability, CapabilityId, CapabilityProvider,
        CapabilityRegistrar, CompletionSource, DataRefused, HostBridge, IntentFrame,
        InvalidResultKey, JniFrame, MAX_RESULT_KEY_LEN, MainThreadRefused, OwnerJob, OwnerPoster,
        OwnerRetired, OwnerScope, PendingCompletions, PostedWork, ProviderContext, RequestKey,
        Resolver, ResultCode, ResultIntent, ResultKey, ResultMint, ResultOutcome, ResultRefused,
        ResultRequest, ResultTicketHold, ResultToken, ResultTokenMint, RuntimeLocals,
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
fn an_owner_local_answer_settles_through_its_resolver() {
    let owner = Owner::new();
    // `Rc` is not `Send`: its resolver stays on the owner, and needs no more.
    let (completion, resolver) = owner.source.pending::<Rc<u32>>();
    let seen = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&seen);
    completion.on_complete(move |outcome| *sink.borrow_mut() = Some(outcome));

    resolver.resolve(Rc::new(7)).expect("owner live");
    assert_eq!(
        *seen.borrow(),
        None,
        "resolve never runs the closure inline"
    );
    owner.turn();
    assert_eq!(*seen.borrow(), Some(Ok(Rc::new(7))));
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
    let mut ctx = Context::from_waker(Waker::noop());
    assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);
    resolver.resolve(9).expect("owner live");
    owner.turn();
    assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Ok(9)));
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
        let host = flui_platform::capability::HostBridge::new();
        let ctx = flui_platform::capability::ProviderContext::new(
            owner.source.poster(),
            &owner.source,
            &locals,
            &host,
            None,
        );
        entries[0].provide(&ctx).map(|handle| {
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

    let woken = flui_foundation::testing::CountingWaker::new().shared();
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
        woken.count(),
        1,
        "a later completion's future is still woken"
    );
    assert!(dropped.get(), "a later closure is still dropped unrun");
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(Abandoned::OwnerRetired))
    );
}

#[test]
fn a_second_retirement_panic_does_not_replace_the_first() {
    let owner = Owner::new();
    let (first, _first_answer) = owner.source.pending::<u32>();
    let capture = PanicOnDrop("first capture dropped at retirement");
    first.on_complete(move |_| {
        let _ = &capture;
    });
    let (second, _second_answer) = owner.source.pending::<u32>();
    let panicking = PanicOnDrop("second capture dropped at retirement");
    second.on_complete(move |_| {
        let _ = &panicking;
    });

    let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.pending.retire()))
        .expect("a later panic does not escape the retirement");
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

/// A waker capture whose destructor runs user code, as an executor's may.
struct ReenterOnDrop;

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

    let reentering = flui_foundation::testing::CountingWaker::new()
        .shared()
        .retiring_waker(|| {}, ReenterOnDrop);
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&reentering)),
        Poll::Pending
    );
    // The completion now holds the last clone.
    drop(reentering);
    resolver.resolve(5).expect("owner live");
    let turning = Rc::clone(&owner);
    ON_WAKER_DROP.with(|hook| *hook.borrow_mut() = Some(Box::new(move || turning.turn())));

    let woken = flui_foundation::testing::CountingWaker::new().shared();
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

/// A host's side of the Activity that records what a provider asked of it
/// and answers each launch with a pending request, made with the mint the
/// composition root handed it.
#[derive(Default)]
struct BridgeCalls {
    tokens: ResultTokenMint,
    mint: RefCell<Option<ResultMint>>,
    launched: RefCell<Vec<(RequestKey, ResultToken)>>,
    resolvers: RefCell<Vec<Resolver<ResultOutcome>>>,
    tickets: RefCell<Vec<ResultTicketHold>>,
    /// The work handed to Android's main thread, with its refusal, in order.
    posted: RefCell<Vec<PostedWork>>,
}

impl BridgeCalls {
    fn mint(&self) -> ResultMint {
        self.mint.borrow().clone().expect("installed")
    }
}

impl ActivityBridge for BridgeCalls {
    fn bind(&self, mint: ResultMint) {
        self.mint.replace(Some(mint));
    }

    fn launch_for_result(
        &self,
        completions: &CompletionSource,
        _capability: &CapabilityId,
        key: RequestKey,
        _intent: &mut dyn FnMut(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused> {
        let token = self
            .tokens
            .next()
            .map_err(|_| ResultRefused::TokensExhausted)?;
        let (completion, resolver) = completions.pending();
        self.launched.borrow_mut().push((key, token));
        self.resolvers.borrow_mut().push(resolver);
        let (ticket, hold) = self.mint().ticket(token, Rc::new(|| {}));
        self.tickets.borrow_mut().push(hold);
        Ok(self.mint().request(ticket, completion))
    }

    fn reattach(
        &self,
        _completions: &CompletionSource,
        _capability: &CapabilityId,
        _key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused> {
        Err(ResultRefused::ActivityEnded)
    }

    fn take_data(
        &self,
        _capability: &CapabilityId,
        _data: ResultIntent,
        _f: &mut dyn FnMut(JniFrame),
    ) -> Result<(), DataRefused> {
        Err(DataRefused::NotHeld)
    }

    fn lend(&self, _f: &mut dyn FnMut(JniFrame)) -> bool {
        false
    }

    fn post(&self, work: PostedWork) {
        self.posted.borrow_mut().push(work);
    }
}

/// A capability whose handle is the Activity its provider reached.
enum Files {}

impl Capability for Files {
    type Handle = Option<Activity>;
    const NAME: &'static str = "files";
}

/// The same, under a name that could not key requests.
enum Spaced {}

impl Capability for Spaced {
    type Handle = Option<Activity>;
    const NAME: &'static str = "not a key";
}

/// A provider reaches the Activity the host offers through its context,
/// scoped by the identity the registrar minted for it, and none once the host
/// takes it back; a context not handed through a registration reaches none,
/// so no caller names another capability by its string. A launch through it
/// is answered by the completion of the token it was minted, at an owner
/// turn, and its ticket's drop tells the host.
#[test]
fn a_provider_reaches_the_activity_the_host_offers() {
    const PICK: ResultKey = match ResultKey::try_new("flui-test.pick") {
        Ok(key) => key,
        Err(_) => panic!("BUG: a printable key"),
    };
    let owner = Owner::new();
    let locals = RuntimeLocals::new();
    let host = HostBridge::new();
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<Files>(|ctx: &ProviderContext<'_>| Ok(ctx.activity()));
    registrar.provide::<Spaced>(|ctx: &ProviderContext<'_>| Ok(ctx.activity()));
    let entries = registrar.into_entries();
    let context = ProviderContext::new(owner.source.poster(), &owner.source, &locals, &host, None);
    let reach = |entry: usize| -> Option<Activity> {
        *entries[entry]
            .provide(&context)
            .expect("provided")
            .downcast::<Option<Activity>>()
            .expect("the handle")
    };
    assert!(reach(0).is_none(), "no host offers one yet");

    let bridge = Rc::new(BridgeCalls::default());
    let offered: Rc<dyn ActivityBridge> = Rc::clone(&bridge) as Rc<dyn ActivityBridge>;
    assert!(host.install_activity(Some(offered)).is_none());
    assert!(
        context.activity().is_none(),
        "a context no registration scoped reaches nothing"
    );
    assert!(
        reach(1).is_none(),
        "a name with a space cannot key requests"
    );
    let activity = reach(0).expect("the host offers its Activity");
    let request = activity
        .launch_for_result(&owner.source, PICK, |_| true)
        .expect("taken");
    let (ticket, mut completion) = request.into_parts();
    let token = ticket.token();
    let files = ResultKey::try_new("files").expect("a key");
    assert_eq!(
        *bridge.launched.borrow(),
        [(RequestKey::new(files, PICK), token)]
    );
    assert!(
        activity.with_java(|_| ()).is_none(),
        "this host lends nothing"
    );

    let resolver = bridge.resolvers.borrow_mut().pop().expect("one open");
    let answer = bridge.mint().result(ResultCode::from_raw(-1), None);
    assert!(resolver.resolve(Ok(answer)).is_ok());
    assert!(completion.poll().is_none(), "settles at the owner's turn");
    owner.turn();
    match completion.poll() {
        Some(Ok(Ok(result))) => assert_eq!(result.code(), ResultCode::Ok),
        other => panic!("the answer: {other:?}"),
    }
    assert!(
        !bridge.tickets.borrow()[0].is_released(),
        "the provider holds its ticket"
    );
    drop(ticket);
    assert!(
        bridge.tickets.borrow()[0].is_released(),
        "dropping the ticket tells the host"
    );

    assert!(
        host.install_activity(None).is_some(),
        "the host takes it back"
    );
    assert!(reach(0).is_none());
}

/// Work a provider hands to Android's main thread goes through the host,
/// and its completion settles at an owner turn, once: with what the work
/// answered, with the host's refusal, or, for work the host dropped without
/// either, as a hand-off that failed.
#[test]
fn work_for_the_main_thread_answers_through_the_host() {
    let owner = Owner::new();
    let locals = RuntimeLocals::new();
    let host = HostBridge::new();
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<Files>(|ctx: &ProviderContext<'_>| Ok(ctx.activity()));
    let entries = registrar.into_entries();
    let context = ProviderContext::new(owner.source.poster(), &owner.source, &locals, &host, None);
    let bridge = Rc::new(BridgeCalls::default());
    drop(host.install_activity(Some(Rc::clone(&bridge) as Rc<dyn ActivityBridge>)));
    let activity = entries[0]
        .provide(&context)
        .expect("provided")
        .downcast::<Option<Activity>>()
        .expect("the handle")
        .expect("the host offers its Activity");
    let nothing = std::ptr::NonNull::dangling();
    let frame = || JniFrame::new(nothing, nothing, nothing);

    let mut ran = activity.on_main_thread(&owner.source, |_frame| 7_u32);
    let mut refused = activity.on_main_thread(&owner.source, |_frame| 8_u32);
    let mut dropped = activity.on_main_thread(&owner.source, |_frame| 9_u32);
    let mut posted = bridge.posted.take().into_iter();
    posted
        .next()
        .expect("the first work reached the host")
        .run(frame());
    posted
        .next()
        .expect("the second work reached the host")
        .refuse(MainThreadRefused::ActivityEnded);
    drop(posted.next().expect("the third work reached the host"));
    assert!(ran.poll().is_none(), "settles at the owner's turn");
    owner.turn();
    assert_eq!(ran.poll(), Some(Ok(Ok(7))));
    assert_eq!(
        refused.poll(),
        Some(Ok(Err(MainThreadRefused::ActivityEnded)))
    );
    assert_eq!(
        dropped.poll(),
        Some(Ok(Err(MainThreadRefused::HandOffFailed))),
        "work a host drops unended is refused, never left waiting"
    );
}

/// A result key is a short printable name, checked where it is written; a
/// result code reads Android's two named values and keeps every other.
#[test]
fn result_keys_and_codes_hold_their_rules() {
    let long: &'static str = "k".repeat(MAX_RESULT_KEY_LEN + 1).leak();
    for (name, expected) in [
        ("flui-system.open-document", Ok("flui-system.open-document")),
        ("", Err(InvalidResultKey::Empty)),
        (long, Err(InvalidResultKey::TooLong)),
        ("with space", Err(InvalidResultKey::NotPrintable)),
        ("\u{e9}t\u{e9}", Err(InvalidResultKey::NotPrintable)),
    ] {
        assert_eq!(
            ResultKey::try_new(name).map(ResultKey::as_str),
            expected,
            "{name:?}"
        );
    }
    for (raw, code) in [
        (-1, ResultCode::Ok),
        (0, ResultCode::Canceled),
        (1, ResultCode::Other(1)),
        (-7, ResultCode::Other(-7)),
    ] {
        assert_eq!(ResultCode::from_raw(raw), code);
        assert_eq!(code.raw(), raw);
    }
}

#[test]
fn a_window_closes_once_and_tells_only_the_watches_still_held() {
    use flui_platform::capability::WindowCloser;

    let (closer, lifetime) = WindowCloser::new();
    assert!(!lifetime.is_closed());
    let heard = Rc::new(Cell::new(0_u32));
    let count = |heard: &Rc<Cell<u32>>| {
        let heard = Rc::clone(heard);
        move || heard.set(heard.get() + 1)
    };
    let kept = lifetime.on_close(count(&heard));
    let observer = lifetime.clone();
    let reads_closed = Rc::new(Cell::new(false));
    let read = Rc::clone(&reads_closed);
    let reader = lifetime.on_close(move || read.set(observer.is_closed()));
    drop(lifetime.on_close(count(&heard)));

    closer.close();
    assert_eq!(heard.get(), 1, "only the held watch is told");
    assert!(reads_closed.get(), "a listener reads the window as closed");
    assert!(lifetime.is_closed());
    closer.close();
    assert_eq!(heard.get(), 1, "a window closes once");

    let late = lifetime.on_close(count(&heard));
    drop(closer);
    assert_eq!(heard.get(), 1, "a watch taken after the close never runs");
    drop((kept, reader, late));

    let (closer, lifetime) = WindowCloser::new();
    let watch = lifetime.on_close(count(&heard));
    drop(closer);
    assert!(lifetime.is_closed(), "a dropped closer closes its window");
    assert_eq!(heard.get(), 2);
    drop(watch);
}

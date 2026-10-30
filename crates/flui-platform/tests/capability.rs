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
        self.pending.retire();
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

enum Probe {}

impl Capability for Probe {
    type Handle = ();
    const NAME: &'static str = "probe";
}

// The provider trait stays dyn-compatible.
const _: fn(&dyn CapabilityProvider<Probe>) = |_| {};

#[test]
fn a_refusal_names_its_capability() {
    let refusal = Unsupported::of::<Probe>(UnsupportedReason::NotOnThisPlatform);
    assert_eq!(refusal.capability(), Some("probe"));
    assert_eq!(
        refusal.to_string(),
        "unsupported: probe: not on this platform"
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
        let cx = flui_platform::capability::ProviderContext::new(
            owner.source.poster(),
            &owner.source,
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

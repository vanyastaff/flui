use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    Delivery, InstallToken, OwnerEffects, OwnerHost, PreparedInstall, PublicationError,
    RecoveryState,
};
use flui_runtime::ui_runtime::UiRuntime;

use crate::owner_publication::{runtime, window};

static_assertions::assert_not_impl_any!(InstallToken: Clone, Copy, Send, Sync);

struct Effects {
    owner: OwnerHost,
    pending: RefCell<Vec<PreparedInstall>>,
    published: RefCell<Vec<PresentationAddress>>,
    trace: RefCell<Vec<&'static str>>,
    stop: bool,
    retired: Cell<usize>,
    cancellation_failures: Cell<usize>,
    recoveries: RefCell<Vec<RecoveryState>>,
    completion: Cell<RecoveryState>,
    native_retired: Rc<RefCell<Vec<PresentationAddress>>>,
    native_failure: Cell<bool>,
}

impl Effects {
    fn new(owner: &OwnerHost, stop: bool) -> Self {
        Self {
            owner: owner.clone(),
            pending: RefCell::new(Vec::new()),
            published: RefCell::new(Vec::new()),
            trace: RefCell::new(Vec::new()),
            stop,
            retired: Cell::new(0),
            cancellation_failures: Cell::new(0),
            recoveries: RefCell::new(Vec::new()),
            completion: Cell::new(RecoveryState::Healthy),
            native_retired: Rc::new(RefCell::new(Vec::new())),
            native_failure: Cell::new(false),
        }
    }
    fn stage(&self, mut prepared: PreparedInstall) -> InstallToken {
        let token = prepared.take_delivery().expect("one token");
        assert!(
            prepared.take_delivery().is_none(),
            "no second token can be minted"
        );
        self.pending.borrow_mut().push(prepared);
        token
    }
    fn take(&self, token: &InstallToken) -> PreparedInstall {
        let mut pending = self.pending.borrow_mut();
        let index = pending
            .iter()
            .position(|prepared| prepared.address() == token.address())
            .expect("complete bundle matches token");
        pending.remove(index)
    }
}

impl OwnerEffects for Effects {
    fn finish_install(
        &self,
        _: flui_foundation::PresentationAddress,
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
    fn frame(&self, address: PresentationAddress, _: &mut UiRuntime) {
        self.trace.borrow_mut().push("frame entered");
        for prepared in [
            self.owner
                .prepare_presentation(address, window())
                .expect("first sibling"),
            self.owner
                .prepare_presentation(address, window())
                .expect("second sibling"),
        ] {
            let token = self.stage(prepared);
            assert_eq!(
                self.owner.queue_install(token, self).expect("admit commit"),
                Delivery::Queued
            );
        }
        assert!(self.published.borrow().is_empty());
        if self.stop {
            self.owner.shutdown(self);
        }
        self.trace.borrow_mut().push("frame returned");
    }
    fn commit_install(
        &self,
        token: InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        let prepared = self.take(&token);
        let published = self
            .owner
            .publication(prepared)
            .expect("runtime checkout returned before registry commit")
            .commit();
        assert_eq!(published, token.address());
        self.published.borrow_mut().push(published);
        self.trace.borrow_mut().push("commit");
        Some(flui_runtime::owner::InstallInitialization {
            address: published,
            observations: Vec::new(),
            lifecycle: None,
        })
    }
    fn retire_host(&self, presentations: &[PresentationAddress], _: RecoveryState) {
        assert_eq!(self.owner.runtime_count(), 0);
        assert!(
            self.native_retired.borrow().is_empty(),
            "shutdown is one-shot"
        );
        self.native_retired
            .borrow_mut()
            .extend_from_slice(presentations);
        self.owner.shutdown(self);
        if self.native_failure.get() {
            std::panic::panic_any("native retirement");
        }
    }
    fn cancel_install(&self, token: InstallToken, recovery: RecoveryState) {
        assert!(
            !self.native_retired.borrow().is_empty(),
            "native routing withdrawn first"
        );
        self.recoveries.borrow_mut().push(recovery);
        assert_eq!(
            self.owner.runtime_count(),
            0,
            "logical admission was withdrawn before native cancellation"
        );
        let prepared = self.take(&token);
        drop(prepared);
        self.retired.set(self.retired.get() + 1);
        self.trace.borrow_mut().push("cancel");
        if let Some(remaining) = self.cancellation_failures.get().checked_sub(1) {
            self.cancellation_failures.set(remaining);
            std::panic::panic_any(if self.retired.get() == 1 {
                "first cancellation"
            } else {
                "second cancellation"
            });
        }
    }
    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, recovery: RecoveryState) {
        self.completion.set(recovery);
    }
    fn request_continuation(&self) -> bool {
        panic!("finite commits fit the callback");
    }
}

fn prepared_sibling_commits_follow_the_frame_in_one_fifo() {
    let owner = OwnerHost::new();
    let initial = owner
        .publication(owner.prepare_runtime(runtime()))
        .expect("initial runtime")
        .commit();
    let effects = Effects::new(&owner, false);
    owner
        .frame_dispatcher(initial)
        .expect("frame")
        .deliver(&effects)
        .expect("delivery");
    assert_eq!(
        *effects.trace.borrow(),
        ["frame entered", "frame returned", "commit", "commit"]
    );
    let published = effects.published.borrow();
    assert_eq!(published.len(), 2);
    assert_ne!(published[0], published[1]);
    for address in published.iter().copied() {
        owner
            .frame_dispatcher(address)
            .expect("each committed sibling is routable");
    }
    assert!(effects.pending.borrow().is_empty());
}

fn shutdown_cancels_every_pending_bundle_before_checkout_returns() {
    let owner = OwnerHost::new();
    let initial = owner
        .publication(owner.prepare_runtime(runtime()))
        .expect("initial runtime")
        .commit();
    let effects = Effects::new(&owner, true);
    owner
        .frame_dispatcher(initial)
        .expect("frame")
        .deliver(&effects)
        .expect("delivery");
    assert_eq!(
        *effects.trace.borrow(),
        ["frame entered", "cancel", "cancel", "frame returned"]
    );
    assert_eq!(effects.retired.get(), 2);
    assert!(effects.pending.borrow().is_empty());
    assert!(effects.published.borrow().is_empty());
    assert_eq!(owner.runtime_count(), 0);
}

fn foreign_owner_refusal_returns_the_same_token_for_retry() {
    let owner = OwnerHost::new();
    let other = OwnerHost::new();
    let effects = Effects::new(&owner, false);
    let token = effects.stage(owner.prepare_runtime(runtime()));
    let refused = other
        .queue_install(token, &effects)
        .expect_err("foreign host");
    assert_eq!(refused.error, PublicationError::ForeignOwner);
    assert_eq!(
        owner
            .queue_install(refused.token, &effects)
            .expect("retry exact owner"),
        Delivery::Driven
    );
    assert_eq!(owner.runtime_count(), 1);
    assert_eq!(other.runtime_count(), 0);
    assert!(effects.pending.borrow().is_empty());
}

fn cancellation_failures_preserve_the_first_and_finish_the_pending_tail() {
    for failures in [1, 2] {
        let owner = OwnerHost::new();
        let initial = owner
            .publication(owner.prepare_runtime(runtime()))
            .expect("initial runtime")
            .commit();
        let effects = Effects::new(&owner, true);
        effects.cancellation_failures.set(failures);
        let frame = owner.frame_dispatcher(initial).expect("frame");
        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frame.deliver(&effects)))
                .expect_err("cancellation failure resumes");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"first cancellation"));
        assert_eq!(effects.retired.get(), 2);
        assert!(effects.pending.borrow().is_empty());
        assert_eq!(
            *effects.recoveries.borrow(),
            [RecoveryState::Healthy, RecoveryState::PreservingFailure]
        );
        assert_eq!(effects.completion.get(), RecoveryState::PreservingFailure);
        assert_eq!(owner.runtime_count(), 0);
    }
}

fn native_shutdown_covers_all_presentations_and_preserves_its_failure() {
    struct Capture {
        native_retired: Rc<RefCell<Vec<PresentationAddress>>>,
        dropped: Rc<Cell<usize>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert_eq!(self.native_retired.borrow().len(), 3);
            self.dropped.set(self.dropped.get() + 1);
        }
    }
    for fail in [false, true] {
        let owner = OwnerHost::new();
        let effects = Effects::new(&owner, true);
        let dropped = Rc::new(Cell::new(0));
        let runtimes = [runtime(), runtime()];
        for runtime in &runtimes {
            let capture = Capture {
                native_retired: Rc::clone(&effects.native_retired),
                dropped: Rc::clone(&dropped),
            };
            runtime
                .owner_frame()
                .post_frame_handle()
                .schedule(move |_| drop(capture))
                .expect("capture");
        }
        let [first_runtime, second_runtime] = runtimes;
        let initial = owner
            .publication(owner.prepare_runtime(first_runtime))
            .expect("runtime")
            .commit();
        let sibling = owner
            .publication(
                owner
                    .prepare_presentation(initial, window())
                    .expect("sibling"),
            )
            .expect("publish sibling")
            .commit();
        let independent = owner
            .publication(owner.prepare_runtime(second_runtime))
            .expect("independent")
            .commit();
        effects.native_failure.set(fail);
        effects.cancellation_failures.set(usize::from(fail));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner
                .frame_dispatcher(initial)
                .expect("frame")
                .deliver(&effects)
        }));
        if fail {
            let failure = result.expect_err("native failure resumes after cancellation");
            assert_eq!(failure.downcast_ref::<&str>(), Some(&"native retirement"));
            assert_eq!(
                *effects.recoveries.borrow(),
                [RecoveryState::PreservingFailure; 2]
            );
        } else {
            assert_eq!(result.expect("healthy cleanup"), Ok(Delivery::Driven));
        }
        assert_eq!(
            *effects.native_retired.borrow(),
            [initial, sibling, independent]
        );
        assert!(effects.pending.borrow().is_empty());
        assert_eq!(dropped.get(), 2, "resident and leased runtime retired");
        owner.shutdown(&effects);
        assert_eq!(owner.runtime_count(), 0);
    }
}

#[test]
fn owner_registry_contract() {
    crate::table_test::run_table(
        "owner_registry_contract",
        &[
            (
                "native_shutdown_covers_all_presentations_and_preserves_its_failure",
                native_shutdown_covers_all_presentations_and_preserves_its_failure as fn(),
            ),
            (
                "cancellation_failures_preserve_the_first_and_finish_the_pending_tail",
                cancellation_failures_preserve_the_first_and_finish_the_pending_tail as fn(),
            ),
            (
                "prepared_sibling_commits_follow_the_frame_in_one_fifo",
                prepared_sibling_commits_follow_the_frame_in_one_fifo as fn(),
            ),
            (
                "shutdown_cancels_every_pending_bundle_before_checkout_returns",
                shutdown_cancels_every_pending_bundle_before_checkout_returns as fn(),
            ),
            (
                "foreign_owner_refusal_returns_the_same_token_for_retry",
                foreign_owner_refusal_returns_the_same_token_for_retry as fn(),
            ),
        ],
    );
}

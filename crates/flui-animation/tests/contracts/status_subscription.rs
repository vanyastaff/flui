use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, CurvedAnimation, Curves,
    FloatTween, MotionClock, ProxyAnimation, ReverseAnimation, StatusCallback, StatusSubscription,
    TweenAnimation, Vsync,
};
use flui_foundation::{Listenable, ListenerId, Notifier};

fn dropping_one_subscription_preserves_independent_sources_and_channels() {
    let registry = Vsync::new();
    let a = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let b = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let a_calls = Rc::new(Cell::new(0));
    let b_calls = Rc::new(Cell::new(0));
    let values = Rc::new(Cell::new(0));
    let a_subscription = a.controller().subscribe_status({
        let calls = Rc::clone(&a_calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    let _b_subscription = b.controller().subscribe_status({
        let calls = Rc::clone(&b_calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    a.controller().add_listener({
        let calls = Rc::clone(&values);
        Rc::new(move || calls.set(calls.get() + 1))
    });
    drop(a_subscription);
    let _a_run = a.controller().forward().unwrap();
    let _b_run = b.controller().forward().unwrap();
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!(a_calls.get(), 0);
    assert_eq!(b_calls.get(), 2);
    assert!(
        values.get() > 0,
        "status removal leaves value delivery live"
    );
}

fn dropping_a_later_subscription_during_delivery_skips_it() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let later = Rc::new(RefCell::new(None));
    let _first = controller.subscribe_status({
        let later = Rc::clone(&later);
        Rc::new(move |_| {
            let removed: Option<StatusSubscription> = later.borrow_mut().take();
            drop(removed);
        })
    });
    let calls = Rc::new(Cell::new(0));
    *later.borrow_mut() = Some(controller.subscribe_status({
        let calls = Rc::clone(&calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    }));
    let _run = controller.forward().unwrap();
    assert_eq!(calls.get(), 0);
}

struct Capture(Rc<Cell<usize>>);

impl Drop for Capture {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn detach_keeps_the_callback_until_owner_teardown() {
    let registry = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    owner
        .controller()
        .subscribe_status({
            let calls = Rc::clone(&calls);
            let capture = Capture(Rc::clone(&drops));
            Rc::new(move |_| {
                let _keep = &capture;
                calls.set(calls.get() + 1);
            })
        })
        .detach();
    let _run = owner.controller().forward().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(drops.get(), 0);
    drop(owner);
    assert_eq!(drops.get(), 1);
    assert!(registry.is_empty());
}

fn a_surviving_guard_does_not_keep_the_source_alive() {
    let registry = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let drops = Rc::new(Cell::new(0));
    let subscription = owner.controller().subscribe_status({
        let capture = Capture(Rc::clone(&drops));
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    drop(owner);
    assert_eq!(drops.get(), 1);
    drop(subscription);
    assert_eq!(drops.get(), 1);
}

fn custom_sources_can_construct_removal_authority() {
    fn remove(
        source: &Notifier<AnimationStatus>,
        token: ListenerId,
        recovery: &mut flui_foundation::panic::PanicRecovery,
    ) -> Option<Rc<flui_foundation::notifier_generic::NotificationCallback<AnimationStatus>>> {
        source.inherit_failure(recovery);
        source.take_callback(token)
    }
    let source = Rc::new(Notifier::new());
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let token = source.add({
        let calls = Rc::clone(&calls);
        let capture = Capture(Rc::clone(&drops));
        Rc::new(move |_| {
            let _keep = &capture;
            calls.set(calls.get() + 1);
        })
    });
    let subscription = StatusSubscription::new(&source, token, remove);
    source.notify(&AnimationStatus::Forward);
    assert_eq!(calls.get(), 1);
    drop(subscription);
    assert_eq!(drops.get(), 1);
    source.notify(&AnimationStatus::Completed);
    assert_eq!(calls.get(), 1);
}

struct ReentrantCapture {
    controller: AnimationController,
    drops: Rc<Cell<usize>>,
}

impl Drop for ReentrantCapture {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        self.controller.set_value(0.5);
    }
}

fn callback_retirement_can_reenter_the_same_source() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let drops = Rc::new(Cell::new(0));
    let subscription = controller.subscribe_status({
        let capture = ReentrantCapture {
            controller: controller.clone(),
            drops: Rc::clone(&drops),
        };
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    drop(subscription);
    assert_eq!(drops.get(), 1);
    assert_eq!(controller.value(), 0.5);
}

struct FailingCapture;

impl Drop for FailingCapture {
    fn drop(&mut self) {
        panic!("status capture retirement failed");
    }
}

fn a_failed_retirement_withdraws_before_the_next_subscription() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let subscription = controller.subscribe_status({
        let capture = FailingCapture;
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(subscription)))
        .expect_err("capture retirement must report its failure");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"status capture retirement failed")
    );
    let calls = Rc::new(Cell::new(0));
    let _next = controller.subscribe_status({
        let calls = Rc::clone(&calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    let _run = controller.forward().unwrap();
    let _run = controller.reverse().unwrap();
    assert_eq!(calls.get(), 2);
}

fn removal_during_a_failed_round_preserves_failure_and_progress() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let _first = controller.subscribe_status(Rc::new(|status| {
        assert!(status != AnimationStatus::Forward, "first status failure");
    }));
    let later = Rc::new(RefCell::new(None));
    let _remover = controller.subscribe_status({
        let later = Rc::clone(&later);
        Rc::new(move |_| {
            let subscription: Option<StatusSubscription> = later.borrow_mut().take();
            drop(subscription);
        })
    });
    let removed_calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    *later.borrow_mut() = Some(controller.subscribe_status({
        let calls = Rc::clone(&removed_calls);
        let capture = Capture(Rc::clone(&drops));
        Rc::new(move |_| {
            let _keep = &capture;
            calls.set(calls.get() + 1);
        })
    }));
    let tail_calls = Rc::new(Cell::new(0));
    let _tail = controller.subscribe_status({
        let calls = Rc::clone(&tail_calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.forward()))
        .expect_err("the first status failure must be resumed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"first status failure")
    );
    assert_eq!(removed_calls.get(), 0);
    assert_eq!(drops.get(), 0, "failed delivery retains opaque captures");
    assert_eq!(tail_calls.get(), 1);
    let _run = controller.reverse().unwrap();
    assert_eq!(removed_calls.get(), 0);
    assert_eq!(tail_calls.get(), 2);
}

fn a_subscription_can_withdraw_itself_during_delivery() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let own = Rc::new(RefCell::new(None));
    let calls = Rc::new(Cell::new(0));
    *own.borrow_mut() = Some(controller.subscribe_status({
        let own = Rc::clone(&own);
        let calls = Rc::clone(&calls);
        Rc::new(move |_| {
            calls.set(calls.get() + 1);
            let removed: Option<StatusSubscription> = own.borrow_mut().take();
            drop(removed);
        })
    }));
    let _run = controller.forward().unwrap();
    let _run = controller.reverse().unwrap();
    assert_eq!(calls.get(), 1);
    assert!(own.borrow().is_none());
}

fn a_closed_sources_guard_cannot_remove_a_new_owners_callback() {
    let registry = Vsync::new();
    let mut old = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let old_subscription = old.controller().subscribe_status(Rc::new(|_| {}));
    old.dispose();
    let next = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let calls = Rc::new(Cell::new(0));
    let _next_subscription = next.controller().subscribe_status({
        let calls = Rc::clone(&calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    drop(old_subscription);
    let _run = next.controller().forward().unwrap();
    assert_eq!(calls.get(), 1);
}

fn admission_after_disposal_retires_captures_outside_the_state_borrow() {
    let registry = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    owner.dispose();
    let drops = Rc::new(Cell::new(0));
    let subscription = owner.controller().subscribe_status({
        let capture = ReentrantCapture {
            controller: owner.controller().clone(),
            drops: Rc::clone(&drops),
        };
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    assert_eq!(drops.get(), 1);
    drop(subscription);
    assert_eq!(drops.get(), 1);
    assert!(registry.is_empty());
}

fn capture_retirement_releasing_the_last_wrapper_silences_reentry() {
    struct ReleaseAndNotify {
        owner: Rc<RefCell<Option<Rc<dyn Animation<f64>>>>>,
        parent: Rc<AnimationController>,
    }
    impl Drop for ReleaseAndNotify {
        fn drop(&mut self) {
            let owner = self.owner.borrow_mut().take();
            drop(owner);
            let _run = self.parent.forward().unwrap();
        }
    }
    type Wrap = fn(Rc<dyn Animation<f64>>) -> Rc<dyn Animation<f64>>;
    let wrappers: &[(&str, Wrap)] = &[
        ("reverse", |parent| Rc::new(ReverseAnimation::new(parent))),
        ("curved", |parent| {
            Rc::new(CurvedAnimation::new(parent, Curves::Linear))
        }),
        ("tween", |parent| {
            Rc::new(TweenAnimation::new(FloatTween::new(0.0, 1.0), parent))
        }),
        ("proxy", |parent| Rc::new(ProxyAnimation::new(parent))),
        ("switch", |parent| {
            Rc::new(AnimationSwitch::new(parent, None))
        }),
    ];
    for (name, wrap) in wrappers {
        let parent = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
        let adapter = wrap(parent.clone());
        let owner = Rc::new(RefCell::new(Some(adapter.clone())));
        let subscription = adapter.subscribe_status({
            let capture = ReleaseAndNotify {
                owner: Rc::clone(&owner),
                parent: Rc::clone(&parent),
            };
            Rc::new(move |_| {
                let _keep = &capture;
            })
        });
        let calls = Rc::new(Cell::new(0));
        adapter
            .subscribe_status({
                let calls = Rc::clone(&calls);
                Rc::new(move |_| calls.set(calls.get() + 1))
            })
            .detach();
        drop(adapter);
        drop(subscription);
        assert!(owner.borrow().is_none());
        assert_eq!(
            calls.get(),
            0,
            "{name}: the retired wrapper must silence reentrant notifications"
        );
    }
}

fn enclosing_cleanup_preserves_its_first_failure() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let drops = Rc::new(Cell::new(0));
    let mut subscription = controller.subscribe_status({
        let capture = Capture(Rc::clone(&drops));
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    let mut recovery = flui_foundation::panic::PanicRecovery::new();
    recovery.run(|| panic!("enclosing cleanup failure"));
    subscription.cancel_with_recovery(&mut recovery);
    assert_eq!(drops.get(), 0, "a failed cleanup retains opaque captures");
    drop(subscription);
    let calls = Rc::new(Cell::new(0));
    let _next = controller.subscribe_status({
        let calls = Rc::clone(&calls);
        Rc::new(move |_| calls.set(calls.get() + 1))
    });
    let _run = controller.forward().unwrap();
    assert_eq!(calls.get(), 1);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recovery.finish()))
        .expect_err("the enclosing failure must be resumed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"enclosing cleanup failure")
    );
}

fn proxy_subscription_survives_parent_replacement() {
    let first = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
    let next = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
    let proxy = ProxyAnimation::new(first.clone());
    let statuses = Rc::new(RefCell::new(Vec::new()));
    let subscription = proxy.subscribe_status({
        let statuses = Rc::clone(&statuses);
        Rc::new(move |status| statuses.borrow_mut().push(status))
    });
    let _run = first.forward().unwrap();
    proxy.set_parent(next.clone());
    let _run = next.forward().unwrap();
    assert_eq!(
        *statuses.borrow(),
        [
            AnimationStatus::Forward,
            AnimationStatus::Dismissed,
            AnimationStatus::Forward
        ]
    );
    drop(subscription);
    let _run = next.reverse().unwrap();
    let _run = first.reverse().unwrap();
    assert_eq!(statuses.borrow().len(), 3);
}

fn switch_subscription_survives_the_active_parent_hop() {
    let first = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
    let next = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
    first.set_value(0.8);
    next.set_value(0.3);
    let switch = AnimationSwitch::new(first.clone(), Some(next.clone()));
    let statuses = Rc::new(RefCell::new(Vec::new()));
    let subscription = switch.subscribe_status({
        let statuses = Rc::clone(&statuses);
        Rc::new(move |status| statuses.borrow_mut().push(status))
    });
    next.set_value(1.0);
    assert!(Rc::ptr_eq(
        &switch.current(),
        &(next.clone() as Rc<dyn Animation<f64>>)
    ));
    assert_eq!(*statuses.borrow(), [AnimationStatus::Completed]);
    let _run = next.reverse().unwrap();
    assert_eq!(
        *statuses.borrow(),
        [AnimationStatus::Completed, AnimationStatus::Reverse]
    );
    drop(subscription);
    let _run = next.forward().unwrap();
    first.set_value(0.0);
    assert_eq!(statuses.borrow().len(), 2);
}

fn adapter_subscriptions_follow_the_shared_owner() {
    fn check<A: Animation<f64> + Clone>(
        construct: impl FnOnce(Rc<dyn Animation<f64>>) -> A,
        subscribe: fn(&A, StatusCallback) -> StatusSubscription,
    ) {
        let parent = Rc::new(AnimationController::builder(Duration::from_secs(1)).build());
        let adapter = construct(parent.clone());
        let sibling = adapter.clone();
        let calls = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let removed = subscribe(&adapter, {
            let calls = Rc::clone(&calls);
            let capture = Capture(Rc::clone(&drops));
            Rc::new(move |_| {
                let _keep = &capture;
                calls.set(calls.get() + 1);
            })
        });
        let kept_calls = Rc::new(Cell::new(0));
        let kept_drops = Rc::new(Cell::new(0));
        let surviving_guard = subscribe(&sibling, {
            let calls = Rc::clone(&kept_calls);
            let capture = Capture(Rc::clone(&kept_drops));
            Rc::new(move |_| {
                let _keep = &capture;
                calls.set(calls.get() + 1);
            })
        });
        drop(adapter);
        let _run = parent.forward().unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(kept_calls.get(), 1);
        drop(removed);
        assert_eq!(drops.get(), 1);
        let _run = parent.reverse().unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(kept_calls.get(), 2);
        drop(sibling);
        assert_eq!(kept_drops.get(), 1, "the guard must not own the adapter");
        drop(surviving_guard);
        let _run = parent.forward().unwrap();
        assert_eq!(kept_calls.get(), 2);
    }
    check(ReverseAnimation::new, ReverseAnimation::subscribe_status);
    check(
        |parent| CurvedAnimation::new(parent, Curves::Linear),
        CurvedAnimation::subscribe_status,
    );
    check(
        |parent| TweenAnimation::new(FloatTween::new(0.0, 1.0), parent),
        TweenAnimation::subscribe_status,
    );
    check(ProxyAnimation::new, ProxyAnimation::subscribe_status);
    check(
        |parent| AnimationSwitch::new(parent, None),
        AnimationSwitch::subscribe_status,
    );
}

#[test]
fn owning_status_subscription_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "last wrapper released by capture retirement",
            capture_retirement_releasing_the_last_wrapper_silences_reentry,
        ),
        (
            "enclosing cleanup failure",
            enclosing_cleanup_preserves_its_first_failure,
        ),
        (
            "adapter shared owner",
            adapter_subscriptions_follow_the_shared_owner,
        ),
        (
            "proxy replacement",
            proxy_subscription_survives_parent_replacement,
        ),
        (
            "switch parent hop",
            switch_subscription_survives_the_active_parent_hop,
        ),
        (
            "independent sources and channels",
            dropping_one_subscription_preserves_independent_sources_and_channels,
        ),
        (
            "drop during delivery",
            dropping_a_later_subscription_during_delivery_skips_it,
        ),
        (
            "detach until source closes",
            detach_keeps_the_callback_until_owner_teardown,
        ),
        (
            "weak source ownership",
            a_surviving_guard_does_not_keep_the_source_alive,
        ),
        (
            "custom source construction",
            custom_sources_can_construct_removal_authority,
        ),
        (
            "capture reentry",
            callback_retirement_can_reenter_the_same_source,
        ),
        (
            "retirement failure recovery",
            a_failed_retirement_withdraws_before_the_next_subscription,
        ),
        (
            "removal in a failed round",
            removal_during_a_failed_round_preserves_failure_and_progress,
        ),
        (
            "own removal during delivery",
            a_subscription_can_withdraw_itself_during_delivery,
        ),
        (
            "closed source and new owner",
            a_closed_sources_guard_cannot_remove_a_new_owners_callback,
        ),
        (
            "disposed admission",
            admission_after_disposal_retires_captures_outside_the_state_borrow,
        ),
    ];
    if let Some(selected) = crate::child_process::selected_case() {
        let (_, case) = cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("selected status subscription case");
        case();
        crate::child_process::pass();
    }
    let names: Vec<_> = cases.iter().map(|(name, _)| *name).collect();
    crate::child_process::run_rows(
        "status_subscription::owning_status_subscription_contract",
        &names,
    );
}

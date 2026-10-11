//! Registry nesting, muting and owner retirement through public handles.

use flui_animation::{
    Animation, AnimationController, AnimationStatus, MotionClock, Vsync, VsyncRegistrationError,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

fn a_muted_ancestor_starves_an_unmuted_descendant() {
    let outer = Vsync::new();
    let middle = Vsync::new();
    let inner = Vsync::new();
    outer.attach_child(&middle).unwrap();
    middle.attach_child(&inner).unwrap();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&inner));
    let c = owner.controller();
    let run = c.forward().unwrap();
    middle.set_muted(true);
    assert!(!inner.is_muted());
    let mut clock = MotionClock::new();
    outer.tick_all(&clock.frame(Duration::ZERO));
    outer.tick_all(&clock.frame(Duration::from_millis(500)));
    assert_eq!(
        c.value(),
        0.0,
        "an enabled descendant cannot bypass a muted ancestor"
    );
    assert!(run.is_pending());
    middle.set_muted(false);
    outer.tick_all(&clock.frame(Duration::from_secs(1)));
    outer.tick_all(&clock.frame(Duration::from_millis(1400)));
    assert!(
        (c.value() - 0.4).abs() < 1e-6,
        "the first eligible frame anchors the run"
    );
}

fn a_cyclic_nesting_is_refused() {
    let outer = Vsync::new();
    let inner = Vsync::new();
    outer.attach_child(&inner).unwrap();
    assert_eq!(
        inner.attach_child(&outer),
        Err(VsyncRegistrationError::WouldCycle)
    );
    assert_eq!(
        outer.attach_child(&outer),
        Err(VsyncRegistrationError::WouldCycle)
    );
    outer.tick_all(&MotionClock::new().frame(Duration::ZERO));
}

fn a_listener_may_unregister_from_inside_tick_all() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    let c = owner.controller().clone();
    let slot = Rc::new(RefCell::new(Some(owner)));
    let retiring = Rc::downgrade(&slot);
    let _subscription = c.subscribe_status(Rc::new(move |status| {
        if status == AnimationStatus::ForwardSettled {
            let slot = retiring.upgrade().expect("logical owner is live");
            let owner = slot.borrow_mut().take();
            drop(owner);
        }
    }));
    let run = c.forward().unwrap();
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    vsync.tick_all(&clock.frame(Duration::from_millis(200)));
    assert!(
        slot.borrow().is_none(),
        "the completion listener retired its owner"
    );
    assert!(vsync.is_empty(), "retirement withdrew the registry seat");
    assert!(
        run.is_complete(),
        "owner retirement preserves the completed run"
    );
}

#[test]
fn vsync_admission_contract() {
    crate::run_table(&[
        (
            "muted ancestor gates enabled descendants",
            a_muted_ancestor_starves_an_unmuted_descendant,
        ),
        (
            "cycles refuse without corrupting the graph",
            a_cyclic_nesting_is_refused,
        ),
        (
            "completion listener retires its own registry seat",
            a_listener_may_unregister_from_inside_tick_all,
        ),
    ]);
}

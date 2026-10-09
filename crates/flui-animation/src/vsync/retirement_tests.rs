//! Private registration identity and final registry custody need token access
//! that public controller owners deliberately do not expose.

use super::{AnimationController, Vsync, VsyncRegistration};
use crate::{Animation, AnimationStatus};
use std::{
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy)]
enum VsyncRemoval {
    Controller,
    Child,
}

struct VsyncRetirementProbe {
    registry: Vsync,
    registration: Rc<Mutex<Option<VsyncRegistration>>>,
    replacement: Rc<Mutex<Option<(AnimationController, VsyncRegistration)>>>,
    drops: Rc<AtomicUsize>,
    removal: VsyncRemoval,
    panics: bool,
}

impl Drop for VsyncRetirementProbe {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            self.registry.len(),
            1,
            "removed owner is absent during retirement"
        );
        let id = self
            .registration
            .lock()
            .expect("vsync registration")
            .as_ref()
            .expect("admitted owner")
            .clone();
        match self.removal {
            VsyncRemoval::Controller => self.registry.unregister(&id),
            VsyncRemoval::Child => self.registry.detach_child(&id),
        }
        let fresh = AnimationController::builder(Duration::from_secs(1)).build();
        fresh.forward().expect("retirement registers a fresh run");
        let id = self.registry.register(fresh.clone());
        *self.replacement.lock().expect("fresh registration") = Some((fresh, id));
        assert_eq!(
            self.registry.len(),
            2,
            "fresh work admitted during retirement"
        );
        assert!(!self.panics, "vsync retirement first");
    }
}

fn vsync_retirement_reentry(removal: VsyncRemoval) {
    for panics in [false, true] {
        let registry = Vsync::new();
        let sibling = AnimationController::builder(Duration::from_secs(1)).build();
        let sibling_id = registry.register(sibling.clone());
        sibling.forward().expect("sibling runs");
        let registration = Rc::new(Mutex::new(None));
        let replacement = Rc::new(Mutex::new(None));
        let drops = Rc::new(AtomicUsize::new(0));
        let owner = AnimationController::builder(Duration::from_secs(1)).build();
        let probe = VsyncRetirementProbe {
            registry: registry.clone(),
            registration: registration.clone(),
            replacement: replacement.clone(),
            drops: drops.clone(),
            removal,
            panics,
        };
        owner.add_status_listener(std::rc::Rc::new(move |_| {
            let _capture = &probe;
        }));

        let child_order = Rc::new(Mutex::new(Vec::new()));
        let mut sibling_children = Vec::new();
        // Put the removed child between surviving siblings, with two trailing
        // children so an order-changing swap removal would be observable.
        let make_child = |label| {
            let child = Vsync::new();
            let animation = AnimationController::builder(Duration::from_secs(1)).build();
            let order = child_order.clone();
            animation.add_status_listener(std::rc::Rc::new(move |status| {
                if status == AnimationStatus::Completed {
                    order.lock().expect("child tick order").push(label);
                }
            }));
            animation.forward().expect("child sibling runs");
            child.register(animation);
            child
        };
        let id = match removal {
            VsyncRemoval::Controller => registry.register(owner),
            VsyncRemoval::Child => {
                let before = make_child("before");
                registry
                    .attach_child(&before)
                    .expect("before child admitted");
                sibling_children.push(before);
                let child = Vsync::new();
                child.register(owner);
                let id = registry.attach_child(&child).expect("owned child admitted");
                drop(child);
                for label in ["after first", "after second"] {
                    let after = make_child(label);
                    registry.attach_child(&after).expect("after child admitted");
                    sibling_children.push(after);
                }
                id
            }
        };
        *registration.lock().expect("registration stored") = Some(id.clone());
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "registry owns the last controller"
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match removal {
            VsyncRemoval::Controller => registry.unregister(&id),
            VsyncRemoval::Child => registry.detach_child(&id),
        }));
        if panics {
            let failure = result.expect_err("ordinary retirement failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("vsync retirement first")
            );
            flui_foundation::panic::retain_opaque_payload(failure);
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "actual last owner retired once"
        );
        match removal {
            VsyncRemoval::Controller => registry.unregister(&id),
            VsyncRemoval::Child => registry.detach_child(&id),
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1, "removal is idempotent");
        let (fresh, fresh_id) = replacement
            .lock()
            .expect("fresh run preserved")
            .take()
            .expect("reentrant registration");
        assert_eq!(
            registry.len(),
            2,
            "independent sibling and new registration survive"
        );
        registry.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
        );
        registry.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
        );
        assert_eq!(
            sibling.value(),
            1.0,
            "sibling reaches its target after retirement"
        );
        assert_eq!(fresh.value(), 1.0, "reentrantly registered run ticks next");
        assert!(!registry.has_running(), "completed runs quiesce");
        if let VsyncRemoval::Child = removal {
            assert_eq!(
                *child_order.lock().expect("surviving children order"),
                vec!["before", "after first", "after second"]
            );
        }
        registry.unregister(&sibling_id);
        registry.unregister(&fresh_id);
        assert!(registry.is_empty());
        drop(sibling_children);
    }
}

fn vsync_unregister_retires_last_owner_outside_registry_guard() {
    vsync_retirement_reentry(VsyncRemoval::Controller);
}

#[expect(
    clippy::redundant_clone,
    reason = "owning registry and token clones must authorize the same removal"
)]
fn vsync_tokens_only_remove_their_own_admission() {
    let a = Vsync::new();
    let b = Vsync::new();
    let first = AnimationController::builder(Duration::from_secs(1)).build();
    let second = AnimationController::builder(Duration::from_secs(1)).build();
    let a_id = a.try_register(&first).expect("first registry admission");
    let b_id = b.try_register(&second).expect("second registry admission");
    first.forward().expect("first run");
    second.forward().expect("second run");
    a.unregister(&b_id);
    b.unregister(&a_id);
    assert_eq!(
        a.len(),
        1,
        "foreign first-slot token preserves first registry"
    );
    assert_eq!(
        b.len(),
        1,
        "foreign first-slot token preserves second registry"
    );
    for registry in [&a, &b] {
        registry.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
        );
        registry.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
        );
    }
    assert_eq!(first.value(), 0.5);
    assert_eq!(second.value(), 0.5);
    a.clone().unregister(&a_id.clone());
    assert!(a.is_empty(), "owning registry clone accepts cloned token");
    let replacement_id = a.register(first.clone());
    a.unregister(&a_id);
    a.unregister(&b_id);
    assert_eq!(
        a.len(),
        1,
        "stale or foreign token cannot remove fresh admission"
    );
    a.tick_all(&flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)));
    a.tick_all(&flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)));
    assert_eq!(first.value(), 0.5, "fresh admission has its own anchor");
    a.tick_all(&flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.5)));
    assert_eq!(first.value(), 1.0);
    a.unregister(&replacement_id);
    b.unregister(&b_id);

    let expired = {
        let registry = Vsync::new();
        registry.register(AnimationController::builder(Duration::from_secs(1)).build())
    };
    let unrelated = Vsync::new();
    let resident = AnimationController::builder(Duration::from_secs(1)).build();
    let resident_id = unrelated.register(resident);
    unrelated.unregister(&expired);
    assert_eq!(
        unrelated.len(),
        1,
        "expired owner token cannot name a new registry"
    );
    unrelated.unregister(&resident_id);

    let parents = [Vsync::new(), Vsync::new()];
    let expired_child = {
        let parent = Vsync::new();
        parent
            .attach_child(&Vsync::new())
            .expect("temporary child admission")
    };
    let children = [Vsync::new(), Vsync::new()];
    let animations = [
        AnimationController::builder(Duration::from_secs(1)).build(),
        AnimationController::builder(Duration::from_secs(1)).build(),
    ];
    let child_ids: Vec<_> = parents
        .iter()
        .zip(&children)
        .zip(&animations)
        .map(|((parent, child), animation)| {
            child.register(animation.clone());
            animation.forward().expect("nested run");
            parent.attach_child(child).expect("nested admission")
        })
        .collect();
    parents[0].detach_child(&child_ids[1]);
    parents[1].detach_child(&child_ids[0]);
    parents[0].detach_child(&expired_child);
    for parent in &parents {
        parent.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
        );
        parent.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
        );
    }
    assert_eq!(
        animations[0].value(),
        0.5,
        "foreign child token preserves first nested run"
    );
    assert_eq!(
        animations[1].value(),
        0.5,
        "foreign child token preserves second nested run"
    );
    let own_controller = AnimationController::builder(Duration::from_secs(1)).build();
    let controller_id = parents[0].register(own_controller.clone());
    own_controller.forward().expect("parent run");
    parents[0].detach_child(&controller_id);
    parents[0].unregister(&child_ids[0]);
    parents[0].tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
    );
    parents[0].tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
    );
    assert_eq!(
        own_controller.value(),
        0.5,
        "child removal refuses controller token"
    );
    assert_eq!(
        animations[0].value(),
        1.0,
        "controller removal refuses child token"
    );
    parents[0].clone().detach_child(&child_ids[0].clone());
    parents[0].detach_child(&child_ids[0]);
    animations[0]
        .reverse()
        .expect("detached child starts a new run");
    parents[0].tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.5)),
    );
    assert_eq!(
        animations[0].value(),
        1.0,
        "own child removal stopped nested ticks"
    );
    let reattached = parents[0]
        .attach_child(&children[0])
        .expect("fresh child admission");
    parents[0].detach_child(&child_ids[0]);
    parents[0].tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.5)),
    );
    parents[0].tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(2.0)),
    );
    assert_eq!(
        animations[0].value(),
        0.5,
        "stale child token preserves new admission"
    );
    parents[0].detach_child(&reattached);
    parents[0].unregister(&controller_id);
}

fn vsync_detach_retires_last_child_outside_parent_guard() {
    vsync_retirement_reentry(VsyncRemoval::Child);
}

#[test]
fn registration_identity_and_retirement() {
    crate::test_cases::run_cases(&[
        (
            "vsync tokens preserve owner identity",
            vsync_tokens_only_remove_their_own_admission,
        ),
        (
            "vsync unregister last-owner reentry",
            vsync_unregister_retires_last_owner_outside_registry_guard,
        ),
        (
            "vsync detach last-child reentry",
            vsync_detach_retires_last_child_outside_parent_guard,
        ),
    ]);
}

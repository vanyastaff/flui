//! Owner callbacks may capture UI state; only wake capabilities cross threads.

use std::cell::RefCell;
use std::rc::Rc;

use flui_scheduler::{OwnerFrame, Priority, UpdateScheduler};

thread_local! {
    static OWNER_SCHEDULER: RefCell<Option<flui_scheduler::WeakUpdateScheduler>> = const { RefCell::new(None) };
}

/// A Send test probe resolves UI state only on the thread that installed it.
/// Production wake capabilities never contain or resolve this state.
#[derive(Clone, Copy)]
pub(crate) struct OwnerProbe(std::thread::ThreadId);

pub(crate) fn owner_probe(scheduler: &UpdateScheduler) -> OwnerProbe {
    OWNER_SCHEDULER.with(|slot| *slot.borrow_mut() = Some(scheduler.downgrade()));
    OwnerProbe(std::thread::current().id())
}

impl OwnerProbe {
    pub(crate) fn upgrade(self) -> Option<UpdateScheduler> {
        assert_eq!(
            self.0,
            std::thread::current().id(),
            "owner probe stays on its thread"
        );
        OWNER_SCHEDULER.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(flui_scheduler::WeakUpdateScheduler::upgrade)
        })
    }
}

fn callbacks_share_owner_state_and_allow_registration_reentry() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("one frame owner");
    let events = Rc::new(RefCell::new(Vec::new()));
    let callback_events = events.clone();
    let reentrant = scheduler.clone();
    scheduler.schedule_frame_callback(Box::new(move |_| {
        callback_events.borrow_mut().push("transient");
        let events = callback_events.clone();
        reentrant.schedule_microtask(Box::new(move || events.borrow_mut().push("microtask")));
    }));
    let callback_events = events.clone();
    scheduler.add_task(Priority::Build, move || {
        callback_events.borrow_mut().push("build");
    });
    let callback_events = events.clone();
    scheduler.add_persistent_frame_callback(Rc::new(move |_| {
        callback_events.borrow_mut().push("persistent");
    }));
    let callback_events = events.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        callback_events.borrow_mut().push("post_frame");
    }));
    scheduler.execute_frame(&owner);
    assert_eq!(
        events.borrow().as_slice(),
        [
            "transient",
            "microtask",
            "persistent",
            "build",
            "post_frame"
        ]
    );
}

fn post_frame_storage_belongs_to_one_owner_generation() {
    struct Capture {
        drops: Rc<std::cell::Cell<usize>>,
        owner_thread: std::thread::ThreadId,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert_eq!(self.owner_thread, std::thread::current().id());
            self.drops.set(self.drops.get() + 1);
        }
    }
    let scheduler = UpdateScheduler::new();
    let construction = flui_scheduler::PostFrameHandle::new(&scheduler);
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    construction
        .schedule(move |_| observed.borrow_mut().push("construction"))
        .expect("construction admits work before its first owner");
    let first = OwnerFrame::new(&scheduler).expect("first owner");
    scheduler.execute_frame(&first);
    assert_eq!(events.borrow().as_slice(), ["construction"]);

    let stale = first.post_frame_handle();
    let drops = Rc::new(std::cell::Cell::new(0));
    let capture = Capture {
        drops: drops.clone(),
        owner_thread: std::thread::current().id(),
    };
    stale
        .schedule(move |_| {
            let _ = &capture;
        })
        .expect("live owner");
    drop(first);
    assert_eq!(
        drops.get(),
        1,
        "scheduler and weak handles do not retain the retired queue"
    );
    let second = OwnerFrame::new(&scheduler).expect("replacement owner");
    assert!(
        stale
            .schedule(|_| panic!("stale admission must be refused"))
            .is_err()
    );
    assert!(
        construction
            .schedule(|_| panic!("construction handle cannot follow replacement"))
            .is_err()
    );
    let observed = events.clone();
    second
        .post_frame_handle()
        .schedule(move |_| observed.borrow_mut().push("replacement"))
        .expect("replacement admits its own work");
    scheduler.execute_frame(&second);
    assert_eq!(events.borrow().as_slice(), ["construction", "replacement"]);
}

#[test]
fn owner_callback_contract() {
    crate::run_table(
        "owner callbacks",
        &[
            (
                "frame reentry",
                callbacks_share_owner_state_and_allow_registration_reentry,
            ),
            (
                "post-frame owner generation",
                post_frame_storage_belongs_to_one_owner_generation,
            ),
        ],
    );
}

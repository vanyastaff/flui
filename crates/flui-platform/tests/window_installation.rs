use flui_platform::shared::window_installation::WindowInstallation;
use flui_platform::{HeadlessPlatform, Platform};

fn acknowledgement_and_cancellation_are_committed_before_notification() {
    Box::new(HeadlessPlatform::new())
        .run(Box::new(|owner| {
            let (ready, pending) = WindowInstallation::channel(owner.proxy());
            assert!(pending.outcome().is_none());
            ready.complete().expect("notify readiness");
            assert_eq!(pending.outcome(), Some(Ok(())));
            assert_eq!(
                pending.outcome(),
                Some(Ok(())),
                "reading cannot consume completion"
            );

            let (cancelled, pending) = WindowInstallation::channel(owner.proxy());
            drop(cancelled);
            assert!(pending.outcome().expect("cancellation committed").is_err());

            let (late, retired) = WindowInstallation::channel(owner.proxy());
            drop(retired);
            late.complete()
                .expect("retired attachment ignores late acknowledgement");
            Ok(())
        }))
        .expect("headless owner");
}

#[test]
fn window_installation_contract() {
    crate::run_table(
        "window_installation_contract",
        &[
            (
                "acknowledgement_and_cancellation_are_committed_before_notification",
                acknowledgement_and_cancellation_are_committed_before_notification as fn(),
            ),
            (
                "failed_wake_does_not_undo_installation_acknowledgement",
                failed_wake_does_not_undo_installation_acknowledgement as fn(),
            ),
        ],
    );
}

fn failed_wake_does_not_undo_installation_acknowledgement() {
    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(platform)
        .run(Box::new(move |owner| {
            owner.on_wake(Box::new(|| {}))?;
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");
    turns.drive();
    let (ready, pending) = WindowInstallation::channel(owner.proxy());
    turns.fail_next_wake();
    assert!(
        ready.complete().is_err(),
        "native posting failure reaches caller"
    );
    assert_eq!(
        pending.outcome(),
        Some(Ok(())),
        "accepted completion survives a failed notification"
    );
    owner.proxy().wake().expect("later owner opportunity");
    turns.drive();
    assert_eq!(
        pending.outcome(),
        Some(Ok(())),
        "retry cannot convert readiness to cancellation"
    );
}

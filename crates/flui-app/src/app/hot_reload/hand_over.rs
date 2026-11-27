//! The hand-over a development restart runs the application's entry with
//! (ADR-0393 §4 step 2).
//!
//! While [`run_entry`] runs the entry, the hand-over is open on the owner
//! thread: [`Application::run`](crate::Application::run) and
//! `run_resident` hand their application to it and return without starting a
//! loop, since the loop that restarts is already running. The hand-over
//! belongs to that one call: it is empty before it and drained after it on
//! every exit, so an application handed to it is composed by the restart that
//! opened it or by nothing.

use flui_foundation::panic::{FirstPanic, payload_text};
use flui_view::BoxedView;

use crate::app::LaunchError;
use crate::app::application::DesktopRun;
use crate::app::application_control::AppHandle;
use crate::app::runner::with_hand_over;

/// An application the entry handed over, its root erased so that the loop
/// composes it whatever view type the new code declares.
pub(crate) type HandedRun = DesktopRun<BoxedView, Box<dyn FnMut(&AppHandle) -> BoxedView>>;

/// The hand-over's state, held in the loop's state on the owner thread.
#[derive(Default)]
pub(crate) enum EntrySlot {
    /// No entry runs: `run` starts a loop as it does outside a restart.
    #[default]
    Closed,
    /// An entry runs and has handed nothing over yet.
    Open,
    /// The entry handed this application over.
    Handed(Box<HandedRun>),
    /// The entry's `run` refused its application with this error, which the
    /// entry also received.
    Refused(LaunchError),
}

/// Why the entry gave the restart no application to compose.
#[derive(Debug)]
pub(crate) enum EntryFailure {
    /// The entry panicked; what it handed over before is dropped.
    Panicked,
    /// The entry returned without calling `run`.
    NoApplication,
    /// The entry's `run` refused the application it was given.
    Launch(LaunchError),
}

/// Whether an entry is running with the hand-over open on this thread, so
/// that `run` hands its application over instead of starting a loop.
pub(crate) fn is_open() -> bool {
    with_hand_over(|hand_over| !matches!(hand_over, EntrySlot::Closed)).unwrap_or(false)
}

/// Hand `run` over to the open hand-over, as `run` does inside a restart.
///
/// # Errors
///
/// [`LaunchError::InvalidWindowOption`] for window options no window can be
/// asked for, as a run outside a restart refuses them before anything starts;
/// [`LaunchError::AlreadyHandedOver`] when this entry already called `run`,
/// whether that call was handed over or refused: the first call decides what
/// the restart composes.
pub(crate) fn hand(run: HandedRun) -> Result<(), LaunchError> {
    let checked = crate::app::runner::check_window_options(&run.config);
    let mut run = Some(run);
    let answer = with_hand_over(|hand_over| match hand_over {
        EntrySlot::Open => {
            *hand_over = match &checked {
                Ok(()) => EntrySlot::Handed(Box::new(
                    run.take().expect("BUG: the run is handed over once"),
                )),
                Err(LaunchError::InvalidWindowOption(option)) => {
                    EntrySlot::Refused(LaunchError::InvalidWindowOption(*option))
                }
                Err(_) => {
                    unreachable!("BUG: the window-option check fails only with its own variant")
                }
            };
            Ok(())
        }
        EntrySlot::Handed(_) | EntrySlot::Refused(_) => Err(LaunchError::AlreadyHandedOver),
        EntrySlot::Closed => {
            unreachable!("BUG: `run` hands over only while `is_open` holds on this thread")
        }
    })
    .expect("BUG: `run` is called with no borrow of the loop's state");
    // A run that was not handed over is dropped here, with no borrow held:
    // its factory's captures are the application's own values.
    drop(run);
    answer.and(checked)
}

/// Run `entry` with the hand-over open and take what it handed over.
///
/// The loop's state is not borrowed while `entry` runs. A panic in `entry` is
/// contained and logged; whatever it handed over before is dropped, under
/// containment too, and the restart fails.
///
/// # Errors
///
/// [`EntryFailure`]: the entry panicked, handed nothing over, or had its
/// application refused.
pub(crate) fn run_entry(entry: impl FnOnce()) -> Result<HandedRun, EntryFailure> {
    let open = Opened::open();
    let mut panic = FirstPanic::new();
    let ran = panic.contain(entry);
    let handed = open.drain();
    if ran.is_none() {
        // Dropped before the event, whose subscriber is user code too: a
        // panic there finds nothing of the application left to drop.
        panic.retire(handed);
        let text = panic
            .held()
            .and_then(payload_text)
            .unwrap_or("a panic without a message");
        tracing::error!(
            panic = text,
            "the application's entry panicked during a restart"
        );
        // Reported above; nobody resumes it, and dropping the collector
        // discards it under containment.
        drop(panic);
        return Err(EntryFailure::Panicked);
    }
    match handed {
        EntrySlot::Handed(run) => Ok(*run),
        EntrySlot::Refused(error) => Err(EntryFailure::Launch(error)),
        EntrySlot::Open => Err(EntryFailure::NoApplication),
        EntrySlot::Closed => unreachable!("BUG: the hand-over stays open until it is drained"),
    }
}

/// The open hand-over: drained when the entry returns, and on any unwind
/// that passes it.
struct Opened {
    drained: bool,
}

impl Opened {
    fn open() -> Self {
        with_hand_over(|hand_over| {
            assert!(
                matches!(hand_over, EntrySlot::Closed),
                "BUG: one hand-over is open at a time; the hook is lent while its entry runs"
            );
            *hand_over = EntrySlot::Open;
        })
        .expect("BUG: a restart runs its entry with no borrow of the loop's state");
        Self { drained: false }
    }

    fn drain(mut self) -> EntrySlot {
        self.drained = true;
        take()
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // Only an unwind out of the entry's own containment passes here; a
        // borrow it left held keeps the slot, which the next `open` refuses.
        if !self.drained
            && let Ok(handed) = with_hand_over(std::mem::take)
        {
            drop(handed);
        }
    }
}

/// Close the hand-over and take what it holds; the caller drops it with the
/// loop's state no longer borrowed.
fn take() -> EntrySlot {
    with_hand_over(std::mem::take)
        .expect("BUG: the hand-over drains with no borrow of the loop's state")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::app::{AppConfig, Application};

    fn root() -> flui_widgets::Text {
        flui_widgets::Text::new("handed")
    }

    /// Counts the drops of the value a handed factory captures.
    struct Captured(Rc<Cell<usize>>);

    impl Drop for Captured {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// The failure of an entry the test expects to fail.
    fn failure_of(ran: Result<HandedRun, EntryFailure>, expected: &str) -> EntryFailure {
        match ran {
            Ok(_) => panic!("{expected}, but an application was handed over"),
            Err(failure) => failure,
        }
    }

    #[test]
    fn run_outside_a_restart_finds_the_hand_over_closed() {
        assert!(!is_open());
    }

    #[test]
    fn an_entry_hands_its_application_over_and_run_returns() {
        let mut returned = None;
        let handed = run_entry(|| {
            assert!(is_open());
            returned = Some(
                Application::new()
                    .config(AppConfig::new().title("next"))
                    .run(root()),
            );
        })
        .expect("the entry handed an application over");
        assert!(matches!(returned, Some(Ok(()))));
        assert_eq!(handed.config.title, "next");
        assert_eq!(handed.startup, crate::app::application::StartupWindow::Open);
        assert!(!is_open());
    }

    #[test]
    fn a_resident_entry_hands_over_without_a_startup_window() {
        let handed = run_entry(|| {
            Application::new()
                .run_resident(|_| root())
                .expect("handed over");
        })
        .expect("the entry handed an application over");
        assert_eq!(handed.startup, crate::app::application::StartupWindow::None);
    }

    #[test]
    fn an_entry_without_an_application_fails_the_restart() {
        let failure = failure_of(run_entry(|| {}), "nothing was handed over");
        assert!(matches!(failure, EntryFailure::NoApplication));
        assert!(!is_open());
    }

    #[test]
    fn a_panicking_entry_fails_the_restart() {
        let failure = failure_of(
            run_entry(|| panic!("the entry fails")),
            "the entry panicked",
        );
        assert!(matches!(failure, EntryFailure::Panicked));
        assert!(!is_open());
    }

    #[test]
    fn a_failed_restart_leaves_nothing_in_the_hand_over() {
        let drops = Rc::new(Cell::new(0));
        let captured = Captured(Rc::clone(&drops));
        let failure = failure_of(
            run_entry(|| {
                Application::new()
                    .run_resident(move |_| {
                        let _keep = &captured;
                        root()
                    })
                    .expect("handed over");
                panic!("the entry fails after handing over");
            }),
            "the entry panicked",
        );
        assert!(matches!(failure, EntryFailure::Panicked));
        assert_eq!(drops.get(), 1, "the handed application was dropped");
        assert!(!is_open());
        // The next restart opens a hand-over of its own.
        assert!(matches!(run_entry(|| {}), Err(EntryFailure::NoApplication)));
    }

    /// A capture whose `Drop` panics, as an application's value may.
    struct PanicsOnDrop;

    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("the capture's drop fails");
        }
    }

    #[test]
    fn a_handed_application_whose_drop_panics_is_contained_with_the_entry() {
        let captured = PanicsOnDrop;
        let failure = failure_of(
            run_entry(|| {
                Application::new()
                    .run_resident(move |_| {
                        let _keep = &captured;
                        root()
                    })
                    .expect("handed over");
                panic!("the entry fails after handing over");
            }),
            "the entry panicked",
        );
        assert!(matches!(failure, EntryFailure::Panicked));
        assert!(!is_open());
    }

    #[test]
    fn a_run_after_a_refused_one_keeps_the_first_refusal() {
        let mut second = None;
        let failure = failure_of(
            run_entry(|| {
                let first = Application::new()
                    .config(AppConfig::new().size(0, 0))
                    .run(root());
                assert!(matches!(first, Err(LaunchError::InvalidWindowOption(_))));
                second = Some(Application::new().run(root()));
            }),
            "the first run was refused",
        );
        assert!(matches!(second, Some(Err(LaunchError::AlreadyHandedOver))));
        assert!(matches!(
            failure,
            EntryFailure::Launch(LaunchError::InvalidWindowOption(_))
        ));
    }

    #[test]
    fn an_application_run_refuses_fails_the_restart_with_its_error() {
        let mut returned = None;
        let failure = failure_of(
            run_entry(|| {
                returned = Some(
                    Application::new()
                        .config(AppConfig::new().size(0, 0))
                        .run(root()),
                );
            }),
            "run refused the application",
        );
        assert!(matches!(
            returned,
            Some(Err(LaunchError::InvalidWindowOption(_)))
        ));
        assert!(matches!(
            failure,
            EntryFailure::Launch(LaunchError::InvalidWindowOption(_))
        ));
    }

    #[test]
    fn a_second_run_in_one_entry_is_refused_and_the_first_is_composed() {
        let mut second = None;
        let handed = run_entry(|| {
            Application::new()
                .config(AppConfig::new().title("first"))
                .run(root())
                .expect("handed over");
            second = Some(
                Application::new()
                    .config(AppConfig::new().title("second"))
                    .run(root()),
            );
        })
        .expect("the first application stays handed over");
        assert!(matches!(second, Some(Err(LaunchError::AlreadyHandedOver))));
        assert_eq!(handed.config.title, "first");
    }
}

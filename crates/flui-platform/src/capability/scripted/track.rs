//! [`Track`]: a value the platform changes over a scenario's time.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::Scenario;
use crate::Unsupported;
use crate::capability::{Completion, CompletionSource, Resolver};

/// A value the platform owns and changes while the application runs: a
/// service that goes away and comes back ([`ServiceState`]), or any state a
/// capability's simulator reads and its handles watch.
///
/// A double reads it when it answers ([`get`](Self::get)) and a handle
/// watches it ([`changed`](Self::changed)). The test changes it now
/// ([`set`](Self::set)) or at an instant ([`set_at`](Self::set_at)).
/// Owner-thread only, cheap to clone; every clone is the same value.
pub struct Track<T> {
    cell: Rc<TrackCell<T>>,
}

struct TrackCell<T> {
    name: &'static str,
    scenario: Scenario,
    state: RefCell<TrackState<T>>,
}

struct TrackState<T> {
    value: Rc<T>,
    /// The handles waiting for the next change.
    watchers: Vec<Resolver<T>>,
}

impl<T> Clone for Track<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}

impl<T> Track<T>
where
    T: Clone + PartialEq + fmt::Debug + 'static,
{
    pub(super) fn on(scenario: &Scenario, name: &'static str, initial: T) -> Self {
        Self {
            cell: Rc::new(TrackCell {
                name,
                scenario: scenario.clone(),
                state: RefCell::new(TrackState {
                    value: Rc::new(initial),
                    watchers: Vec::new(),
                }),
            }),
        }
    }

    /// The track's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.cell.name
    }

    /// The value now.
    #[must_use]
    pub fn get(&self) -> T {
        let value = Rc::clone(&self.cell.state.borrow().value);
        // Cloned after the borrow: `Clone` is the value type's code.
        (*value).clone()
    }

    /// Change the value to `value`. When it differs from the value now,
    /// every handle waiting on [`changed`](Self::changed) is answered with
    /// it, at its next owner turn; setting the value it already has changes
    /// nothing and answers nobody.
    ///
    /// # Panics
    ///
    /// A panic of the value type's `PartialEq` leaves before anything
    /// changes. One of its `Drop` or `Clone` is resumed once the value is
    /// changed and every other watcher answered; a watcher whose answer could
    /// not be cloned keeps waiting for the next change.
    pub fn set(&self, value: T) {
        let current = Rc::clone(&self.cell.state.borrow().value);
        // Compared, and the old value dropped, with no borrow held:
        // `PartialEq` and `Drop` are the value type's code.
        if *current == value {
            return;
        }
        drop(current);
        let value = Rc::new(value);
        let (replaced, watchers) = {
            let mut state = self.cell.state.borrow_mut();
            (
                std::mem::replace(&mut state.value, Rc::clone(&value)),
                std::mem::take(&mut state.watchers),
            )
        };
        let mut first = FirstPanic::new();
        first.contain(|| drop(replaced));
        for watcher in watchers {
            match first.contain(|| (*value).clone()) {
                // A retired owner has nobody left to tell.
                Some(answer) => {
                    let _ = watcher.resolve(answer);
                }
                None => self.cell.state.borrow_mut().watchers.push(watcher),
            }
        }
        first.resume();
    }

    /// Change the value to `value` at `at` on the scenario's clock: at the
    /// moment its driver reaches it, as [`set`](Self::set) does. An instant
    /// already passed applies when the driver next moves.
    pub fn set_at(&self, at: Duration, value: T) {
        let what = Cow::Owned(format!("{} becomes {value:?}", self.cell.name));
        let track = Rc::downgrade(&self.cell);
        self.cell.scenario.schedule(
            at,
            what,
            Box::new(move || {
                if let Some(cell) = track.upgrade() {
                    Track { cell }.set(value);
                }
            }),
        );
    }

    /// The next change of the value, settled at the owner turn after it.
    #[must_use = "a completion delivers the change only to on_complete, poll or its future"]
    pub fn changed(&self, completions: &CompletionSource) -> Completion<T> {
        let (completion, resolver) = completions.pending::<T>();
        self.cell.state.borrow_mut().watchers.push(resolver);
        completion
    }
}

impl Track<ServiceState> {
    /// `Ok` while the service is on; the refusal it was turned off with
    /// otherwise, the answer a provider gives an acquisition.
    ///
    /// # Errors
    ///
    /// The [`Unsupported`] of [`ServiceState::Off`].
    pub fn check(&self) -> Result<(), Unsupported> {
        match self.get() {
            ServiceState::On => Ok(()),
            ServiceState::Off(refusal) => Err(refusal),
        }
    }
}

impl<T> fmt::Debug for Track<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let watchers = self.cell.state.borrow().watchers.len();
        f.debug_struct("Track")
            .field("name", &self.cell.name)
            .field("watchers", &watchers)
            .finish_non_exhaustive()
    }
}

/// Whether a platform service is there: the value of a [`Track`] a double
/// checks before it provides or answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a service is there or it is not; why not is the refusal it carries"
)]
pub enum ServiceState {
    /// The service answers.
    On,
    /// The service refuses, with this refusal.
    Off(Unsupported),
}

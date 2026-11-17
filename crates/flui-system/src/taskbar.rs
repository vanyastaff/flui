//! Progress and a count on the application's launcher or taskbar button.
//!
//! A widget acquires [`TaskbarEntry`] through
//! `ctx.capability::<TaskbarCapability>()` and sets what the entry shows.
//! The entry is the application's: every handle sets the same one, and the
//! last value set is what it shows. What a system can show differs, and the
//! handle says which parts it honours ([`TaskbarEntry::honours`]).

use std::{fmt, rc::Rc};

use flui_platform::capability::{Capability, ProviderContext};

/// The taskbar capability: `ctx.capability::<TaskbarCapability>()` hands out
/// [`TaskbarEntry`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum TaskbarCapability {}

impl Capability for TaskbarCapability {
    type Handle = TaskbarEntry;
    const NAME: &'static str = "taskbar";
}

/// How far a task has come, from `0.0` (nothing done) to `1.0` (done).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Fraction(f64);

impl Fraction {
    /// `value`, when it is a finite number from `0.0` to `1.0`; `None`
    /// otherwise, so no entry is handed a value it cannot draw.
    #[must_use]
    pub fn try_new(value: f64) -> Option<Self> {
        (0.0..=1.0).contains(&value).then_some(Self(value))
    }

    /// The value, from `0.0` to `1.0`.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

/// What the entry's progress shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum Progress {
    /// No progress.
    #[default]
    Hidden,
    /// A task whose extent is not known yet; a system that cannot show it
    /// ([`Honoured::indeterminate`]) shows no progress.
    Indeterminate,
    /// A task this far along.
    Done(Fraction),
}

/// Which parts of the entry a system shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Honoured {
    /// A progress value.
    pub progress: bool,
    /// [`Progress::Indeterminate`].
    pub indeterminate: bool,
    /// A count.
    pub count: bool,
    /// Whether the system confirms that a launcher shows it. A signal that
    /// docks may or may not listen to is not confirmed.
    pub confirmed: bool,
}

impl Honoured {
    /// What a backend shows: `progress`, `indeterminate`, `count` and
    /// `confirmed`, in that order.
    #[must_use]
    pub const fn new(progress: bool, indeterminate: bool, count: bool, confirmed: bool) -> Self {
        Self {
            progress,
            indeterminate,
            count,
            confirmed,
        }
    }
}

/// What the entry shows: its progress and its count.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct EntryState {
    /// The progress.
    pub progress: Progress,
    /// The count on its badge, `None` for no badge.
    pub count: Option<u64>,
}

impl EntryState {
    /// An entry showing `progress` and `count`.
    #[must_use]
    pub const fn new(progress: Progress, count: Option<u64>) -> Self {
        Self { progress, count }
    }
}

/// The system's launcher entry as [`TaskbarEntry`] reaches it.
///
/// Owner-local. Each call carries the whole state the entry is to show; a
/// backend that sends it later sends the latest, never one that a later
/// call replaced.
pub trait TaskbarBackend: 'static {
    /// Show `state` on the application's entry.
    fn show(&self, state: EntryState);

    /// Which parts the system shows.
    fn honours(&self) -> Honoured;
}

/// Widget-facing launcher entry.
///
/// Owner-thread (`!Send`), cheap to clone; clones and other handles of the
/// application set the same entry.
#[derive(Clone)]
pub struct TaskbarEntry {
    backend: Rc<dyn TaskbarBackend>,
    state: Rc<std::cell::Cell<EntryState>>,
}

impl TaskbarEntry {
    /// A handle over `backend`, for the acquisition `ctx`: every handle of
    /// one UI runtime shares the state it last set.
    pub fn new(backend: impl TaskbarBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            state: ctx.runtime_local::<SharedEntry>().0.clone(),
        }
    }

    /// Show `progress`, keeping the count.
    pub fn set_progress(&self, progress: Progress) {
        self.update(|state| state.progress = progress);
    }

    /// Show `count` on the entry's badge, `None` for no badge, keeping the
    /// progress.
    pub fn set_count(&self, count: Option<u64>) {
        self.update(|state| state.count = count);
    }

    /// What the entry shows, as the handles last set it.
    #[must_use]
    pub fn state(&self) -> EntryState {
        self.state.get()
    }

    /// Which parts of the entry the system shows.
    #[must_use]
    pub fn honours(&self) -> Honoured {
        self.backend.honours()
    }

    fn update(&self, change: impl FnOnce(&mut EntryState)) {
        let mut state = self.state.get();
        change(&mut state);
        if state != self.state.get() {
            self.state.set(state);
            self.backend.show(state);
        }
    }
}

/// The entry's state one UI runtime's handles share.
#[derive(Default)]
struct SharedEntry(Rc<std::cell::Cell<EntryState>>);

impl fmt::Debug for TaskbarEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskbarEntry")
            .field("state", &self.state.get())
            .finish_non_exhaustive()
    }
}

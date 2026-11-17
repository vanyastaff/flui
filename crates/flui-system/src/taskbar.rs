//! A window's progress and badge on the taskbar (Windows) or the dock
//! (macOS).
//!
//! A widget acquires [`Taskbar`] through
//! `ctx.capability::<TaskbarCapability>()` in a lifecycle hook of a widget
//! in a window: the progress and the badge belong to that window's button.

use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;

use flui_platform::capability::{Capability, ProviderContext, WindowRef};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

/// The taskbar capability: `ctx.capability::<TaskbarCapability>()` hands out
/// a [`Taskbar`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum TaskbarCapability {}

impl Capability for TaskbarCapability {
    type Handle = Taskbar;
    const NAME: &'static str = "taskbar";
}

/// How far a task has come, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Fraction(f64);

impl Fraction {
    /// Nothing done.
    pub const ZERO: Self = Self(0.0);
    /// All done.
    pub const ONE: Self = Self(1.0);

    /// `value` as a fraction; `None` unless it is finite and within 0 and 1.
    #[must_use]
    pub fn try_new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(Self(value))
    }

    /// The fraction, within 0 and 1.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// The progress a window's taskbar button shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum TaskbarProgress {
    /// No progress.
    #[default]
    None,
    /// A task runs, and how far it has come is not known.
    Indeterminate,
    /// A task has come this far.
    Normal(Fraction),
    /// A task has come this far and is paused.
    Paused(Fraction),
    /// A task has come this far and failed.
    Error(Fraction),
}

/// The badge a window's taskbar button shows over its icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskbarBadge {
    /// No badge.
    #[default]
    None,
    /// A dot: something waits, with no count.
    Dot,
    /// A count, shown as `99+` above 99.
    Count(NonZeroU32),
}

/// Why the taskbar did not take a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TaskbarError {
    /// The window has closed.
    #[error("the window closed")]
    WindowClosed,
    /// This system has no taskbar progress or badge, or not for this
    /// window; the [`Unsupported`] says why.
    #[error("the taskbar cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// The system's taskbar as [`Taskbar`] reaches it.
///
/// Owner-local and synchronous: a change is a value the backend keeps for
/// its window and shows when it can, again after the taskbar restarts. It
/// is told only of a window that is open.
pub trait TaskbarBackend: 'static {
    /// Show `progress` on `window`'s button.
    ///
    /// # Errors
    ///
    /// Why the change was not taken.
    fn set_progress(
        &self,
        window: &WindowRef,
        progress: TaskbarProgress,
    ) -> Result<(), TaskbarError>;

    /// Show `badge` on `window`'s button.
    ///
    /// # Errors
    ///
    /// Why the change was not taken.
    fn set_badge(&self, window: &WindowRef, badge: TaskbarBadge) -> Result<(), TaskbarError>;
}

/// Widget-facing taskbar progress and badge for one window.
///
/// Owner-thread (`!Send`), cheap to clone. Each change replaces the last;
/// what a window's button shows ends with the window.
#[derive(Clone)]
pub struct Taskbar {
    backend: Rc<dyn TaskbarBackend>,
    window: WindowRef,
}

impl Taskbar {
    /// A handle over `backend` for the acquisition `ctx`, bound to the
    /// acquiring widget's window.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] with [`UnsupportedReason::NoWindow`] when the widget
    /// is in no window.
    pub fn try_new(
        backend: impl TaskbarBackend,
        ctx: &ProviderContext<'_>,
    ) -> Result<Self, Unsupported> {
        let window = ctx.window().ok_or(Unsupported::of::<TaskbarCapability>(
            UnsupportedReason::NoWindow,
        ))?;
        Ok(Self {
            backend: Rc::new(backend),
            window,
        })
    }

    /// Show `progress` on the window's button.
    ///
    /// # Errors
    ///
    /// [`TaskbarError::WindowClosed`] once the window has closed, without
    /// asking the system; otherwise why the system did not take it.
    pub fn set_progress(&self, progress: TaskbarProgress) -> Result<(), TaskbarError> {
        self.open()?;
        self.backend.set_progress(&self.window, progress)
    }

    /// Show `badge` on the window's button.
    ///
    /// # Errors
    ///
    /// As [`set_progress`](Self::set_progress).
    pub fn set_badge(&self, badge: TaskbarBadge) -> Result<(), TaskbarError> {
        self.open()?;
        self.backend.set_badge(&self.window, badge)
    }

    fn open(&self) -> Result<(), TaskbarError> {
        if self.window.lifetime().is_closed() {
            Err(TaskbarError::WindowClosed)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for Taskbar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Taskbar")
            .field("window", &self.window.id())
            .finish_non_exhaustive()
    }
}

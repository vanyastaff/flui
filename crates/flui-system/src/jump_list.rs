//! The Windows jump list: tasks the taskbar button's menu offers, each
//! starting the application with its own arguments (ADR-0154 §4: it needs
//! no object the host owns, so it is not core).
//!
//! A widget acquires [`JumpList`] through
//! `ctx.capability::<JumpListCapability>()`. Other systems have no jump
//! list and refuse it with
//! [`UnsupportedReason::NotOnThisPlatform`](flui_platform::UnsupportedReason::NotOnThisPlatform);
//! macOS's Dock menu is the application menu's, which is core.

use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

/// The jump list capability: `ctx.capability::<JumpListCapability>()` hands
/// out a [`JumpList`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum JumpListCapability {}

impl Capability for JumpListCapability {
    type Handle = JumpList;
    const NAME: &'static str = "jump-list";
}

/// One task of the jump list: a title the menu shows, and the arguments the
/// application is started with when the user picks it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct JumpListTask {
    /// The title the menu shows.
    pub title: String,
    /// The arguments the application starts with, each one argument
    /// whatever it holds: the backend quotes them.
    pub arguments: Vec<String>,
    /// The tooltip, if set.
    pub description: Option<String>,
}

impl JumpListTask {
    /// The longest title, description or argument, in bytes.
    pub const MAX_TEXT_LEN: usize = 260;

    /// The most arguments one task passes.
    pub const MAX_ARGUMENTS: usize = 32;

    /// A task titled `title` that starts the application with no arguments.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            arguments: Vec::new(),
            description: None,
        }
    }

    /// Also pass `argument`.
    #[must_use]
    pub fn argument(mut self, argument: impl Into<String>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    /// The tooltip.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn check(&self) -> Result<(), MalformedTask> {
        let is_line = |text: &str| {
            !text.trim().is_empty()
                && text.len() <= Self::MAX_TEXT_LEN
                && !text.chars().any(char::is_control)
        };
        if !is_line(&self.title) || !self.description.as_deref().is_none_or(is_line) {
            return Err(MalformedTask::Text);
        }
        if self.arguments.len() > Self::MAX_ARGUMENTS {
            return Err(MalformedTask::TooManyArguments);
        }
        // An argument may be empty or hold spaces and quotes, which the
        // backend quotes; a NUL would end it early.
        if self
            .arguments
            .iter()
            .any(|argument| argument.len() > Self::MAX_TEXT_LEN || argument.contains('\0'))
        {
            return Err(MalformedTask::Argument);
        }
        Ok(())
    }
}

/// What is wrong with a task list that [`JumpList`] refuses before the
/// system is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedTask {
    /// A title is blank, or a title or description is longer than
    /// [`JumpListTask::MAX_TEXT_LEN`] or holds a control character.
    #[error("a task's title or description cannot be shown as written")]
    Text,
    /// A task passes more than [`JumpListTask::MAX_ARGUMENTS`] arguments.
    #[error("a task passes too many arguments")]
    TooManyArguments,
    /// An argument is longer than [`JumpListTask::MAX_TEXT_LEN`] or holds a
    /// NUL.
    #[error("a task's argument cannot be passed as written")]
    Argument,
    /// The list holds more than [`JumpList::MAX_TASKS`] tasks.
    #[error("the jump list holds too many tasks")]
    TooManyTasks,
}

/// Why the jump list did not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum JumpListError {
    /// The tasks are malformed, as the [`MalformedTask`] says.
    #[error("the jump list is malformed: {0}")]
    InvalidTasks(#[from] MalformedTask),
    /// This system has no jump list; the [`Unsupported`] says why.
    #[error("the jump list cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a change answers.
pub type JumpListResult = Result<(), JumpListError>;

/// The system's jump list as [`JumpList`] reaches it.
///
/// Owner-local. A task list it receives is well formed; it answers through
/// the [`Resolver`] once the system has the list. Writing the list runs off
/// the owner thread.
pub trait JumpListBackend: 'static {
    /// Replace the application's tasks with `tasks`; an empty list clears
    /// them.
    fn set_tasks(&self, tasks: Vec<JumpListTask>, answer: Resolver<JumpListResult>);
}

/// Widget-facing jump list of the application.
///
/// Owner-thread (`!Send`), cheap to clone. The list is the application's,
/// not a window's: the last change wins.
#[derive(Clone)]
pub struct JumpList {
    backend: Rc<dyn JumpListBackend>,
    completions: CompletionSource,
}

impl JumpList {
    /// The most tasks one list holds: the taskbar shows a dozen or so.
    pub const MAX_TASKS: usize = 32;

    /// A handle over `backend` for the acquisition `ctx`.
    #[must_use]
    pub fn new(backend: impl JumpListBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Replace the tasks with `tasks`, in order.
    ///
    /// A malformed list is refused without asking the system
    /// ([`JumpListError::InvalidTasks`]).
    pub fn set_tasks(&self, tasks: Vec<JumpListTask>) -> Completion<JumpListResult> {
        let (completion, answer) = self.completions.pending();
        let checked = if tasks.len() > Self::MAX_TASKS {
            Err(MalformedTask::TooManyTasks)
        } else {
            tasks.iter().try_for_each(JumpListTask::check)
        };
        match checked {
            Ok(()) => self.backend.set_tasks(tasks, answer),
            // The owner retiring first leaves nothing waiting for the answer.
            Err(malformed) => drop(answer.resolve(Err(malformed.into()))),
        }
        completion
    }

    /// Remove every task.
    pub fn clear(&self) -> Completion<JumpListResult> {
        self.set_tasks(Vec::new())
    }
}

impl fmt::Debug for JumpList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JumpList").finish_non_exhaustive()
    }
}

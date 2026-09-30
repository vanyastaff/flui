//! The development-agent seam (ADR-0095 §3).
//!
//! A development tool that serves the application's semantics tree to an
//! agent — `flui-devtools`' `agent` server today — implements
//! [`DevAgentHook`]; the application installs the instance on its
//! configuration (`flui_app::AppConfig::with_dev_agent`), and the host hands
//! it an [`AgentWindow`] for each window that has content to read. The trait
//! lives here, below the runtime, so the host never names the tool and the
//! tool never names the runtime: `flui-app` has no edge to any devtools
//! crate, and a package reaches this module through `flui-sdk`
//! (`flui_sdk::view::dev_agent`).
//!
//! # What the host promises
//!
//! | Host | `attach` / `detach` | `window_opened` |
//! |------|---------------------|-----------------|
//! | desktop, iOS | yes | each window that mounts a root view |
//! | Android | no | no |
//! | web | no | no |
//!
//! The Android and web hosts log once, at start, that they drive no agent
//! hook when one is installed.
//!
//! - [`DevAgentHook::attach`] runs once per event loop, on the owner thread,
//!   before the first window opens; [`DevAgentHook::detach`] runs once when
//!   that loop ends. A hook is never attached twice without a detach between.
//! - [`DevAgentHook::window_opened`] runs on the owner thread once a window's
//!   realm is installed, and only between `attach` and `detach`. A window
//!   whose installation fails is never handed over.
//! - A hook that panics in any method is dropped and never called again;
//!   the window being opened opens without it.
//!
//! There is no close notification: an [`AgentWindow`] holds its window
//! weakly, so once the window closes every call answers `gone` with kind
//! `window`, and [`AgentWindow::is_open`] turns `false`.
//!
//! # What a hook promises
//!
//! - It never waits on an [`AgentAnswer`] inside a hook method, and never on
//!   the owner thread at all: the owner fills an answer only at its next
//!   drain, a frame boundary, so waiting there waits forever. Serve agents
//!   from threads of the hook's own.
//! - It keeps what it logs free of labels and values: a read returns the
//!   application's text.
//!
//! While a hook is installed, every published window collects semantics on
//! every frame, as it does while assistive technology is on.

use std::fmt;
use std::sync::Weak;
use std::time::Duration;

use flui_protocol::{ActionRequest, ErrorCode, ReadQuery, Retry, Tree, WindowId};

use crate::__runtime::{AgentPort, PendingAnswer};

/// A development tool that serves the application's windows to an agent,
/// installed by the application and called by the host on the owner thread.
/// See the [module docs](self) for the call order.
///
/// `Send` because the instance travels inside the application's
/// configuration, which is `Send`; the host still calls it only on the owner
/// thread, so an implementation needs no `Sync` and no locking of its own
/// for the calls themselves.
///
/// # Example
///
/// ```
/// use flui_view::dev_agent::{AgentWindow, DevAgentHook};
///
/// /// Keeps every window it is handed, for a server thread to read.
/// #[derive(Default)]
/// struct Windows(Vec<AgentWindow>);
///
/// impl DevAgentHook for Windows {
///     fn attach(&mut self) {}
///
///     fn detach(&mut self) {
///         self.0.clear();
///     }
///
///     fn window_opened(&mut self, window: AgentWindow) {
///         self.0.push(window);
///     }
/// }
/// ```
pub trait DevAgentHook: Send + 'static {
    /// Once per event loop, before the first window: start serving (bind an
    /// endpoint, start the threads that answer agents).
    fn attach(&mut self);

    /// The loop is ending: stop serving and let go of every window. The
    /// host pairs every `attach` with one `detach`.
    fn detach(&mut self) {}

    /// A window with content to read has opened.
    fn window_opened(&mut self, window: AgentWindow);
}

/// The kind of handle a `gone` or `unknown_handle` fault names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandleKind {
    /// An element handle, `e<n>`.
    Element,
    /// A window handle, `w<n>`.
    Window,
}

impl HandleKind {
    /// The name ADR-0080's error envelope carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Element => "element",
            Self::Window => "window",
        }
    }
}

impl fmt::Display for HandleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why an agent's read or action failed, in ADR-0080's terms: a fixed code,
/// retry advice, whether part of the action may have reached the
/// application, and the kind of handle a `gone` or `unknown_handle` names.
///
/// The message is for people and carries element and window ids only, never
/// a label or a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFault {
    code: ErrorCode,
    retry: Retry,
    may_have_run: bool,
    kind: Option<HandleKind>,
    message: String,
}

impl AgentFault {
    /// A fault with `code`, `retry` advice and a message for people.
    #[must_use]
    pub fn new(code: ErrorCode, retry: Retry, message: impl Into<String>) -> Self {
        Self {
            code,
            retry,
            may_have_run: false,
            kind: None,
            message: message.into(),
        }
    }

    /// The fault every call on a closed window answers.
    #[must_use]
    pub fn window_gone() -> Self {
        Self::new(ErrorCode::Gone, Retry::Never, "the window has closed")
            .with_kind(HandleKind::Window)
    }

    /// Name the kind of handle the fault is about.
    #[must_use]
    pub fn with_kind(mut self, kind: HandleKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Say whether part of the action may have reached the application
    /// (ADR-0080's `may_have_run` effect).
    #[must_use]
    pub fn with_may_have_run(mut self, may_have_run: bool) -> Self {
        self.may_have_run = may_have_run;
        self
    }

    /// The ADR-0080 code an agent branches on.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Whether the same call can succeed later.
    #[must_use]
    pub fn retry(&self) -> Retry {
        self.retry
    }

    /// Whether part of the action may have reached the application.
    #[must_use]
    pub fn may_have_run(&self) -> bool {
        self.may_have_run
    }

    /// The kind of handle a `gone` or `unknown_handle` names.
    #[must_use]
    pub fn kind(&self) -> Option<HandleKind> {
        self.kind
    }

    /// The message, for people.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AgentFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AgentFault {}

/// The answer to one [`AgentWindow::read`] or [`AgentWindow::act`], filled by
/// the window's owner at its next drain.
///
/// Never wait on it on the owner thread, or inside a [`DevAgentHook`] method:
/// the owner answers only at its drain.
#[must_use = "an answer carries the result of the read or action"]
pub struct AgentAnswer<T>(Box<dyn PendingAnswer<T>>);

impl<T> AgentAnswer<T> {
    pub(crate) fn new(pending: Box<dyn PendingAnswer<T>>) -> Self {
        Self(pending)
    }

    /// The answer, if the owner has sent it; `None` while it has not, and
    /// after the answer has been taken once.
    pub fn try_take(&mut self) -> Option<Result<T, AgentFault>> {
        self.0.try_take()
    }

    /// The answer, waiting up to `timeout` for it; `None` if it has not come
    /// by then, and after the answer has been taken once.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Result<T, AgentFault>> {
        self.0.recv_timeout(timeout)
    }
}

impl<T> fmt::Debug for AgentAnswer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentAnswer").finish_non_exhaustive()
    }
}

/// One window an agent can read and act on, from any thread.
///
/// Handed to [`DevAgentHook::window_opened`] by the host. `Clone + Send +
/// Sync`; it holds the window weakly, so it never keeps a closed window, or
/// its semantics collection, alive. Its element handles are scoped to this
/// window: another window can report the same `e<n>` for a different element.
#[derive(Clone)]
pub struct AgentWindow {
    id: WindowId,
    port: Weak<dyn AgentPort>,
}

impl AgentWindow {
    pub(crate) fn new(id: WindowId, port: Weak<dyn AgentPort>) -> Self {
        Self { id, port }
    }

    /// The window's handle, `w<n>` on the wire. Unique within the process.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Whether the window is still open. A window reported open can close
    /// before the next call, which then answers `gone`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.port.strong_count() > 0
    }

    /// Read the window's semantics tree as ADR-0080 wire nodes, as of the
    /// last committed frame.
    ///
    /// # Errors
    ///
    /// `gone` (kind `window`) once the window has closed; `busy` when the
    /// owner's inbox is full; `shutting_down` when the owner has gone. The
    /// answer carries the rest.
    pub fn read(&self, query: ReadQuery) -> Result<AgentAnswer<Tree>, AgentFault> {
        self.port
            .upgrade()
            .ok_or_else(AgentFault::window_gone)?
            .read(query)
    }

    /// Perform `request` on its element, checked against the element as the
    /// last committed frame shows it. An `Ok` answer means the action was
    /// delivered to the element's handler, not that its effect has happened:
    /// read the tree for the effect rather than acting again.
    ///
    /// # Errors
    ///
    /// As [`Self::read`]; the answer carries the rest.
    pub fn act(&self, request: ActionRequest) -> Result<AgentAnswer<()>, AgentFault> {
        self.port
            .upgrade()
            .ok_or_else(AgentFault::window_gone)?
            .act(request)
    }
}

impl fmt::Debug for AgentWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWindow")
            .field("id", &self.id)
            .field("open", &self.is_open())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    static_assertions::assert_impl_all!(AgentWindow: Send, Sync, Clone);
    static_assertions::assert_impl_all!(AgentAnswer<Tree>: Send);

    struct Answering;

    struct Ready<T>(Option<T>);

    impl<T: Send> PendingAnswer<T> for Ready<T> {
        fn try_take(&mut self) -> Option<Result<T, AgentFault>> {
            self.0.take().map(Ok)
        }

        fn recv_timeout(&mut self, _timeout: Duration) -> Option<Result<T, AgentFault>> {
            self.try_take()
        }
    }

    impl AgentPort for Answering {
        fn read(&self, _query: ReadQuery) -> Result<AgentAnswer<Tree>, AgentFault> {
            Ok(crate::__runtime::agent_answer(Ready(Some(Tree::new(
                Vec::new(),
                false,
            )))))
        }

        fn act(&self, _request: ActionRequest) -> Result<AgentAnswer<()>, AgentFault> {
            Ok(crate::__runtime::agent_answer(Ready(Some(()))))
        }
    }

    #[test]
    fn a_window_answers_through_its_port_until_the_port_is_gone() {
        let id = WindowId::from_u64(3).expect("non-zero");
        let port: Arc<dyn AgentPort> = Arc::new(Answering);
        let window = crate::__runtime::agent_window(id, Arc::downgrade(&port));
        assert!(window.is_open());
        let mut answer = window.read(ReadQuery::new()).expect("the port is alive");
        assert!(matches!(answer.try_take(), Some(Ok(_))));
        assert!(answer.try_take().is_none(), "an answer is taken once");

        drop(port);
        assert!(!window.is_open());
        let element = flui_protocol::ElementId::from_u64(1).expect("non-zero");
        for fault in [
            window.read(ReadQuery::new()).expect_err("closed"),
            window
                .act(ActionRequest::new(
                    element,
                    flui_protocol::ActionName::Invoke,
                ))
                .expect_err("closed"),
        ] {
            assert_eq!(
                (fault.code(), fault.retry(), fault.kind()),
                (ErrorCode::Gone, Retry::Never, Some(HandleKind::Window))
            );
            assert!(!fault.may_have_run());
        }
    }
}

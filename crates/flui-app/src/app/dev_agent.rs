//! The application's development agents (ADR-0095 §3, ADR-0239, ADR-0288 §7).
//!
//! `flui-app` names no devtools crate. An application that wants a tool of its
//! own to read and act on its windows installs a [`DevAgentHook`] on its
//! configuration ([`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent));
//! in a debug build, or with the `driver` feature, the runner adds the driver
//! agent (`app::agent`) beside it when the launch asks for one. Each hook's
//! containment is the runtime's (`flui_runtime::dev_agent`), shared with
//! `flui-testing`'s headless host: a hook that panics is dropped alone, and the
//! others keep being served.
//!
//! The event loop owns the hooks. A configuration holds each hook until a loop
//! takes it: when the loop starts it takes its configuration's hooks into a
//! `DevAgentHost` kept in the loop's state ([`LoopAgents`]), which attaches
//! them, and when it ends it drops the host, which detaches them. A hook
//! serves the first loop that takes it; a clone of the configuration made
//! before then shares it, and one used after has none left.
//!
//! - Every runner hands each window that mounts a root view to the loop's
//!   hooks once its UI runtime is installed, whatever configuration the
//!   application opened it with. The window's agent is vended while the UI
//!   runtime is still the runner's, so an installation that fails drops the
//!   agent with the UI runtime and no hook ever sees a half-installed window.
//! - The page's loop ends only with a quit (its driver going under
//!   `EXIT_ON_DISCONNECT`), which ends its hooks; a page that is closed
//!   ends with them attached.
//!
//! The host is lent out of the loop's state for each call, so no borrow of
//! that state is held while a hook runs: a call made meanwhile finds it lent
//! and does nothing, and a loop that ends meanwhile has the lender drop the
//! host when the call returns.

use std::fmt;
use std::sync::Arc;

use flui_runtime::dev_agent::DevAgentHost;
use flui_view::dev_agent::DevAgentHook;
use parking_lot::Mutex;

/// One hook installed on a configuration, until a loop takes it.
#[derive(Default)]
struct Pending(Mutex<Option<Box<dyn DevAgentHook>>>);

/// The development agent hooks installed on an [`AppConfig`](crate::AppConfig),
/// in the order they were installed, until a loop takes them.
///
/// Built by [`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent).
/// `Clone` shares the hooks, so whichever clone a loop starts with, the loop
/// takes them all.
#[derive(Clone)]
pub(crate) struct DevAgent(Vec<Arc<Pending>>);

impl DevAgent {
    /// `agents` with `hook` installed after the ones it holds.
    pub(crate) fn with(agents: Option<Self>, hook: impl DevAgentHook) -> Self {
        let hook: Box<dyn DevAgentHook> = Box::new(hook);
        let mut agents = agents.unwrap_or(Self(Vec::new()));
        agents.0.push(Arc::new(Pending(Mutex::new(Some(hook)))));
        agents
    }

    /// The hooks no loop has taken yet, in the order they were installed.
    pub(crate) fn take(&self) -> Vec<Box<dyn DevAgentHook>> {
        self.0
            .iter()
            .filter_map(|pending| pending.0.lock().take())
            .collect()
    }
}

impl fmt::Debug for DevAgent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let waiting = self
            .0
            .iter()
            .filter(|pending| pending.0.lock().is_some())
            .count();
        f.debug_struct("DevAgent")
            .field("installed", &self.0.len())
            .field("waiting", &waiting)
            .finish()
    }
}

/// The development agents of the loop running on this thread, in its state.
#[derive(Debug, Default)]
pub(crate) enum LoopAgents {
    /// No loop runs, or no hook of the running one serves.
    #[default]
    None,
    /// The loop's hooks, attached.
    Serving(DevAgentHost),
    /// A call has the host; it puts it back when it returns, unless the
    /// loop ended or another started meanwhile.
    Lent,
}

impl LoopAgents {
    /// Lend the host out for one call.
    pub(crate) fn lend(&mut self) -> Option<DevAgentHost> {
        match std::mem::replace(self, Self::Lent) {
            Self::Serving(host) => Some(host),
            other @ (Self::None | Self::Lent) => {
                *self = other;
                None
            }
        }
    }

    /// Put back a lent host; the host when its loop is gone, for the caller
    /// to drop outside the borrow.
    #[must_use = "a host returned here is dropped outside the borrow"]
    pub(crate) fn restore(&mut self, host: DevAgentHost) -> Option<DevAgentHost> {
        if matches!(self, Self::Lent) {
            *self = Self::Serving(host);
            None
        } else {
            Some(host)
        }
    }

    /// End the loop's agents; the host, for the caller to drop outside the
    /// borrow. A lent host is dropped by its lender.
    #[must_use = "the host returned here is dropped outside the borrow"]
    pub(crate) fn end(&mut self) -> Option<DevAgentHost> {
        match std::mem::take(self) {
            Self::Serving(host) => Some(host),
            Self::None | Self::Lent => None,
        }
    }

    /// Whether a hook of the running loop serves.
    #[cfg(test)]
    pub(crate) fn is_serving(&self) -> bool {
        match self {
            Self::Serving(host) => host.is_serving(),
            Self::Lent => true,
            Self::None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_runtime::dev_agent::DevAgentHost;
    use flui_view::dev_agent::{AgentWindow, DevAgentHook};

    use super::LoopAgents;
    use crate::AppConfig;

    /// Counts its attaches and detaches; panics in `attach` when told to.
    struct Hook {
        attaches: Arc<AtomicUsize>,
        detaches: Arc<AtomicUsize>,
        panics: bool,
    }

    impl DevAgentHook for Hook {
        fn attach(&mut self) -> bool {
            self.attaches.fetch_add(1, Ordering::SeqCst);
            assert!(!self.panics, "a hook that fails to attach");
            true
        }
        fn detach(&mut self) {
            self.detaches.fetch_add(1, Ordering::SeqCst);
        }
        fn window_opened(&mut self, _window: AgentWindow) {}
    }

    fn hook(panics: bool) -> (Hook, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let attaches = Arc::new(AtomicUsize::new(0));
        let detaches = Arc::new(AtomicUsize::new(0));
        let hook = Hook {
            attaches: Arc::clone(&attaches),
            detaches: Arc::clone(&detaches),
            panics,
        };
        (hook, attaches, detaches)
    }

    #[test]
    fn a_loop_takes_every_installed_hook_once_and_one_that_panics_is_dropped_alone() {
        let (first, first_attaches, first_detaches) = hook(false);
        let (failing, failing_attaches, failing_detaches) = hook(true);
        let (last, last_attaches, last_detaches) = hook(false);
        let config = AppConfig::new()
            .with_dev_agent(first)
            .with_dev_agent(failing)
            .with_dev_agent(last);
        let shared = config.clone();
        let agents = config.dev_agent.as_ref().expect("hooks installed");

        let host = DevAgentHost::attach(agents.take());
        let counts = || {
            [
                &first_attaches,
                &first_detaches,
                &failing_attaches,
                &failing_detaches,
                &last_attaches,
                &last_detaches,
            ]
            .map(|count| count.load(Ordering::SeqCst))
        };
        assert_eq!(counts(), [1, 0, 1, 0, 1, 0], "each hook attached once");
        assert!(host.is_serving(), "two hooks serve");
        assert!(
            shared
                .dev_agent
                .as_ref()
                .expect("the clone shares the hooks")
                .take()
                .is_empty(),
            "a clone of the configuration finds the hooks taken"
        );
        drop(host);
        assert_eq!(
            counts(),
            [1, 1, 1, 0, 1, 1],
            "the serving hooks detached; the one that panicked is never detached"
        );
    }

    #[test]
    fn a_host_lent_when_its_loop_ends_is_handed_back_to_be_dropped() {
        let (serving, attaches, detaches) = hook(false);
        let serving: Box<dyn DevAgentHook> = Box::new(serving);
        let mut agents = LoopAgents::Serving(DevAgentHost::attach([serving]));
        let host = agents.lend().expect("the loop's host is lent");
        assert!(agents.lend().is_none(), "a nested call finds it lent");
        assert!(agents.end().is_none(), "the lender keeps the host");
        let gone = agents.restore(host).expect("the loop ended meanwhile");
        assert_eq!(detaches.load(Ordering::SeqCst), 0);
        drop(gone);
        assert_eq!(
            (
                attaches.load(Ordering::SeqCst),
                detaches.load(Ordering::SeqCst)
            ),
            (1, 1),
            "the lender's drop detached the hook once"
        );
        assert!(!agents.is_serving());
    }
}

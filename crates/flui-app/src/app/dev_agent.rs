//! The application's development agents (ADR-0095 §3, ADR-0239).
//!
//! `flui-app` names no devtools crate. An application that wants a tool of its
//! own to read and act on its windows installs a [`DevAgentHook`] on its
//! configuration ([`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent));
//! in a debug build, or with the `driver` feature, the runner adds the driver
//! agent (`app::agent`) beside it when the launch asks for one. The runners
//! reach the hooks only through the crate-private calls below, and each hook's
//! containment is the runtime's (`flui_runtime::dev_agent`), shared with
//! `flui-testing`'s headless host: a hook that panics is dropped alone, and the
//! others keep being served.
//!
//! - desktop, iOS and Android: the loop attaches the hooks when it starts and
//!   detaches them when it ends; each window that mounts a root view is handed
//!   over once its UI runtime is installed. The window's agent is vended while
//!   the UI runtime is still the runner's, so an installation that fails drops
//!   the agent with the UI runtime and no hook ever sees a half-installed
//!   window.
//! - web: the same calls for the page's one window. The page's loop never
//!   ends while the page lives, so the hooks are never detached.

use std::fmt;

use flui_runtime::dev_agent::DevAgentHost;
use flui_view::dev_agent::DevAgentHook;

/// The development agent hooks installed on an [`AppConfig`](crate::AppConfig),
/// in the order they were installed.
///
/// Built by [`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent).
/// `Clone` shares the hooks, so every window opened with clones of the same
/// configuration is handed to the same hooks.
#[derive(Clone)]
pub(crate) struct DevAgent(Vec<DevAgentHost>);

impl DevAgent {
    pub(crate) fn new(hook: impl DevAgentHook) -> Self {
        Self(vec![DevAgentHost::new(hook)])
    }

    /// `agents` with `hook` installed after the ones it holds.
    pub(crate) fn with(agents: Option<Self>, hook: impl DevAgentHook) -> Self {
        match agents {
            Some(mut agents) => {
                agents.0.push(DevAgentHost::new(hook));
                agents
            }
            None => Self::new(hook),
        }
    }
}

impl fmt::Debug for DevAgent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.0).finish()
    }
}

mod driven {
    use flui_foundation::PresentationId;
    use flui_runtime::dev_agent::{DevAgentAttachment, DevAgentHost};
    use flui_view::dev_agent::AgentWindow;

    use super::DevAgent;
    use crate::app::ui_runtime::UiRuntime;

    /// The hooks one loop attached; dropping it detaches each of them, in the
    /// order they were attached.
    #[must_use = "dropping the attachments detaches the hooks"]
    pub(crate) struct DevAgentAttachments(Vec<DevAgentAttachment>);

    impl std::fmt::Debug for DevAgentAttachments {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_list().entries(&self.0).finish()
        }
    }

    impl DevAgent {
        /// Attach every hook for this loop; the attachments detach them when
        /// dropped, so they must outlive the loop. `None` when no hook
        /// attached: each was already attached, panicked, or does not serve.
        pub(crate) fn attach(&self) -> Option<DevAgentAttachments> {
            let attached: Vec<DevAgentAttachment> =
                self.0.iter().filter_map(DevAgentHost::attach).collect();
            (!attached.is_empty()).then_some(DevAgentAttachments(attached))
        }

        /// Vend `presentation`'s agent window while the runner still owns
        /// the UI runtime; `None` unless a hook is attached. Every hook is
        /// handed a clone of the one window, so they share its element
        /// handles.
        pub(crate) fn vend(
            &self,
            ui_runtime: &UiRuntime,
            presentation: PresentationId,
        ) -> Option<AgentWindow> {
            self.0
                .iter()
                .find_map(|host| host.vend(ui_runtime, presentation))
        }

        /// Hand an installed window to every attached hook.
        pub(crate) fn window_opened(&self, window: AgentWindow) {
            for host in &self.0 {
                host.window_opened(window.clone());
            }
        }
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) use driven::DevAgentAttachments;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_view::dev_agent::{AgentWindow, DevAgentHook};

    use super::DevAgent;
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
    fn every_installed_hook_is_attached_and_one_that_panics_is_dropped_alone() {
        let (first, first_attaches, first_detaches) = hook(false);
        let (failing, failing_attaches, failing_detaches) = hook(true);
        let (last, last_attaches, last_detaches) = hook(false);
        let config = AppConfig::new()
            .with_dev_agent(first)
            .with_dev_agent(failing)
            .with_dev_agent(last);
        let agents: &DevAgent = config.dev_agent.as_ref().expect("hooks installed");

        let attached = agents.attach().expect("two hooks serve");
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
        drop(attached);
        assert_eq!(
            counts(),
            [1, 1, 1, 0, 1, 1],
            "the serving hooks detached; the one that panicked is never detached"
        );
        assert!(
            agents.attach().is_some(),
            "the next loop attaches the hooks that still serve"
        );
    }
}

//! The application's development agent (ADR-0095 §3).
//!
//! `flui-app` names no devtools crate. An application that wants an agent to
//! read and act on its windows installs a [`DevAgentHook`] on its
//! configuration ([`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent));
//! the runners reach it only through the crate-private calls below, and the
//! hook's containment is the runtime's (`flui_runtime::dev_agent`), shared
//! with `flui-testing`'s headless host.
//!
//! - desktop and iOS: the loop attaches the hook when it starts and detaches
//!   it when it ends; each window that mounts a root view is handed over once
//!   its realm is installed. The window's agent is vended while the realm is
//!   still the runner's, so an installation that fails drops the agent with
//!   the realm and the hook never sees a half-installed window.
//! - Android and web: no call; the runner logs once, at start, that an
//!   installed hook is not driven there.

use std::fmt;

use flui_runtime::dev_agent::DevAgentHost;
use flui_view::dev_agent::DevAgentHook;

/// A development agent hook installed on an [`AppConfig`](crate::AppConfig).
///
/// Built by [`AppConfig::with_dev_agent`](crate::AppConfig::with_dev_agent).
/// `Clone` shares the one hook, so every window opened with clones of the
/// same configuration is handed to the same hook.
#[derive(Clone)]
pub struct DevAgent(DevAgentHost);

impl DevAgent {
    pub(crate) fn new(hook: impl DevAgentHook) -> Self {
        Self(DevAgentHost::new(hook))
    }
}

impl fmt::Debug for DevAgent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DevAgent").field(&self.0).finish()
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
mod driven {
    use flui_foundation::PresentationId;
    use flui_runtime::dev_agent::DevAgentAttachment;
    use flui_view::dev_agent::AgentWindow;

    use super::DevAgent;
    use crate::app::ui_realm::UiRealm;

    impl DevAgent {
        /// Attach the hook for this loop; the attachment detaches it when
        /// dropped, so it must outlive the loop. `None` when the hook is
        /// already attached, or panicked.
        pub(crate) fn attach(&self) -> Option<DevAgentAttachment> {
            self.0.attach()
        }

        /// Vend `presentation`'s agent window while the runner still owns
        /// the realm; `None` unless the hook is attached.
        pub(crate) fn vend(
            &self,
            realm: &UiRealm,
            presentation: PresentationId,
        ) -> Option<AgentWindow> {
            self.0.vend(realm, presentation)
        }

        /// Hand an installed window to the hook.
        pub(crate) fn window_opened(&self, window: AgentWindow) {
            self.0.window_opened(window);
        }
    }
}

/// Logs, once per runner start, that this host drives no agent hook.
#[cfg(any(target_os = "android", target_arch = "wasm32"))]
pub(crate) fn log_undriven(config: &crate::AppConfig, host: &'static str) {
    if config.dev_agent.is_some() {
        tracing::info!(
            host,
            "development agent hook installed, but this host drives none; no agent is served"
        );
    }
}

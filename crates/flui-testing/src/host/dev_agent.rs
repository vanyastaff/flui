//! A development-agent hook attached the way a runner's event loop attaches it.

#[cfg(doc)]
use super::HeadlessHost;

use flui_runtime::dev_agent::DevAgentHost;

use flui_view::dev_agent::DevAgentHook;

/// A development-agent hook attached the way a runner's event loop attaches
/// it: once, when this value is built, and detached when it is dropped.
///
/// Every [`HeadlessHost`] built [`with`](HeadlessHost::with_dev_agent) it
/// hands the hook its window, as a runner hands it each window it opens, so
/// one hook can serve several UI runtimes, and a UI runtime dropped before the hook is
/// a window that closed while the tool kept running. Containment is the
/// runtime's (`flui_runtime::dev_agent`): a hook that panics is dropped, and
/// the UI runtimes keep pumping.
pub struct HeadlessDevAgent {
    pub(super) host: DevAgentHost,
}

impl HeadlessDevAgent {
    /// Take `hook` and attach it, as a runner does when its loop starts.
    #[must_use]
    pub fn attach(hook: impl DevAgentHook) -> Self {
        let hook: Box<dyn DevAgentHook> = Box::new(hook);
        Self {
            host: DevAgentHost::attach([hook]),
        }
    }

    /// Whether the hook is attached: it served and did not panic in `attach`
    /// or since.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.host.is_serving()
    }
}

impl std::fmt::Debug for HeadlessDevAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDevAgent")
            .field("host", &self.host)
            .finish()
    }
}

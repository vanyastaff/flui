//! The development agents of the loop running on this thread (ADR-0288 §7):
//! the loop's `DevAgentHost`, kept in its state and lent out for each call.
//! See `app::dev_agent` for when each runner calls these.

use flui_foundation::PresentationId;
use flui_runtime::dev_agent::DevAgentHost;
use flui_view::dev_agent::AgentWindow;

use super::host::APP_RUNTIME;
use crate::app::AppConfig;
use crate::app::dev_agent::LoopAgents;
use crate::app::ui_runtime::UiRuntime;

/// Start this loop's development agents: take `config`'s hooks and attach
/// them. Agents a loop on this thread failed to end are ended first.
pub(super) fn start(config: &AppConfig) {
    let hooks = config
        .dev_agent
        .as_ref()
        .map(crate::app::dev_agent::DevAgent::take)
        .unwrap_or_default();
    // Attached with no borrow of the loop's state held: `attach` is the
    // hooks' code.
    let host = (!hooks.is_empty()).then(|| DevAgentHost::attach(hooks));
    let agents = match host {
        Some(host) if host.is_serving() => LoopAgents::Serving(host),
        _ => LoopAgents::None,
    };
    let stale = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let stale = runtime.loop_dev_agent.end();
        runtime.loop_dev_agent = agents;
        stale
    });
    // Its hooks detach outside the borrow.
    drop(stale);
}

/// `presentation`'s agent window while a hook of this loop serves; `None`
/// otherwise, and no semantics work is started.
pub(super) fn vend(ui_runtime: &UiRuntime, presentation: PresentationId) -> Option<AgentWindow> {
    lend(|host| host.vend(ui_runtime, presentation)).flatten()
}

/// Hand an installed window to this loop's hooks.
pub(super) fn window_opened(window: AgentWindow) {
    let _: Option<()> = lend(|host| host.window_opened(window));
}

/// End this loop's development agents: each hook is detached, then dropped.
/// The desktop loop ends them with its main window (`shutdown_main_window`).
#[cfg(any(target_os = "ios", target_os = "android", target_arch = "wasm32", test))]
pub(super) fn end() {
    let host = APP_RUNTIME.with(|slot| slot.borrow_mut().loop_dev_agent.end());
    // Detached outside the borrow.
    drop(host);
}

/// Whether a hook of the loop running on this thread serves.
// The desktop loop's tests read it.
#[cfg(all(
    test,
    not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
))]
pub(super) fn is_serving() -> bool {
    APP_RUNTIME.with(|slot| slot.borrow().loop_dev_agent.is_serving())
}

/// Run `call` with the loop's host lent out of its state; `None` when no
/// hook serves or a call already has the host. The host goes back before a
/// panic of `call` resumes, so the next call finds it.
fn lend<R>(call: impl FnOnce(&mut DevAgentHost) -> R) -> Option<R> {
    let mut host = APP_RUNTIME.with(|slot| slot.borrow_mut().loop_dev_agent.lend())?;
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(&mut host)));
    let gone = APP_RUNTIME.with(|slot| slot.borrow_mut().loop_dev_agent.restore(host));
    // The loop ended, or another started, while the host was lent: its hooks
    // detach here, outside the borrow.
    drop(gone);
    match value {
        Ok(value) => Some(value),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Ends this loop's development agents when dropped, for a runner whose loop
/// ends where `run` returns or unwinds.
#[cfg(target_os = "android")]
#[must_use = "dropping it ends the loop's development agents"]
pub(super) struct EndOnDrop;

#[cfg(target_os = "android")]
impl Drop for EndOnDrop {
    fn drop(&mut self) {
        end();
    }
}

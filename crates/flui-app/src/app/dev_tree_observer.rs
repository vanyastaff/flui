//! The development tree observer: a factory an application installs with
//! [`AppConfig::dev_tree_observer`](super::AppConfig::dev_tree_observer),
//! which every UI runtime the runner builds asks once for the observer of its
//! tree (ADR-0040).

use std::sync::Arc;

use flui_foundation::UiRuntimeId;
use flui_foundation::observe::TreeObserver;
use flui_foundation::panic::{FirstPanic, payload_text};
use flui_runtime::ui_runtime::UiRuntime;

/// The factory type the configuration carries. `Send + Sync` because a
/// configuration crosses threads to the loop that opens its windows; the
/// observer it makes stays on the runtime's owner thread.
type Factory = dyn Fn(UiRuntimeId) -> Arc<dyn TreeObserver> + Send + Sync;

/// The installed factory. `Clone` shares it, so every window opened with a
/// clone of the configuration asks the one factory.
#[derive(Clone)]
pub(crate) struct DevTreeObserver(Arc<Factory>);

impl std::fmt::Debug for DevTreeObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevTreeObserver").finish_non_exhaustive()
    }
}

impl DevTreeObserver {
    pub(crate) fn new(
        factory: impl Fn(UiRuntimeId) -> Arc<dyn TreeObserver> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(factory))
    }

    /// Ask the factory for `runtime`'s observer and install it through the
    /// seeded install, which replays the tree already mounted before the
    /// observer's first live event (ADR-0040 §3). The observer is the primary
    /// presentation's: its build owner holds it, and ends its stream with
    /// `detached()` when it drops with the retiring runtime.
    ///
    /// The factory is application code, called with no lock held. If it
    /// panics, the panic is logged and destroyed under containment, and the
    /// runtime runs without an observer.
    pub(crate) fn install_on(&self, runtime: &UiRuntime) {
        let runtime_id = runtime.id();
        let mut caught = FirstPanic::new();
        let Some(observer) = caught.contain(|| (self.0)(runtime_id)) else {
            let panic = caught.held().and_then(payload_text).map(str::to_owned);
            FirstPanic::new().contain(|| {
                tracing::error!(
                    ui_runtime_id = runtime_id.as_u64(),
                    panic = panic.as_deref().unwrap_or("<non-text payload>"),
                    "the development tree-observer factory panicked; this UI runtime runs \
                     without a tree observer"
                );
            });
            return;
        };
        runtime.widgets().install_tree_observer(observer);
    }
}

//! Worker ↔ host dispatch helpers (Phase B).
//!
//! Worker crates register reloadable `build()` bodies via `flui_worker_init`.
//! The host registers a [`set_request_rebuild`] hook so gesture handlers inside
//! the worker can schedule a frame without linking `flui-app`.

use std::sync::OnceLock;

use flui_view::BuildContext;

static REQUEST_REBUILD: OnceLock<fn()> = OnceLock::new();

/// Register the host hook that schedules a widget rebuild (typically
/// `WidgetsBinding::perform_reassemble` + `AppBinding::request_redraw`).
pub fn set_request_rebuild(hook: fn()) {
    let _ = REQUEST_REBUILD.set(hook);
}

/// Ask the host to rebuild dirty elements on the next frame.
///
/// No-op with a warning when the host has not registered a hook yet.
pub fn request_rebuild() {
    if let Some(hook) = REQUEST_REBUILD.get() {
        hook();
    } else {
        tracing::warn!(
            "flui_hot_reload::request_rebuild called before host registered a hook"
        );
    }
}

/// Context passed from the host-owned `ViewState::build` into worker code.
pub struct WorkerBuildEnv<'a> {
    ctx: &'a dyn BuildContext,
}

impl std::fmt::Debug for WorkerBuildEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerBuildEnv")
            .field("element_id", &self.ctx.element_id())
            .finish_non_exhaustive()
    }
}

impl<'a> WorkerBuildEnv<'a> {
    /// Wrap the framework `BuildContext` for a worker build call.
    pub fn new(ctx: &'a dyn BuildContext) -> Self {
        Self { ctx }
    }

    /// Underlying framework build context.
    pub fn framework_ctx(&self) -> &'a dyn BuildContext {
        self.ctx
    }

    /// Schedule a host rebuild (see [`request_rebuild`]).
    pub fn request_rebuild(&self) {
        request_rebuild();
    }
}

//! Shared types for the hot-reload counter example.
//!
//! Linked into both the host binary and the reloadable worker dylib.
//! `ViewState` memory lives in the host element tree; only the worker-provided
//! UI body changes across reload.

use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
    OnceLock,
};

use flui_hot_reload::WorkerBuildEnv;
use flui_view::prelude::*;

/// Stable-layout fingerprint — bump when `CounterAppState` layout changes.
pub const TYPE_FINGERPRINT: u64 = 0xC0_07_EA_0001;

type CounterBuildFn = fn(WorkerBuildEnv<'_>, &CounterAppState, &CounterApp) -> BoxedView;

static COUNTER_BUILD: OnceLock<CounterBuildFn> = OnceLock::new();

/// Register the worker's reloadable UI builder (called from `flui_worker_init`).
pub fn set_counter_build(build: CounterBuildFn) {
    let _ = COUNTER_BUILD.set(build);
}

/// Root stateful widget — `count` is shared with the worker for tap handlers.
#[derive(Clone, Debug, StatefulView)]
pub struct CounterApp {
    /// Initial value (used only when state is first created).
    pub initial: i32,
    /// Live counter storage (survives worker reload).
    pub count: Arc<AtomicI32>,
}

/// Persistent state owned by the host element tree.
#[derive(Debug)]
pub struct CounterAppState {
    count: Arc<AtomicI32>,
}

impl CounterAppState {
    /// Shared counter storage (readable from worker UI code).
    pub fn count(&self) -> &Arc<AtomicI32> {
        &self.count
    }
}

impl StatefulView for CounterApp {
    type State = CounterAppState;

    fn create_state(&self) -> Self::State {
        self.count.store(self.initial, Ordering::Relaxed);
        CounterAppState {
            count: Arc::clone(&self.count),
        }
    }
}

impl ViewState<CounterApp> for CounterAppState {
    fn build(&self, view: &CounterApp, ctx: &dyn BuildContext) -> impl IntoView {
        let build = COUNTER_BUILD.get().expect(
            "counter worker not loaded — set FLUI_WORKER_PLUGIN and build the logic crate",
        );
        let env = WorkerBuildEnv::new(ctx);
        build(env, self, view)
    }
}

/// Stateless shell so `flui_app::run_app` can mount a stateful subtree.
#[derive(Clone, Debug, StatelessView)]
pub struct CounterShell {
    /// Stateful counter mounted as the sole child.
    pub app: CounterApp,
}

impl StatelessView for CounterShell {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.app.clone()
    }
}

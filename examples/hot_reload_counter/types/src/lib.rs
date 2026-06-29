//! Shared types for the hot-reload counter example.
//!
//! Linked into both the host binary and the reloadable worker dylib.
//! `ViewState` memory lives in the host element tree; only the worker-provided
//! UI body changes across reload.

use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};

use flui_hot_reload::WorkerBuildEnv;
use flui_view::prelude::*;

/// Stable-layout fingerprint — bump when `CounterAppState` layout changes.
pub const TYPE_FINGERPRINT: u64 = 0xC0_07_EA_0001;

type CounterBuildFn = fn(WorkerBuildEnv<'_>, &CounterAppState, &CounterApp) -> BoxedView;

fn get_counter_build() -> CounterBuildFn {
    let ptr = flui_hot_reload::get_worker_build_ptr(TYPE_FINGERPRINT)
        .expect("counter worker not loaded — build the logic crate and set FLUI_WORKER_PLUGIN");
    // SAFETY: the worker registers this pointer via host-owned storage in
    // `flui_worker_init` with the same `CounterBuildFn` signature.
    unsafe { std::mem::transmute(ptr) }
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
        let build = get_counter_build();
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

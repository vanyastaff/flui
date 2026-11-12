//! The runtime the host runs its IO on (ADR-0327 §3).
//!
//! The Linux hosts watch logind, the settings portal and the IBus bus, and
//! the Wayland host loads its title fonts, on tokio; no other host runs IO.
//! The runtime is the one the host's creator hands it: `flui-app` passes its
//! execution services' IO runtime with [`NativeExecutor::from_tokio`], so a
//! process runs one IO pool (ADR-0047). A host started without an application
//! (its own tests, headless, `current_platform`) takes
//! [`NativeExecutor::own`], a small runtime that starts no thread until its
//! first use.

use std::sync::{Arc, OnceLock};

use tokio::runtime::{Handle, Runtime};

/// Worker threads of the host's own runtime. Small and fixed: an application
/// hands the host its IO runtime instead, so this one serves only a host
/// started without one, and must never claim a full-core pool.
const WORKER_THREADS: usize = 2;

/// The last clone of an own runtime may be released from one of its tasks,
/// where `Runtime`'s default drop, which waits for its workers, cannot run.
struct RuntimeOwner(OnceLock<Runtime>);

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// The tokio runtime a host spawns its IO on. A handle (ADR-0323 §2): its
/// clones reach the pool's threads, and the tasks it spawns run there.
///
/// A source that spawns on it keeps the task's `JoinHandle` and aborts it in
/// its `Drop`; dropping the future closes what it held (ADR-0327 §4).
#[derive(Clone)]
pub struct NativeExecutor(Inner);

#[derive(Clone)]
#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "only the Linux hosts run IO on tokio")
)]
enum Inner {
    /// A runtime of the host's own, started on first use.
    Own(Arc<RuntimeOwner>),
    /// The runtime the host's creator runs, whose IO and time drivers it
    /// enabled.
    Borrowed(Handle),
}

impl NativeExecutor {
    /// A runtime of the host's own: two workers named `flui-native-bg`,
    /// started on the first spawn, shut down without waiting when the last
    /// clone is released. For a host started without an application.
    #[must_use]
    pub fn own() -> Self {
        Self(Inner::Own(Arc::new(RuntimeOwner(OnceLock::new()))))
    }

    /// The runtime `runtime` belongs to, which must have its IO and time
    /// drivers enabled (`Builder::enable_all`); the host's sources wait on
    /// sockets and timers there. `flui-app` passes its execution services'
    /// IO runtime.
    #[must_use]
    pub fn from_tokio(runtime: Handle) -> Self {
        Self(Inner::Borrowed(runtime))
    }

    /// The runtime to spawn on, starting an own runtime on first use.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "only the Linux hosts run IO on tokio")
    )]
    pub(crate) fn handle(&self) -> Handle {
        match &self.0 {
            Inner::Own(owner) => owner.runtime().handle().clone(),
            Inner::Borrowed(handle) => handle.clone(),
        }
    }
}

impl RuntimeOwner {
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "only the Linux hosts run IO on tokio")
    )]
    fn runtime(&self) -> &Runtime {
        self.0.get_or_init(|| {
            tracing::info!(
                worker_threads = WORKER_THREADS,
                "starting the host's own runtime on first use"
            );
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(WORKER_THREADS)
                .thread_name("flui-native-bg")
                .enable_all()
                .build()
                .expect(
                    "BUG: building a tokio runtime must succeed on any platform this \
                     crate targets short of OS thread exhaustion",
                )
        })
    }
}

impl std::fmt::Debug for NativeExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.0 {
            Inner::Own(_) => "own",
            Inner::Borrowed(_) => "borrowed",
        };
        f.debug_tuple("NativeExecutor").field(&kind).finish()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::NativeExecutor;

    /// The thread a task spawned on `executor` runs on.
    fn spawned_thread_name(executor: &NativeExecutor) -> String {
        let (sent, received) = mpsc::channel();
        drop(executor.handle().spawn(async move {
            let name = std::thread::current().name().unwrap_or_default().to_owned();
            sent.send(name).expect("BUG: the test waits for the name");
        }));
        received
            .recv_timeout(Duration::from_secs(5))
            .expect("the spawned task ran")
    }

    fn borrowed_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("lent-io")
            .enable_all()
            .build()
            .expect("a test runtime")
    }

    #[test]
    fn a_borrowed_executor_spawns_on_the_runtime_it_was_handed() {
        let lent = borrowed_runtime();
        let executor = NativeExecutor::from_tokio(lent.handle().clone());
        assert_eq!(spawned_thread_name(&executor), "lent-io");
    }

    #[test]
    fn an_own_executor_spawns_on_its_own_threads() {
        assert_eq!(
            spawned_thread_name(&NativeExecutor::own()),
            "flui-native-bg"
        );
    }

    /// A source's task parked at an `await` forever holds neither its owner's
    /// drop nor the borrowed runtime's shutdown (ADR-0327 §4).
    #[test]
    fn a_hung_source_task_ends_with_its_owner_and_with_the_runtime() {
        let lent = borrowed_runtime();
        let executor = NativeExecutor::from_tokio(lent.handle().clone());

        let aborted = executor.handle().spawn(std::future::pending::<()>());
        aborted.abort();
        let outcome = lent.block_on(aborted);
        assert!(
            outcome.is_err_and(|error| error.is_cancelled()),
            "an aborted source task is cancelled at its await"
        );

        let parked = executor.handle().spawn(std::future::pending::<()>());
        let started = std::time::Instant::now();
        lent.shutdown_timeout(Duration::from_secs(1));
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "the runtime's shutdown does not wait for a task parked at an await"
        );
        assert!(parked.is_finished(), "the shutdown dropped the parked task");
    }

    /// A task that panics leaves the runtime running the next one.
    #[test]
    fn a_panicking_task_leaves_the_runtime_usable() {
        let executor = NativeExecutor::own();
        let failed = executor
            .handle()
            .spawn(async { panic!("a task that panics") });
        let (done, finished) = mpsc::channel();
        drop(executor.handle().spawn(async move {
            assert!(failed.await.is_err_and(|error| error.is_panic()));
            done.send(()).expect("BUG: the test waits");
        }));
        finished
            .recv_timeout(Duration::from_secs(5))
            .expect("the next task ran");
    }

    /// The last clone of an own runtime may be released inside another
    /// runtime's task or inside one of its own tasks.
    #[test]
    fn the_last_own_executor_retires_inside_a_task() {
        let foreign = NativeExecutor::own();
        let retiring = NativeExecutor::own();
        let (done, finished) = mpsc::channel();
        drop(retiring.handle().spawn(async {}));
        drop(foreign.handle().spawn(async move {
            drop(retiring);
            done.send(()).expect("BUG: the test waits");
        }));
        finished
            .recv_timeout(Duration::from_secs(5))
            .expect("released inside another runtime's task");

        let executor = NativeExecutor::own();
        let final_owner = executor.clone();
        let (start, begin) = tokio::sync::oneshot::channel();
        let (done, finished) = mpsc::channel();
        drop(executor.handle().spawn(async move {
            begin.await.expect("BUG: the test starts it");
            drop(final_owner);
            done.send(()).expect("BUG: the test waits");
        }));
        drop(executor);
        start.send(()).expect("the last owner's task is alive");
        finished
            .recv_timeout(Duration::from_secs(5))
            .expect("released inside its own task");
    }
}

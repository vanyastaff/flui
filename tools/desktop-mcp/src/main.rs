//! `flui-desktop-mcp`: an MCP server (stdio) that lets an agent drive any
//! desktop application the way a person with a screen reader does — list and
//! capture windows, read the accessibility tree, invoke element actions, and
//! send real pointer and keyboard input. See `README.md` for the tools and
//! how to register the server.
//!
//! stdout carries the MCP protocol; logs go to stderr (`RUST_LOG` filters).

mod a11y;
mod cache;
mod capture;
mod desktop;
mod error;
mod geometry;
mod input;
mod keys;
mod os;
mod params;
mod process;
mod server;
mod worker;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use rmcp::ServiceExt as _;
use tracing_subscriber::EnvFilter;

use crate::process::Children;
use crate::server::DesktopServer;
use crate::worker::Worker;

/// How long shutdown waits before warning about delayed input cleanup.
/// This is not an exit deadline: a provider call may still be holding the
/// desktop thread while input remains pressed.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(15);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,xcap=off".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // Before any window, capture or input call: fixes the coordinate space.
    // Without per-monitor awareness UIA rects, captures and pointer
    // coordinates are scaled apart, so a point checked against one window
    // could be injected over another: refuse to serve rather than guess.
    #[cfg(target_os = "windows")]
    os::init_dpi().context("making the server per-monitor DPI aware")?;

    let worker = Worker::spawn().context("starting the desktop thread")?;
    let children = Arc::new(Children::new());
    let server = DesktopServer::new(worker.clone(), Arc::clone(&children));

    // Stdin at EOF is the client leaving: input stops and queued calls are
    // skipped from then, not only once rmcp has drained its requests (up to
    // seconds, while queued keystrokes would still be typed).
    let service = server
        .serve((ShutdownOnEof(tokio::io::stdin()), tokio::io::stdout()))
        .await
        .context("starting the MCP session")?;
    // A host that closes stdin and then sends SIGTERM (or a Ctrl+C at a
    // terminal) gets the same clean shutdown as a closed session.
    tokio::select! {
        reason = service.waiting() => tracing::info!(?reason, "MCP session ended"),
        () = shutdown_signal() => tracing::info!("shutdown signal received"),
    }

    // The action running when the client left stops at its next event,
    // queued calls are skipped, then everything held is released: exiting
    // with a button or a modifier down would leave it down for the whole
    // desktop. Launched children are ended meanwhile on a thread of their
    // own, so neither waits for the other.
    desktop::stop_input();
    let ending = {
        let children = Arc::clone(&children);
        std::thread::Builder::new()
            .name("desktop-shutdown-kill".into())
            .spawn(move || children.kill_all())
    };
    release_input_before_exit(
        &worker,
        SHUTDOWN_WAIT,
        Duration::from_secs(1),
        desktop::Desktop::release_input_checked,
    )
    .await;
    match ending {
        Ok(thread) => {
            if thread.join().is_err() {
                tracing::warn!("ending launched children panicked");
            }
        }
        Err(e) => {
            tracing::warn!(
                "could not start ending launched children on a thread ({e}); ending them here"
            );
            children.kill_all();
        }
    }
    // The second try `Children`'s drop would make: exiting skips drops, and
    // a kill the OS refused the first time is kept for one more attempt.
    children.kill_all();
    // Everything is released and ended. Exiting here rather than returning:
    // after a signal, stdin is still open, and the runtime's teardown would
    // wait on its blocked read for good. (On Windows, a logoff or shutdown
    // ends this process without a signal; the kill-on-exit job ends its
    // children then.)
    std::process::exit(0)
}

/// Keeps the desktop worker alive until its release job has completed.
/// A warning never drops an in-flight job, and an unconfirmed result never
/// permits process exit. A disconnected worker therefore requires external
/// intervention instead of silently abandoning potentially held input.
async fn release_input_before_exit<F>(
    worker: &Worker,
    warning_after: Duration,
    retry_after: Duration,
    release_input: F,
) where
    F: Fn(&mut desktop::Desktop) -> error::ToolResult<()> + Clone + Send + 'static,
{
    loop {
        let release = worker.run_at_shutdown(release_input.clone());
        tokio::pin!(release);
        let result = if let Ok(result) = tokio::time::timeout(warning_after, &mut release).await {
            result
        } else {
            tracing::warn!(
                "desktop input cleanup is delayed; waiting for the release job before exiting"
            );
            release.await
        };
        match result {
            Ok(()) => return,
            Err(error) => {
                tracing::error!(%error, "desktop input cleanup was not confirmed; retrying before exit");
                tokio::time::sleep(retry_after).await;
            }
        }
    }
}

/// Stdin that starts shutdown at EOF.
struct ShutdownOnEof<R>(R);

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ShutdownOnEof<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let read = std::pin::Pin::new(&mut self.0).poll_read(cx, buf);
        if matches!(read, std::task::Poll::Ready(Ok(()))) && buf.filled().len() == before {
            desktop::stop_input();
        }
        read
    }
}

/// Resolves on SIGTERM or SIGHUP (Unix), on Ctrl+C, and on Windows on the
/// console closing, logoff or shutdown; never when none can be watched.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let watch = |kind| async move {
            match signal(kind) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            () = watch(SignalKind::terminate()) => {}
            () = watch(SignalKind::hangup()) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_logoff, ctrl_shutdown};
        // Each one watched if it can be; one that cannot never fires.
        macro_rules! watch {
            ($make:expr) => {
                async {
                    match $make {
                        Ok(mut s) => {
                            s.recv().await;
                        }
                        Err(_) => std::future::pending::<()>().await,
                    }
                }
            };
        }
        tokio::select! {
            () = watch!(ctrl_close()) => {}
            () = watch!(ctrl_logoff()) => {}
            () = watch!(ctrl_shutdown()) => {}
            () = watch!(ctrl_break()) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::task::{Context, Waker};

    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_waits_past_its_warning_until_the_worker_can_release_input() {
        let worker = Worker::spawn().expect("BUG: worker starts");
        let held = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::clone(&held);
        let (release, hold) = mpsc::channel();
        let (started, running) = mpsc::channel();
        let ct = CancellationToken::new();
        let mut blocker = std::pin::pin!(worker.run(&ct, move |_| {
            in_flight.store(true, Ordering::SeqCst);
            let _ = started.send(());
            let _ = hold.recv();
            in_flight.store(false, Ordering::SeqCst);
            Ok(())
        }));
        assert!(
            blocker
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        running
            .recv_timeout(Duration::from_secs(5))
            .expect("BUG: worker job starts");
        let mut shutdown = std::pin::pin!(release_input_before_exit(
            &worker,
            Duration::ZERO,
            Duration::from_millis(1),
            desktop::Desktop::release_input_checked,
        ));
        let early = tokio::time::timeout(Duration::from_millis(25), &mut shutdown).await;
        let still_held = held.load(Ordering::SeqCst);
        // Always unblock the real worker before asserting, so a regression
        // cannot leave the test's thread waiting forever.
        release.send(()).expect("BUG: worker is waiting");
        blocker.await.expect("BUG: worker job completes");
        assert!(
            early.is_err(),
            "the warning must not permit exit while the worker is occupied"
        );
        assert!(
            still_held,
            "the in-flight operation was still holding input"
        );
        tokio::time::timeout(Duration::from_secs(5), &mut shutdown)
            .await
            .expect("BUG: shutdown completes once the worker confirms release");
        assert!(!held.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_retries_unconfirmed_release_instead_of_exiting() {
        let worker = Worker::spawn().expect("BUG: worker starts");
        let can_release = Arc::new(AtomicBool::new(false));
        let held = Arc::new(AtomicBool::new(true));
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (failed, observed_failure) = mpsc::channel();
        let (allowed, holding, calls) = (
            Arc::clone(&can_release),
            Arc::clone(&held),
            Arc::clone(&attempts),
        );
        let mut shutdown = std::pin::pin!(release_input_before_exit(
            &worker,
            Duration::from_secs(5),
            Duration::from_millis(1),
            move |desktop| {
                calls.fetch_add(1, Ordering::SeqCst);
                if !allowed.load(Ordering::SeqCst) {
                    let _ = failed.send(());
                    return Err(error::ToolError::InputHeld(
                        "injected release failure".into(),
                    ));
                }
                desktop.release_input_checked()?;
                holding.store(false, Ordering::SeqCst);
                Ok(())
            },
        ));
        assert!(
            shutdown
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        observed_failure
            .recv_timeout(Duration::from_secs(5))
            .expect("BUG: first release attempt fails");
        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut shutdown)
                .await
                .is_err(),
            "an unconfirmed release must not permit exit"
        );
        assert!(held.load(Ordering::SeqCst));
        can_release.store(true, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(5), &mut shutdown)
            .await
            .expect("BUG: a later confirmed release permits exit");
        assert!(!held.load(Ordering::SeqCst));
        assert!(attempts.load(Ordering::SeqCst) >= 2);
    }
}

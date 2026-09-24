//! `flui-desktop-mcp`: an MCP server (stdio) that lets an agent drive any
//! desktop application the way a person with a screen reader does — list and
//! capture windows, read the accessibility tree, invoke element actions, and
//! send real pointer and keyboard input. See `README.md` for the tools and
//! how to register the server.
//!
//! stdout carries the MCP protocol; logs go to stderr (`RUST_LOG` filters).

mod a11y;
// Element handles; only the UIA backend issues them so far.
#[cfg(any(target_os = "windows", test))]
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

/// How long shutdown waits for the desktop thread to stop the call in
/// progress and release held input. Input stops at its next event once
/// shutdown is flagged; the wait covers a UI Automation call running out its
/// own timeout (5 s) on the way.
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

    let service = server
        .serve(rmcp::transport::stdio())
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
    let released = tokio::time::timeout(
        SHUTDOWN_WAIT,
        worker.run_at_shutdown(|d| {
            d.release_input();
            Ok(())
        }),
    )
    .await;
    if !matches!(released, Ok(Ok(()))) {
        tracing::warn!("could not release held input before exiting: {released:?}");
    }
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
    Ok(())
}

/// Resolves on SIGTERM (Unix) or Ctrl+C; never when neither can be watched.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

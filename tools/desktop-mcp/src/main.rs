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

use anyhow::Context as _;
use rmcp::ServiceExt as _;
use tracing_subscriber::EnvFilter;

use crate::process::Children;
use crate::server::DesktopServer;
use crate::worker::Worker;

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
    let server = DesktopServer::new(worker, Arc::clone(&children));

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .context("starting the MCP session")?;
    let reason = service.waiting().await;
    tracing::info!(?reason, "MCP session ended");

    children.kill_all();
    Ok(())
}

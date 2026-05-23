//! Built-in dev server for FLUI web applications.
//!
//! Mirrors `flutter run -d chrome`: builds WASM via wasm-pack, starts an HTTP
//! server with correct MIME types, and opens the browser automatically.
//!
//! # Usage
//!
//! ```bash
//! cargo run -p flui-web-server
//! cargo run -p flui-web-server -- --port 3000 --no-open
//! cargo run -p flui-web-server -- --skip-build
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use axum::Router;
use clap::Parser;
use tower_http::services::ServeDir;

/// FLUI Web Dev Server — build WASM and serve locally.
#[derive(Parser, Debug)]
#[command(
    name = "flui-web-server",
    about = "Built-in dev server for FLUI web apps"
)]
struct Args {
    /// Port to serve on.
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Skip wasm-pack build (serve existing artifacts).
    #[arg(long)]
    skip_build: bool,

    /// Don't open browser automatically.
    #[arg(long)]
    no_open: bool,

    /// Path to the web example directory (defaults to examples/web_demo).
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Build in release mode.
    #[arg(long)]
    release: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let workspace_root = find_workspace_root().expect("Must be run from within the FLUI workspace");
    let web_dir = args
        .dir
        .unwrap_or_else(|| workspace_root.join("examples").join("web_demo"));

    if !web_dir.join("Cargo.toml").exists() {
        eprintln!("Error: {} does not contain a Cargo.toml", web_dir.display());
        std::process::exit(1);
    }

    // Step 1: Build WASM (like flutter run builds Dart)
    if !args.skip_build {
        build_wasm(&web_dir, args.release);
    }

    // Step 2: Serve (like flutter's built-in dev server)
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let url = format!("http://{addr}");

    let app = Router::new().fallback_service(ServeDir::new(&web_dir));

    println!();
    println!("  \x1b[32m✓\x1b[0m FLUI Web Dev Server");
    println!("  \x1b[36m➜\x1b[0m Local: {url}");
    println!("  \x1b[90mPress Ctrl+C to stop\x1b[0m");
    println!();

    // Step 3: Open browser (like flutter opens Chrome)
    if !args.no_open
        && let Err(e) = open::that(&url)
    {
        tracing::warn!("Could not open browser: {e}");
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: Could not bind to {addr}: {e}");
            std::process::exit(1);
        });

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });

    println!("\n  \x1b[33m⏹\x1b[0m Server stopped");
}

/// Build WASM via wasm-pack.
fn build_wasm(web_dir: &Path, release: bool) {
    println!("  \x1b[36m⚙\x1b[0m Building WASM...");

    // Check wasm-pack is installed
    if Command::new("wasm-pack").arg("--version").output().is_err() {
        eprintln!("Error: wasm-pack not found. Install with: cargo install wasm-pack");
        std::process::exit(1);
    }

    let mut cmd = Command::new("wasm-pack");
    cmd.args(["build", "--target", "web", "--out-dir", "pkg"]);

    if release {
        cmd.arg("--release");
    } else {
        cmd.arg("--dev");
    }

    cmd.current_dir(web_dir);

    let status = cmd.status().unwrap_or_else(|e| {
        eprintln!("Error: Failed to run wasm-pack: {e}");
        std::process::exit(1);
    });

    if !status.success() {
        eprintln!("Error: wasm-pack build failed");
        std::process::exit(1);
    }

    println!("  \x1b[32m✓\x1b[0m WASM build complete");
}

/// Find the workspace root by walking up to find the root Cargo.toml with [workspace].
fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists()
            && let Ok(content) = std::fs::read_to_string(&cargo_toml)
            && content.contains("[workspace]")
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Wait for Ctrl+C signal for graceful shutdown.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}

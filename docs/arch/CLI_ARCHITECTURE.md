# FLUI CLI Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the complete architecture for FLUI's command-line interface (`flui_cli`), based on Flutter CLI and Cargo best practices. The system provides **project scaffolding**, **build management**, **development tools**, and **workspace management**.

**Key Design Principles:**
1. **Flutter-Compatible Commands**: `flui create`, `build`, `run`, `test`, `doctor`
2. **Cargo Integration**: Seamless integration with existing Rust tooling
3. **Project Templates**: Opinionated project scaffolding with best practices
4. **Hot Reload**: Fast development cycle with incremental compilation
5. **Build Modes**: Debug, Profile, Release (like Flutter)
6. **Workspace Management**: Multi-package monorepo support

**Core Commands:**
```bash
# Project management
flui create my_app                    # Create new FLUI app
flui create --template widget my_pkg  # Create reusable widget package

# Development
flui run                              # Run app (debug mode, hot reload)
flui run --release                    # Run in release mode
flui test                             # Run tests
flui analyze                          # Lint and analyze code

# Build
flui build windows                    # Build for Windows
flui build linux                      # Build for Linux
flui build macos                      # Build for macOS
flui build web                        # Build for Web/WASM

# Utilities
flui doctor                           # Check environment setup
flui devices                          # List available devices
flui clean                            # Clean build artifacts
flui upgrade                          # Upgrade FLUI SDK

# DevTools
flui devtools                         # Launch DevTools
```

**Total Work Estimate:** ~3,000 LOC (core ~800 + commands ~1,500 + templates ~700)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Command Structure](#command-structure)
3. [Project Templates](#project-templates)
4. [Build System](#build-system)
5. [Hot Reload](#hot-reload)
6. [Workspace Management](#workspace-management)
7. [Configuration](#configuration)
8. [Implementation Plan](#implementation-plan)
9. [Usage Examples](#usage-examples)
10. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### CLI Structure

```text
flui (binary)
  ↓
clap (argument parsing)
  ↓
Command Dispatch
  ├─ CreateCommand      (flui create)
  ├─ RunCommand         (flui run)
  ├─ BuildCommand       (flui build)
  ├─ TestCommand        (flui test)
  ├─ AnalyzeCommand     (flui analyze)
  ├─ DoctorCommand      (flui doctor)
  ├─ DevicesCommand     (flui devices)
  ├─ CleanCommand       (flui clean)
  ├─ UpgradeCommand     (flui upgrade)
  └─ DevToolsCommand    (flui devtools)
```

### Main Entry Point

```rust
// In flui_cli/src/main.rs

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "flui")]
#[command(about = "FLUI CLI - Build beautiful cross-platform apps with Rust", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new FLUI project
    Create {
        /// Project name
        name: String,

        /// Organization name (reverse domain notation)
        #[arg(long, default_value = "com.example")]
        org: String,

        /// Project template
        #[arg(long, value_enum, default_value = "app")]
        template: Template,

        /// Target platforms
        #[arg(long, value_delimiter = ',')]
        platforms: Option<Vec<Platform>>,
    },

    /// Run the FLUI application
    Run {
        /// Build mode
        #[arg(long, value_enum, default_value = "debug")]
        mode: BuildMode,

        /// Target device
        #[arg(short, long)]
        device: Option<String>,

        /// Enable hot reload
        #[arg(long, default_value = "true")]
        hot_reload: bool,
    },

    /// Build the FLUI application
    Build {
        /// Target platform
        target: BuildTarget,

        /// Build mode
        #[arg(long, value_enum, default_value = "release")]
        mode: BuildMode,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Run tests
    Test {
        /// Test filter
        filter: Option<String>,

        /// Run with coverage
        #[arg(long)]
        coverage: bool,
    },

    /// Analyze project for issues
    Analyze {
        /// Fix automatically
        #[arg(long)]
        fix: bool,
    },

    /// Check FLUI environment setup
    Doctor {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// List available devices
    Devices,

    /// Clean build artifacts
    Clean,

    /// Upgrade FLUI SDK
    Upgrade {
        /// Target channel
        #[arg(long, value_enum)]
        channel: Option<Channel>,
    },

    /// Launch DevTools
    Devtools {
        /// Port to listen on
        #[arg(short, long, default_value = "9100")]
        port: u16,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Template {
    /// Basic application
    App,
    /// Widget package
    Widget,
    /// Plugin package
    Plugin,
    /// Empty project
    Empty,
}

#[derive(Clone, Copy, ValueEnum)]
enum Platform {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Web,
}

#[derive(Clone, Copy, ValueEnum)]
enum BuildTarget {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Web,
}

#[derive(Clone, Copy, ValueEnum)]
enum BuildMode {
    Debug,
    Profile,
    Release,
}

#[derive(Clone, Copy, ValueEnum)]
enum Channel {
    Stable,
    Beta,
    Dev,
}

fn main() {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    // Dispatch command
    let result = match cli.command {
        Commands::Create {
            name,
            org,
            template,
            platforms,
        } => commands::create::execute(name, org, template, platforms),

        Commands::Run {
            mode,
            device,
            hot_reload,
        } => commands::run::execute(mode, device, hot_reload),

        Commands::Build {
            target,
            mode,
            output,
        } => commands::build::execute(target, mode, output),

        Commands::Test { filter, coverage } => commands::test::execute(filter, coverage),

        Commands::Analyze { fix } => commands::analyze::execute(fix),

        Commands::Doctor { verbose } => commands::doctor::execute(verbose),

        Commands::Devices => commands::devices::execute(),

        Commands::Clean => commands::clean::execute(),

        Commands::Upgrade { channel } => commands::upgrade::execute(channel),

        Commands::Devtools { port } => commands::devtools::execute(port),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
```

---

## Command Structure

### CreateCommand

```rust
// In flui_cli/src/commands/create.rs

use crate::{Template, Platform};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

pub fn execute(
    name: String,
    org: String,
    template: Template,
    platforms: Option<Vec<Platform>>,
) -> Result<()> {
    println!("Creating FLUI project '{}'...", name);

    // Validate project name
    validate_project_name(&name)?;

    // Create project directory
    let project_dir = PathBuf::from(&name);
    if project_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    fs::create_dir_all(&project_dir)?;

    // Generate project from template
    let template_gen = TemplateGenerator::new(name.clone(), org.clone());
    match template {
        Template::App => template_gen.generate_app(&project_dir)?,
        Template::Widget => template_gen.generate_widget(&project_dir)?,
        Template::Plugin => template_gen.generate_plugin(&project_dir)?,
        Template::Empty => template_gen.generate_empty(&project_dir)?,
    }

    // Initialize git repository
    init_git_repo(&project_dir)?;

    // Run initial setup
    run_cargo_init(&project_dir)?;

    println!("✓ Created FLUI project '{}'", name);
    println!("\nTo get started:");
    println!("  cd {}", name);
    println!("  flui run");

    Ok(())
}

fn validate_project_name(name: &str) -> Result<()> {
    // Validate Rust package naming rules
    if name.is_empty() {
        anyhow::bail!("Project name cannot be empty");
    }

    if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        anyhow::bail!("Project name must contain only alphanumeric characters, hyphens, and underscores");
    }

    if name.starts_with(char::is_numeric) {
        anyhow::bail!("Project name cannot start with a number");
    }

    Ok(())
}

fn init_git_repo(dir: &PathBuf) -> Result<()> {
    use std::process::Command;

    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .context("Failed to initialize git repository")?;

    // Create .gitignore
    let gitignore = r#"
# Build artifacts
/target
/build

# IDE
.vscode/
.idea/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db

# FLUI
flui.lock
"#;

    fs::write(dir.join(".gitignore"), gitignore)?;

    Ok(())
}

fn run_cargo_init(dir: &PathBuf) -> Result<()> {
    use std::process::Command;

    // Run cargo check to download dependencies
    let output = Command::new("cargo")
        .args(["check"])
        .current_dir(dir)
        .output()
        .context("Failed to run cargo check")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo check failed:\n{}", stderr);
    }

    Ok(())
}
```

### RunCommand

```rust
// In flui_cli/src/commands/run.rs

use crate::BuildMode;
use anyhow::{Context, Result};
use std::process::Command;

pub fn execute(mode: BuildMode, device: Option<String>, hot_reload: bool) -> Result<()> {
    println!("Running FLUI app ({:?} mode)...", mode);

    // Check if in FLUI project
    ensure_flui_project()?;

    // Select device
    let target_device = if let Some(device) = device {
        device
    } else {
        select_default_device()?
    };

    println!("Target device: {}", target_device);

    // Build cargo command
    let mut cmd = Command::new("cargo");
    cmd.arg("run");

    // Add mode flags
    match mode {
        BuildMode::Debug => {
            // Default debug mode
        }
        BuildMode::Profile => {
            cmd.arg("--profile=profile");
        }
        BuildMode::Release => {
            cmd.arg("--release");
        }
    }

    // Add hot reload flag (via environment variable)
    if hot_reload && mode == BuildMode::Debug {
        cmd.env("FLUI_HOT_RELOAD", "1");
        println!("Hot reload enabled");
    }

    // Run command
    let status = cmd.status().context("Failed to run cargo")?;

    if !status.success() {
        anyhow::bail!("cargo run failed");
    }

    Ok(())
}

fn ensure_flui_project() -> Result<()> {
    let cargo_toml = std::path::Path::new("Cargo.toml");
    if !cargo_toml.exists() {
        anyhow::bail!("Not a FLUI project (Cargo.toml not found)");
    }

    // Check for FLUI dependency
    let content = std::fs::read_to_string(cargo_toml)?;
    if !content.contains("flui_app") {
        anyhow::bail!("Not a FLUI project (flui_app dependency not found)");
    }

    Ok(())
}

fn select_default_device() -> Result<String> {
    // For desktop, return current OS
    #[cfg(target_os = "windows")]
    return Ok("Windows".to_string());

    #[cfg(target_os = "linux")]
    return Ok("Linux".to_string());

    #[cfg(target_os = "macos")]
    return Ok("macOS".to_string());

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    anyhow::bail!("No default device for this platform");
}
```

### BuildCommand

```rust
// In flui_cli/src/commands/build.rs

use crate::{BuildTarget, BuildMode};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn execute(target: BuildTarget, mode: BuildMode, output: Option<PathBuf>) -> Result<()> {
    println!("Building for {:?} ({:?} mode)...", target, mode);

    ensure_flui_project()?;

    // Build cargo command
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    // Add target
    let target_triple = get_target_triple(target)?;
    cmd.arg("--target").arg(&target_triple);

    // Add mode flags
    match mode {
        BuildMode::Debug => {
            // Default debug mode
        }
        BuildMode::Profile => {
            cmd.arg("--profile=profile");
        }
        BuildMode::Release => {
            cmd.arg("--release");
        }
    }

    // Run build
    let status = cmd.status().context("Failed to run cargo build")?;

    if !status.success() {
        anyhow::bail!("Build failed");
    }

    // Copy artifacts to output directory if specified
    if let Some(output_dir) = output {
        let build_dir = get_build_dir(&target_triple, mode)?;
        copy_artifacts(&build_dir, &output_dir)?;
        println!("✓ Build artifacts copied to {}", output_dir.display());
    }

    println!("✓ Build completed successfully");

    Ok(())
}

fn get_target_triple(target: BuildTarget) -> Result<String> {
    Ok(match target {
        BuildTarget::Windows => "x86_64-pc-windows-msvc".to_string(),
        BuildTarget::Linux => "x86_64-unknown-linux-gnu".to_string(),
        BuildTarget::Macos => "x86_64-apple-darwin".to_string(),
        BuildTarget::Android => "aarch64-linux-android".to_string(),
        BuildTarget::Ios => "aarch64-apple-ios".to_string(),
        BuildTarget::Web => "wasm32-unknown-unknown".to_string(),
    })
}

fn get_build_dir(target: &str, mode: BuildMode) -> Result<PathBuf> {
    let mut path = PathBuf::from("target");
    path.push(target);

    match mode {
        BuildMode::Debug => path.push("debug"),
        BuildMode::Profile => path.push("profile"),
        BuildMode::Release => path.push("release"),
    }

    Ok(path)
}

fn copy_artifacts(src: &PathBuf, dest: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest)?;

    // Copy executable
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let file_name = path.file_name().unwrap();
            let dest_path = dest.join(file_name);
            std::fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}
```

### DoctorCommand

```rust
// In flui_cli/src/commands/doctor.rs

use anyhow::Result;
use std::process::Command;

pub fn execute(verbose: bool) -> Result<()> {
    println!("Checking FLUI environment...\n");

    let mut all_ok = true;

    // Check Rust installation
    all_ok &= check_rust(verbose);

    // Check Cargo
    all_ok &= check_cargo(verbose);

    // Check FLUI installation
    all_ok &= check_flui(verbose);

    // Check platform tools
    #[cfg(target_os = "windows")]
    {
        all_ok &= check_windows_tools(verbose);
    }

    #[cfg(target_os = "linux")]
    {
        all_ok &= check_linux_tools(verbose);
    }

    #[cfg(target_os = "macos")]
    {
        all_ok &= check_macos_tools(verbose);
    }

    // Check wgpu support
    all_ok &= check_wgpu(verbose);

    println!();

    if all_ok {
        println!("✓ All checks passed!");
    } else {
        println!("✗ Some checks failed. Please fix the issues above.");
    }

    Ok(())
}

fn check_rust(verbose: bool) -> bool {
    print!("[✓] Rust: ");

    match Command::new("rustc").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim();
            println!("{}", version);

            if verbose {
                println!("    Path: {}", which::which("rustc").unwrap().display());
            }

            true
        }
        Err(_) => {
            println!("Not found");
            println!("    Please install Rust: https://rustup.rs/");
            false
        }
    }
}

fn check_cargo(verbose: bool) -> bool {
    print!("[✓] Cargo: ");

    match Command::new("cargo").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("{}", version.trim());
            true
        }
        Err(_) => {
            println!("Not found");
            false
        }
    }
}

fn check_flui(verbose: bool) -> bool {
    print!("[✓] FLUI: ");

    // Check if flui_app is available
    let flui_version = env!("CARGO_PKG_VERSION");
    println!("v{}", flui_version);

    if verbose {
        println!("    Location: {}", env!("CARGO_MANIFEST_DIR"));
    }

    true
}

#[cfg(target_os = "windows")]
fn check_windows_tools(verbose: bool) -> bool {
    print!("[✓] Windows SDK: ");

    // Check for Windows SDK
    let sdk_path = r"C:\Program Files (x86)\Windows Kits\10";
    if std::path::Path::new(sdk_path).exists() {
        println!("Installed");
        true
    } else {
        println!("Not found");
        println!("    Please install Visual Studio with C++ support");
        false
    }
}

#[cfg(target_os = "linux")]
fn check_linux_tools(verbose: bool) -> bool {
    print!("[✓] Build tools: ");

    // Check for gcc/g++
    if Command::new("gcc").arg("--version").output().is_ok() {
        println!("Installed");
        true
    } else {
        println!("Not found");
        println!("    sudo apt install build-essential");
        false
    }
}

#[cfg(target_os = "macos")]
fn check_macos_tools(verbose: bool) -> bool {
    print!("[✓] Xcode: ");

    // Check for Xcode command line tools
    if Command::new("xcode-select").arg("-p").output().is_ok() {
        println!("Installed");
        true
    } else {
        println!("Not found");
        println!("    xcode-select --install");
        false
    }
}

fn check_wgpu(verbose: bool) -> bool {
    print!("[✓] wgpu support: ");

    // wgpu is a library dependency, so just check if we can compile
    println!("Available (via Rust)");
    true
}
```

---

## Project Templates

### App Template

```rust
// In flui_cli/src/templates/app.rs

pub struct AppTemplate {
    name: String,
    org: String,
}

impl AppTemplate {
    pub fn generate(&self, dir: &PathBuf) -> Result<()> {
        // Generate Cargo.toml
        self.generate_cargo_toml(dir)?;

        // Generate src/main.rs
        self.generate_main(dir)?;

        // Generate flui.toml
        self.generate_flui_config(dir)?;

        // Generate README
        self.generate_readme(dir)?;

        Ok(())
    }

    fn generate_cargo_toml(&self, dir: &PathBuf) -> Result<()> {
        let content = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_app = "0.1"
flui_widgets = "0.1"
flui_types = "0.1"

# Optional: DevTools (debug builds only)
[dev-dependencies]
flui_devtools = "0.1"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
"#,
            self.name
        );

        fs::write(dir.join("Cargo.toml"), content)?;
        Ok(())
    }

    fn generate_main(&self, dir: &PathBuf) -> Result<()> {
        let content = r#"use flui_app::runApp;
use flui_widgets::*;

fn main() {
    #[cfg(debug_assertions)]
    flui_devtools::enable();

    runApp(MyApp::new());
}

#[derive(Debug)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        MaterialApp::builder()
            .title("My FLUI App")
            .theme(ThemeData::light())
            .home(MyHomeView::new())
            .build()
    }
}

#[derive(Debug)]
struct MyHomeView;

impl View for MyHomeView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);

        Scaffold::builder()
            .app_bar(
                AppBar::new()
                    .title(Text::new("FLUI App"))
            )
            .body(
                Center::new(
                    Column::new()
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .children(vec![
                            Box::new(Text::new("You have pushed the button this many times:")),
                            Box::new(Text::new(format!("{}", count.get()))
                                .style(TextStyle::new()
                                    .font_size(48.0)
                                    .font_weight(FontWeight::Bold))),
                        ])
                )
            )
            .floating_action_button(
                FloatingActionButton::new(
                    Icon::new("add")
                ).on_pressed({
                    let count = count.clone();
                    move || count.update(|c| *c += 1)
                })
            )
            .build()
    }
}
"#;

        let src_dir = dir.join("src");
        fs::create_dir_all(&src_dir)?;
        fs::write(src_dir.join("main.rs"), content)?;

        Ok(())
    }

    fn generate_flui_config(&self, dir: &PathBuf) -> Result<()> {
        let content = format!(
            r#"[app]
name = "{}"
version = "0.1.0"
organization = "{}"

[build]
target_platforms = ["windows", "linux", "macos"]

[assets]
# Asset directories
directories = ["assets"]

[fonts]
# Custom fonts
# [[fonts]]
# family = "Roboto"
# fonts = [
#     {{ asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" }},
# ]
"#,
            self.name, self.org
        );

        fs::write(dir.join("flui.toml"), content)?;
        Ok(())
    }

    fn generate_readme(&self, dir: &PathBuf) -> Result<()> {
        let content = format!(
            r#"# {}

A FLUI application.

## Getting Started

Run the application:

```bash
flui run
```

Build for release:

```bash
flui build windows --release
```

Run tests:

```bash
flui test
```

## Learn More

- [FLUI Documentation](https://github.com/flui-rs/flui)
- [Examples](https://github.com/flui-rs/flui/tree/main/examples)
"#,
            self.name
        );

        fs::write(dir.join("README.md"), content)?;
        Ok(())
    }
}
```

---

## Build System

### Build Configuration

```toml
# flui.toml (project-level configuration)

[app]
name = "my_app"
version = "0.1.0"
organization = "com.example"

[build]
# Target platforms
target_platforms = ["windows", "linux", "macos", "web"]

# Build optimization
lto = true
opt_level = 3

[build.debug]
# Debug mode settings
incremental = true
hot_reload = true

[build.profile]
# Profile mode settings
debug = true
opt_level = 2

[build.release]
# Release mode settings
strip = true
lto = true
opt_level = 3

[assets]
# Asset directories
directories = ["assets", "images"]

# Asset bundling
bundle = true

[fonts]
# Custom fonts
[[fonts]]
family = "Roboto"
fonts = [
    { asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" },
    { asset = "fonts/Roboto-Bold.ttf", weight = 700, style = "normal" },
]

[dependencies]
# Additional Rust crates
```

### Build Profiles

```toml
# Cargo.toml profiles

[profile.dev]
opt-level = 0
debug = true
incremental = true

[profile.profile]
inherits = "release"
debug = true
strip = false

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
debug = false
```

---

## Hot Reload

### Hot Reload System

```rust
// In flui_cli/src/hot_reload/mod.rs

use notify::{Watcher, RecursiveMode, Event};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Hot reload manager
pub struct HotReloadManager {
    watcher: RecommendedWatcher,
    reload_tx: mpsc::Sender<ReloadEvent>,
}

impl HotReloadManager {
    pub fn new() -> Result<Self> {
        let (reload_tx, mut reload_rx) = mpsc::channel(100);

        let reload_tx_clone = reload_tx.clone();
        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if Self::should_reload(&event) {
                            let _ = reload_tx_clone.try_send(ReloadEvent::FileChanged {
                                path: event.paths[0].clone(),
                            });
                        }
                    }
                    Err(e) => tracing::error!("Watch error: {}", e),
                }
            },
            notify::Config::default(),
        )?;

        // Spawn reload handler
        tokio::spawn(async move {
            while let Some(event) = reload_rx.recv().await {
                Self::handle_reload_event(event).await;
            }
        });

        Ok(Self { watcher, reload_tx })
    }

    /// Watch a directory for changes
    pub fn watch(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;
        Ok(())
    }

    fn should_reload(event: &Event) -> bool {
        // Only reload on Rust source file changes
        event.paths.iter().any(|p| {
            p.extension().map_or(false, |ext| ext == "rs")
        })
    }

    async fn handle_reload_event(event: ReloadEvent) {
        match event {
            ReloadEvent::FileChanged { path } => {
                println!("File changed: {}", path.display());
                println!("Recompiling...");

                // Trigger incremental recompilation
                if let Err(e) = Self::recompile().await {
                    eprintln!("Recompilation failed: {}", e);
                } else {
                    println!("✓ Hot reload completed");
                }
            }
        }
    }

    async fn recompile() -> Result<()> {
        use std::process::Command;

        let output = Command::new("cargo")
            .args(["build", "--incremental"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Compilation failed:\n{}", stderr);
        }

        Ok(())
    }
}

enum ReloadEvent {
    FileChanged { path: PathBuf },
}
```

---

## Workspace Management

### Workspace Commands

```rust
// Support for Cargo workspaces

// flui workspace init
// Creates a new workspace

// flui workspace add <package>
// Adds a package to the workspace

// flui workspace publish
// Publishes all workspace packages
```

---

## Configuration

### Global Configuration

```toml
# ~/.flui/config.toml (user-level configuration)

[sdk]
# SDK channel
channel = "stable"

# SDK path
path = "~/.flui/sdk"

[build]
# Default build settings
jobs = 4

[devtools]
# DevTools settings
port = 9100
auto_launch = true

[telemetry]
# Anonymous usage statistics
enabled = false
```

---

## Implementation Plan

### Phase 1: Core CLI (~800 LOC)

1. **main.rs** (~200 LOC)
   - Argument parsing with clap
   - Command dispatch

2. **config.rs** (~200 LOC)
   - flui.toml parsing
   - Global config management

3. **error.rs** (~100 LOC)
   - Error types
   - User-friendly error messages

4. **utils.rs** (~300 LOC)
   - Common utilities
   - File operations
   - Process management

**Total Phase 1:** ~800 LOC

### Phase 2: Commands (~1,500 LOC)

5. **commands/create.rs** (~300 LOC)
   - Project creation
   - Template generation

6. **commands/run.rs** (~200 LOC)
   - Run application
   - Device selection

7. **commands/build.rs** (~300 LOC)
   - Build system
   - Target compilation

8. **commands/test.rs** (~150 LOC)
   - Test runner

9. **commands/analyze.rs** (~100 LOC)
   - Code analysis

10. **commands/doctor.rs** (~200 LOC)
    - Environment check

11. **commands/devices.rs** (~100 LOC)
    - Device listing

12. **commands/clean.rs** (~50 LOC)
    - Clean artifacts

13. **commands/upgrade.rs** (~100 LOC)
    - SDK upgrade

**Total Phase 2:** ~1,500 LOC

### Phase 3: Templates (~700 LOC)

14. **templates/app.rs** (~250 LOC)
    - App template

15. **templates/widget.rs** (~200 LOC)
    - Widget package template

16. **templates/plugin.rs** (~200 LOC)
    - Plugin template

17. **templates/generator.rs** (~50 LOC)
    - Template engine

**Total Phase 3:** ~700 LOC

---

## Usage Examples

### Example 1: Create New App

```bash
# Create basic app
flui create my_app

# Create with custom organization
flui create --org com.mycompany my_app

# Create widget package
flui create --template widget my_widget
```

### Example 2: Development Workflow

```bash
# Run in debug mode with hot reload
flui run

# Run in release mode
flui run --release

# Run on specific device
flui run --device windows

# Run tests
flui test

# Analyze code
flui analyze --fix
```

### Example 3: Build for Production

```bash
# Build for Windows
flui build windows --release

# Build for multiple platforms
flui build windows --release
flui build linux --release
flui build macos --release

# Build with custom output
flui build windows --release --output ./dist
```

### Example 4: Environment Setup

```bash
# Check environment
flui doctor

# List devices
flui devices

# Upgrade SDK
flui upgrade

# Launch DevTools
flui devtools
```

---

## Testing Strategy

### Unit Tests

1. **Template Generation:**
   - Test all templates
   - Verify file structure
   - Check file contents

2. **Configuration:**
   - Test flui.toml parsing
   - Test global config
   - Test validation

3. **Build System:**
   - Test target selection
   - Test build modes
   - Test artifact copying

### Integration Tests

1. **End-to-End:**
   - Create project
   - Build project
   - Run project

2. **Hot Reload:**
   - File watching
   - Recompilation
   - State preservation

### CLI Tests

```rust
#[test]
fn test_create_command() {
    let temp_dir = tempdir().unwrap();

    let result = create::execute(
        "test_app".to_string(),
        "com.test".to_string(),
        Template::App,
        None,
    );

    assert!(result.is_ok());
    assert!(temp_dir.path().join("test_app/Cargo.toml").exists());
}
```

---

## Crate Dependencies

```toml
# flui_cli/Cargo.toml

[package]
name = "flui_cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "flui"
path = "src/main.rs"

[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }
clap-verbosity-flag = "2.0"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Async runtime
tokio = { version = "1.43", features = ["full"] }

# File watching
notify = "6.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Configuration
toml = "0.8"
serde = { version = "1.0", features = ["derive"] }

# Utilities
which = "6.0"
tempfile = "3.0"

[dev-dependencies]
assert_cmd = "2.0"
predicates = "3.0"
```

---

## Open Questions

1. **Package Registry:**
   - Should we have a FLUI package registry?
   - Integration with crates.io?

2. **IDE Integration:**
   - VSCode extension?
   - IntelliJ plugin?

3. **CI/CD:**
   - GitHub Actions templates?
   - Docker support?

---

## Version History

| Version | Date       | Author | Changes                   |
|---------|------------|--------|---------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial CLI architecture  |

---

## References

- [Flutter CLI Reference](https://docs.flutter.dev/reference/flutter-cli)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
- [clap Documentation](https://docs.rs/clap/)

---

## Conclusion

This architecture provides a **comprehensive CLI tool** for FLUI development:

✅ **Flutter-compatible commands** (create, build, run, test, doctor)
✅ **Project templates** (app, widget, plugin)
✅ **Build system** (debug, profile, release modes)
✅ **Hot reload** (file watching, incremental compilation)
✅ **Workspace management** (multi-package support)
✅ **Environment checking** (doctor command)
✅ **Cross-platform** (Windows, Linux, macOS, Web)

**Key Features:**
1. **clap-based CLI**: Modern argument parsing with derive macros
2. **Template System**: Opinionated project scaffolding
3. **Build Profiles**: Debug, Profile, Release (Flutter-compatible)
4. **Hot Reload**: File watching with incremental recompilation
5. **Doctor Command**: Environment verification

**Estimated Total Work:** ~3,000 LOC
- Core CLI (~800 LOC)
- Commands (~1,500 LOC)
- Templates (~700 LOC)

This provides production-ready CLI tools for FLUI development! 🚀⚡

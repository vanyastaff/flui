//! Command-line interface for FLUI.
//!
//! This crate provides the `flui` CLI tool for creating, building, and managing
//! FLUI projects across multiple platforms (Windows, Linux, macOS, Android, iOS, Web).
//!
//! # Architecture
//!
//! The CLI is organized into several modules:
//! - [`commands`] - Individual command implementations
//! - [`config`] - Configuration file handling (flui.toml)
//! - [`error`] - Error types and result aliases
//! - [`templates`] - Project template generation
//! - [`types`] - Type-safe wrappers (newtypes) for validated values
//!
//! # Examples
//!
//! Create a new project:
//! ```bash
//! flui create my-app --template counter
//! ```
//!
//! Build for Android:
//! ```bash
//! flui build android --release
//! ```

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

/// Custom styles for CLI help output.
///
/// Uses cyan for headers and literals to match cliclack styling.
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::Green.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));

mod commands;
mod config;
pub mod error;
pub mod runner;
mod templates;
pub mod types;
mod utils;

/// Prelude module re-exporting commonly used types.
///
/// Import with `use crate::prelude::*;` for convenient access.
pub mod prelude {
    pub use crate::error::{CliError, CliResult, OptionExt, ResultExt};
    pub use crate::runner::{CargoCommand, CommandResult, GitCommand, OutputStyle};
    pub use crate::types::{OrganizationId, ProjectName, ProjectPath};
}

/// Command-line interface for FLUI - A declarative UI framework for Rust.
#[derive(Debug, Parser)]
#[command(name = "flui")]
#[command(about = "FLUI CLI - Build beautiful cross-platform apps with Rust", long_about = None)]
#[command(version)]
#[command(styles = STYLES)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a new FLUI project
    Create {
        /// Project name (if not provided, interactive mode will be used)
        name: Option<String>,

        /// Organization name (reverse domain notation)
        #[arg(long, default_value = "com.example")]
        org: String,

        /// Project template
        #[arg(long, value_enum, default_value = "counter")]
        template: Template,

        /// Target platforms (comma-separated)
        #[arg(long, value_delimiter = ',')]
        platforms: Option<Vec<Platform>>,

        /// Custom output directory
        #[arg(long)]
        path: Option<PathBuf>,

        /// Use local path dependencies instead of crates.io versions
        #[arg(long)]
        local: bool,

        /// Create a library instead of an application
        #[arg(long)]
        lib: bool,

        /// Interactive mode (prompt for all options)
        #[arg(short, long)]
        interactive: bool,
    },

    /// Run the FLUI application
    Run {
        /// Target device
        #[arg(short, long)]
        device: Option<String>,

        /// Build in release mode
        #[arg(short, long)]
        release: bool,

        /// Enable hot reload (development mode)
        #[arg(long, default_value = "true")]
        hot_reload: bool,

        /// Build profile (dev, release, bench)
        #[arg(long)]
        profile: Option<String>,

        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },

    /// Build the FLUI application
    Build {
        /// Target platform
        platform: BuildTarget,

        /// Build in release mode (optimized)
        #[arg(short, long)]
        release: bool,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Android: Create separate APKs per ABI
        #[arg(long)]
        split_per_abi: bool,

        /// Web: Optimize WASM size
        #[arg(long)]
        optimize_wasm: bool,

        /// iOS: Build universal binary (arm64 + simulator)
        #[arg(long)]
        universal: bool,
    },

    /// Run tests
    Test {
        /// Test filter
        filter: Option<String>,

        /// Run unit tests only
        #[arg(long)]
        unit: bool,

        /// Run integration tests only
        #[arg(long)]
        integration: bool,

        /// Test on specific platform
        #[arg(long)]
        platform: Option<String>,
    },

    /// Analyze project for issues
    Analyze {
        /// Automatically fix issues
        #[arg(long)]
        fix: bool,

        /// Enable pedantic lints
        #[arg(long)]
        pedantic: bool,
    },

    /// Check FLUI environment setup
    Doctor {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,

        /// Check only Android toolchain
        #[arg(long)]
        android: bool,

        /// Check only iOS toolchain
        #[arg(long)]
        ios: bool,

        /// Check only Web toolchain
        #[arg(long)]
        web: bool,
    },

    /// List available devices
    Devices {
        /// Show detailed device information
        #[arg(long)]
        details: bool,

        /// Filter by platform
        #[arg(long)]
        platform: Option<String>,
    },

    /// Manage emulators and simulators
    Emulators {
        #[command(subcommand)]
        subcommand: EmulatorSubcommand,
    },

    /// Clean build artifacts
    Clean {
        /// Deep clean (including cargo caches)
        #[arg(long)]
        deep: bool,

        /// Clean specific platform only
        #[arg(long)]
        platform: Option<String>,
    },

    /// Update `flui_cli` and project dependencies
    Upgrade {
        /// Update `flui_cli` only
        #[arg(long)]
        self_update: bool,

        /// Update project dependencies only
        #[arg(long)]
        dependencies: bool,
    },

    /// Manage platform support for your project
    Platform {
        #[command(subcommand)]
        subcommand: PlatformSubcommand,
    },

    /// Format source code
    Format {
        /// Check formatting without modifying files
        #[arg(long)]
        check: bool,
    },

    /// Launch `DevTools`
    Devtools {
        /// Port to listen on
        #[arg(short, long, default_value = "9100")]
        port: u16,
    },

    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, powershell, elvish)
        #[arg(value_enum)]
        shell: Option<Shell>,
    },
}

#[derive(Debug, Subcommand)]
enum PlatformSubcommand {
    /// Add platform support
    Add {
        /// Platform to add
        platforms: Vec<String>,
    },

    /// Remove platform support
    Remove {
        /// Platform to remove
        platform: String,
    },

    /// List supported platforms
    List,
}

#[derive(Debug, Subcommand)]
enum EmulatorSubcommand {
    /// List available emulators and simulators
    List {
        /// Filter by platform (android or ios)
        #[arg(long)]
        platform: Option<String>,
    },

    /// Launch a specific emulator or simulator
    Launch {
        /// Emulator name to launch
        name: String,
    },
}

/// Available project templates.
///
/// Templates provide starting points for different types of FLUI applications.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash, Default)]
pub enum Template {
    /// Basic application with minimal setup
    Basic,
    /// Counter app demonstrating state management (default)
    #[default]
    Counter,
    /// Todo list app with CRUD operations
    Todo,
    /// Dashboard UI with multiple widgets
    Dashboard,
    /// Reusable widget package
    Widget,
    /// Plugin package for extending FLUI
    Plugin,
    /// Empty project with just the essentials
    Empty,
}

impl Display for Template {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Basic => write!(f, "basic"),
            Self::Counter => write!(f, "counter"),
            Self::Todo => write!(f, "todo"),
            Self::Dashboard => write!(f, "dashboard"),
            Self::Widget => write!(f, "widget"),
            Self::Plugin => write!(f, "plugin"),
            Self::Empty => write!(f, "empty"),
        }
    }
}

impl Template {
    /// Get a human-readable description of the template.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Basic => "Basic application with minimal setup",
            Self::Counter => "Counter app demonstrating state management",
            Self::Todo => "Todo list app with CRUD operations",
            Self::Dashboard => "Dashboard UI with multiple widgets",
            Self::Widget => "Reusable widget package",
            Self::Plugin => "Plugin package for extending FLUI",
            Self::Empty => "Empty project with just the essentials",
        }
    }
}

/// Target platforms for FLUI applications.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Microsoft Windows
    Windows,
    /// Linux distributions
    Linux,
    /// Apple macOS
    Macos,
    /// Google Android
    Android,
    /// Apple iOS
    Ios,
    /// Web browser (WASM)
    Web,
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windows => write!(f, "windows"),
            Self::Linux => write!(f, "linux"),
            Self::Macos => write!(f, "macos"),
            Self::Android => write!(f, "android"),
            Self::Ios => write!(f, "ios"),
            Self::Web => write!(f, "web"),
        }
    }
}

impl Platform {
    /// Check if this platform requires a specific host OS.
    #[must_use]
    pub const fn requires_host_os(&self) -> Option<&'static str> {
        match self {
            Self::Ios | Self::Macos => Some("macOS"),
            _ => None,
        }
    }

    /// Get the Rust target triple for this platform.
    #[must_use]
    pub const fn target_triple(&self) -> &'static str {
        match self {
            Self::Windows => "x86_64-pc-windows-msvc",
            Self::Linux => "x86_64-unknown-linux-gnu",
            Self::Macos => "x86_64-apple-darwin",
            Self::Android => "aarch64-linux-android",
            Self::Ios => "aarch64-apple-ios",
            Self::Web => "wasm32-unknown-unknown",
        }
    }
}

/// Build targets for the FLUI application.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash)]
pub enum BuildTarget {
    /// Google Android
    Android,
    /// Apple iOS
    Ios,
    /// Web browser (WASM)
    Web,
    /// Microsoft Windows
    Windows,
    /// Linux distributions
    Linux,
    /// Apple macOS
    Macos,
    /// Build for the current host platform
    Desktop,
}

impl Display for BuildTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Android => write!(f, "android"),
            Self::Ios => write!(f, "ios"),
            Self::Web => write!(f, "web"),
            Self::Windows => write!(f, "windows"),
            Self::Linux => write!(f, "linux"),
            Self::Macos => write!(f, "macos"),
            Self::Desktop => write!(f, "desktop"),
        }
    }
}

impl BuildTarget {
    /// Get the corresponding Platform, if applicable.
    ///
    /// Returns `None` for `Desktop` which is host-dependent.
    #[must_use]
    pub const fn platform(&self) -> Option<Platform> {
        match self {
            Self::Android => Some(Platform::Android),
            Self::Ios => Some(Platform::Ios),
            Self::Web => Some(Platform::Web),
            Self::Windows => Some(Platform::Windows),
            Self::Linux => Some(Platform::Linux),
            Self::Macos => Some(Platform::Macos),
            Self::Desktop => None,
        }
    }

    /// Get the Rust target triple for this build target.
    #[must_use]
    pub fn target_triple(&self) -> &'static str {
        match self {
            Self::Windows => "x86_64-pc-windows-msvc",
            Self::Linux => "x86_64-unknown-linux-gnu",
            Self::Macos => "x86_64-apple-darwin",
            Self::Android => "aarch64-linux-android",
            Self::Ios => "aarch64-apple-ios",
            Self::Web => "wasm32-unknown-unknown",
            Self::Desktop => {
                #[cfg(target_os = "windows")]
                {
                    "x86_64-pc-windows-msvc"
                }
                #[cfg(target_os = "linux")]
                {
                    "x86_64-unknown-linux-gnu"
                }
                #[cfg(target_os = "macos")]
                {
                    "x86_64-apple-darwin"
                }
                #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
                {
                    "unknown"
                }
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose {
        flui_log::Level::DEBUG
    } else {
        flui_log::Level::INFO
    };

    flui_log::Logger::new().with_level(log_level).init();

    // Dispatch command
    let result: crate::error::CliResult<()> = match cli.command {
        Commands::Create {
            name,
            org,
            template,
            platforms,
            path,
            local,
            lib,
            interactive,
        } => {
            if interactive || name.is_none() {
                // Interactive mode — newtypes already validated by prompts
                (|| {
                    let config = commands::create_interactive::interactive_create()?;
                    commands::create::execute(
                        config.name,
                        config.org,
                        config.template,
                        config.platforms.or(platforms),
                        path,
                        local,
                        lib,
                    )
                })()
            } else {
                // Non-interactive mode — validate raw strings into newtypes
                (|| {
                    let Some(name) = name else {
                        unreachable!("name is Some due to previous check")
                    };
                    let project_name = crate::types::ProjectName::new(name)?;
                    let org_id = crate::types::OrganizationId::new(org)?;
                    commands::create::execute(
                        project_name,
                        org_id,
                        template,
                        platforms,
                        path,
                        local,
                        lib,
                    )
                })()
            }
        }

        Commands::Run {
            device,
            release,
            hot_reload,
            profile,
            verbose,
        } => commands::run::execute(device, release, hot_reload, profile, verbose),

        Commands::Build {
            platform,
            release,
            output,
            split_per_abi,
            optimize_wasm,
            universal,
        } => commands::build::execute(
            platform,
            release,
            output,
            split_per_abi,
            optimize_wasm,
            universal,
        ),

        Commands::Test {
            filter,
            unit,
            integration,
            platform,
        } => commands::test::execute(filter, unit, integration, platform),

        Commands::Analyze { fix, pedantic } => commands::analyze::execute(fix, pedantic),

        Commands::Doctor {
            verbose,
            android,
            ios,
            web,
        } => commands::doctor::execute(verbose, android, ios, web),

        Commands::Devices { details, platform } => commands::devices::execute(details, platform),

        Commands::Emulators { subcommand } => match subcommand {
            EmulatorSubcommand::List { platform } => {
                commands::emulators::execute_list(platform.as_deref())
            }
            EmulatorSubcommand::Launch { name } => commands::emulators::execute_launch(&name),
        },

        Commands::Clean { deep, platform } => commands::clean::execute(deep, platform),

        Commands::Upgrade {
            self_update,
            dependencies,
        } => commands::upgrade::execute(self_update, dependencies),

        Commands::Platform { subcommand } => match subcommand {
            PlatformSubcommand::Add { platforms } => commands::platform::add(&platforms),
            PlatformSubcommand::Remove { platform } => commands::platform::remove(&platform),
            PlatformSubcommand::List => commands::platform::list(),
        },

        Commands::Format { check } => commands::format::execute(check),

        Commands::Devtools { port } => commands::devtools::execute(port),

        Commands::Completions { shell } => commands::completions::execute(shell),
    };

    if let Err(e) = result {
        let _ = cliclack::log::error(format!("{e}"));
        std::process::exit(1);
    }
}

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

mod commands;
mod config;
pub mod error;
mod templates;
mod utils;

#[derive(Parser)]
#[command(name = "flui")]
#[command(about = "FLUI CLI - Build beautiful cross-platform apps with Rust", long_about = None)]
#[command(version)]
pub struct Cli {
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

    /// Launch emulators
    Emulators {
        /// Launch specific emulator
        #[arg(long)]
        launch: Option<String>,
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

    /// Update flui_cli and project dependencies
    Upgrade {
        /// Update flui_cli only
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

    /// Launch DevTools
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

#[derive(Subcommand)]
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

#[derive(Clone, Copy, ValueEnum, Debug)]
enum Template {
    /// Basic application
    Basic,
    /// Counter app (default)
    Counter,
    /// Todo list app
    Todo,
    /// Dashboard UI
    Dashboard,
    /// Widget package
    Widget,
    /// Plugin package
    Plugin,
    /// Empty project
    Empty,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum Platform {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Web,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum BuildTarget {
    Android,
    Ios,
    Web,
    Windows,
    Linux,
    Macos,
    Desktop,
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
    let result = match cli.command {
        Commands::Create {
            name,
            org,
            template,
            platforms,
            path,
            lib,
            interactive,
        } => {
            if interactive || name.is_none() {
                // Interactive mode
                match commands::create_interactive::interactive_create() {
                    Ok(config) => commands::create::execute(
                        config.name,
                        config.org,
                        config.template,
                        platforms,
                        path,
                        lib,
                    )
                    .map_err(|e| anyhow::Error::new(e)),
                    Err(e) => Err(anyhow::Error::new(e)),
                }
            } else if let Some(name) = name {
                // Non-interactive mode
                commands::create::execute(name, org, template, platforms, path, lib)
                    .map_err(|e| anyhow::Error::new(e))
            } else {
                unreachable!("name is Some due to previous check")
            }
        }

        Commands::Run {
            device,
            release,
            hot_reload,
            profile,
            verbose,
        } => commands::run::execute(device, release, hot_reload, profile, verbose)
            .map_err(|e| anyhow::Error::new(e)),

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
        )
        .map_err(|e| anyhow::Error::new(e)),

        Commands::Test {
            filter,
            unit,
            integration,
            platform,
        } => commands::test::execute(filter, unit, integration, platform)
            .map_err(|e| anyhow::Error::new(e)),

        Commands::Analyze { fix, pedantic } => {
            commands::analyze::execute(fix, pedantic).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Doctor {
            verbose,
            android,
            ios,
            web,
        } => commands::doctor::execute(verbose, android, ios, web)
            .map_err(|e| anyhow::Error::new(e)),

        Commands::Devices { details, platform } => {
            commands::devices::execute(details, platform).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Emulators { launch } => {
            commands::emulators::execute(launch).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Clean { deep, platform } => {
            commands::clean::execute(deep, platform).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Upgrade {
            self_update,
            dependencies,
        } => commands::upgrade::execute(self_update, dependencies)
            .map_err(|e| anyhow::Error::new(e)),

        Commands::Platform { subcommand } => match subcommand {
            PlatformSubcommand::Add { platforms } => {
                commands::platform::add(platforms).map_err(|e| anyhow::Error::new(e))
            }
            PlatformSubcommand::Remove { platform } => {
                commands::platform::remove(platform).map_err(|e| anyhow::Error::new(e))
            }
            PlatformSubcommand::List => commands::platform::list().map_err(|e| anyhow::Error::new(e)),
        },

        Commands::Format { check } => {
            commands::format::execute(check).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Devtools { port } => {
            commands::devtools::execute(port).map_err(|e| anyhow::Error::new(e))
        }

        Commands::Completions { shell } => {
            commands::completions::execute(shell).map_err(|e| anyhow::Error::new(e))
        }
    };

    if let Err(e) = result {
        eprintln!("\n{} {}", console::style("Error:").red().bold(), e);
        std::process::exit(1);
    }
}

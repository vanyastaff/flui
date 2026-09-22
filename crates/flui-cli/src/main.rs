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
/// Uses cyan for headers and literals to match the `ui` module's palette.
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::Green.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));

mod build;
mod commands;
mod config;
mod error;
mod proc;
mod runner;
mod templates;
mod types;
mod ui;
mod watch;

/// Command-line interface for FLUI - A declarative UI framework for Rust.
#[derive(Debug, Parser)]
#[command(name = "flui")]
#[command(about = "FLUI CLI - Build beautiful cross-platform apps with Rust", long_about = None)]
#[command(version)]
#[command(styles = STYLES)]
#[command(after_help = "\
Exit codes:
  0 success   2 usage   3 environment   4 build/test failed
  5 device not found   6 not a FLUI project   7 needs a terminal   130 Ctrl-C

Environment:
  CI, FLUI_NON_INTERACTIVE   never prompt (same as --non-interactive)
  NO_COLOR, CLICOLOR_FORCE   colour policy when --color=auto

flui sends no telemetry and never touches the network unless a command
explicitly downloads something (`flui upgrade`, `cargo` fetching crates).")]
pub(crate) struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    command: Commands,

    /// Show diagnostics: the commands flui runs, the probes it makes, the
    /// decisions it takes (dimmed `debug:` lines on stderr)
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Suppress progress narration; keep warnings, errors and tool output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Machine-readable output: one JSON object per line on stdout
    /// (`{"event": ...}`), nothing decorative anywhere
    #[arg(long, global = true)]
    json: bool,

    /// When to use colour
    #[arg(long, global = true, value_enum, default_value_t = ui::ColorChoice::Auto, value_name = "WHEN")]
    color: ui::ColorChoice,

    /// Never prompt or read hot-keys; fail with a hint instead
    /// (implied by CI=1, FLUI_NON_INTERACTIVE=1, or a non-terminal stdin)
    #[arg(long, global = true)]
    non_interactive: bool,
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

        /// Use a FLUI checkout (bare --local uses the current directory)
        #[arg(long, num_args = 0..=1, default_missing_value = ".", require_equals = true, value_name = "PATH")]
        local: Option<PathBuf>,

        /// Create a library instead of an application
        #[arg(long)]
        lib: bool,

        /// Interactive mode (prompt for all options)
        #[arg(short, long)]
        interactive: bool,

        /// Generate a Flutter-parity hot-reload project (three-crate
        /// host / worker / types layout).
        ///
        /// The host binary holds the element-tree state; the reloadable worker
        /// dylib holds only the `build()` implementations, so a code change to
        /// the worker preserves `State` across a reload. `flui run` detects the
        /// `[hot_reload]` section this writes and drives the worker rebuild.
        #[arg(long)]
        hot_reload: bool,

        /// Skip the `cargo check` that normally runs after scaffolding.
        ///
        /// The check only reports; it never fails the command. Skipping it
        /// makes `create` finish in well under a second — for scripted use,
        /// offline machines, or when you will build the project right away
        /// anyway.
        #[arg(long)]
        no_check: bool,

        /// Print the files that would be written and exit without writing
        /// anything (no directory, no git init, no cargo check).
        #[arg(long)]
        dry_run: bool,
    },

    /// Run the FLUI application with hot reload
    ///
    /// While the app runs, these keys work when stdin is a terminal:
    /// `r` rebuild and reload, `R` full restart, `c` clear the screen,
    /// `h` help, `q` (or Ctrl-C) quit and stop the app.
    Run {
        /// Target device: a name or UDID from `flui devices`
        /// (default: this desktop)
        #[arg(short, long)]
        device: Option<String>,

        /// Build in release mode
        #[arg(short, long)]
        release: bool,

        /// Disable hot reload: build and run once, then exit with the app
        #[arg(long, conflicts_with = "hot_reload")]
        no_hot_reload: bool,

        /// Enable hot reload (the default; kept for scripts)
        #[arg(long, hide = true)]
        hot_reload: bool,

        /// Scene-only hot-reload mode (Android): rebuild and push scene plugin
        /// without restarting the app. Much faster than full rebuild.
        #[arg(long, requires = "scene_crate", requires = "package")]
        scene: bool,

        /// Scene plugin crate name (used with --scene)
        #[arg(long, requires = "scene")]
        scene_crate: Option<String>,

        /// Android package name (used with --scene)
        #[arg(long, requires = "scene")]
        package: Option<String>,

        /// Android target ABI (used with --scene)
        #[arg(long, default_value = "arm64-v8a")]
        target: String,

        /// Build profile (dev, release, bench)
        #[arg(long)]
        profile: Option<String>,
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

        /// iOS: Build a static library/XCFramework rather than an application
        #[arg(long = "lib", conflicts_with = "example")]
        library: bool,

        /// iOS: Build for this exact available simulator UDID
        #[arg(long, conflicts_with = "universal")]
        simulator: Option<String>,

        /// iOS: Build device + simulator libraries (XCFramework without an Xcode project);
        /// macOS: fuse the arm64 and x86_64 binaries with `lipo`
        #[arg(long)]
        universal: bool,

        /// Build a named example instead of the current package's binary.
        ///
        /// Needed inside the FLUI source tree, where the runnable entry points
        /// are examples (`material_demo`, `widgets_gallery`, ...) rather than
        /// one application package.
        #[arg(long)]
        example: Option<String>,

        /// Build a named workspace package's binary.
        #[arg(long)]
        package: Option<String>,
    },

    /// Run tests
    Test {
        /// Test filter
        filter: Option<String>,

        /// Run unit tests only
        #[arg(long, conflicts_with = "integration")]
        unit: bool,

        /// Run integration tests only
        #[arg(long)]
        integration: bool,

        /// Build in release mode
        #[arg(short, long)]
        release: bool,

        /// Extra arguments for the test harness (after `--`)
        #[arg(last = true)]
        harness_args: Vec<String>,
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
    ///
    /// Exits 3 when a required component is missing. Optional toolchains
    /// (Android, iOS, Web) only warn unless selected explicitly.
    Doctor {
        /// Show paths and versions for every check
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

        /// Install what can be installed automatically (missing `rustup`
        /// targets) and re-check
        #[arg(long)]
        fix: bool,
    },

    /// List available devices
    Devices {
        /// Show detailed device information
        #[arg(long)]
        details: bool,

        /// Filter by platform
        #[arg(long, value_enum)]
        platform: Option<DevicePlatform>,
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

    /// Update the `flui` CLI and project dependencies
    Upgrade {
        /// Update the CLI only (`cargo install flui-cli`)
        #[arg(long = "self", conflicts_with = "dependencies")]
        self_update: bool,

        /// Update project dependencies only (`cargo update`)
        #[arg(long)]
        dependencies: bool,

        /// Report what would change without installing anything
        #[arg(long)]
        check: bool,
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

        /// Do not ask for confirmation (required in CI / with --json)
        #[arg(short = 'y', long)]
        yes: bool,
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
/// Every variant listed here generates a distinct, compile-tested project;
/// the CLI never silently substitutes another template.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) enum Template {
    /// Counter app: a stateful widget, a button, and a widget test (default)
    #[default]
    Counter,
    /// "Hello, FLUI!" stateless app with a Material theme
    Basic,
    /// The smallest runnable app: one `main` that shows one `Text`
    Empty,
    /// Reusable widget library (`--lib`): a `StatelessView` plus a widget test
    Widget,
}

impl Display for Template {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Basic => write!(f, "basic"),
            Self::Counter => write!(f, "counter"),
            Self::Widget => write!(f, "widget"),
            Self::Empty => write!(f, "empty"),
        }
    }
}

impl Template {
    /// Get a human-readable description of the template.
    #[must_use]
    pub(crate) const fn description(&self) -> &'static str {
        match self {
            Self::Basic => "Hello, FLUI! stateless app with a Material theme",
            Self::Counter => "Counter app with a stateful widget and a widget test",
            Self::Widget => "Reusable widget library with a widget test",
            Self::Empty => "Smallest runnable app",
        }
    }

    /// Whether this template produces a library crate rather than a binary.
    #[must_use]
    pub(crate) const fn is_library(&self) -> bool {
        matches!(self, Self::Widget)
    }
}

/// Platform filter for `flui devices`.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DevicePlatform {
    /// This machine
    Desktop,
    /// Android devices and emulators (via `adb`)
    Android,
    /// iOS simulators (macOS only)
    Ios,
    /// Installed web browsers
    Web,
}

/// Target platforms for FLUI applications.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
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

/// Build targets for the FLUI application.
#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BuildTarget {
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
    /// Get the Rust target triple for this build target.
    ///
    /// `macos` resolves to the *host* architecture's darwin triple: a build is
    /// normally for the machine running it, and returning a fixed
    /// `x86_64-apple-darwin` silently produced an Intel binary on Apple
    /// Silicon. `--universal` (handled by the builder, not here) widens it to
    /// both architectures.
    #[must_use]
    pub(crate) const fn target_triple(&self) -> &'static str {
        match self {
            Self::Windows => "x86_64-pc-windows-msvc",
            Self::Linux => "x86_64-unknown-linux-gnu",
            Self::Macos => Self::host_darwin_triple(),
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
                    Self::host_darwin_triple()
                }
                #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
                {
                    "unknown"
                }
            }
        }
    }

    /// The darwin triple matching the machine the CLI itself was built for.
    #[must_use]
    pub(crate) const fn host_darwin_triple() -> &'static str {
        #[cfg(target_arch = "aarch64")]
        {
            "aarch64-apple-darwin"
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            "x86_64-apple-darwin"
        }
    }
}

fn main() {
    let cli = Cli::parse();

    let verbosity = if cli.verbose {
        ui::Verbosity::Verbose
    } else if cli.quiet {
        ui::Verbosity::Quiet
    } else {
        ui::Verbosity::Normal
    };
    let mode = if cli.json {
        ui::OutputMode::Json
    } else {
        ui::OutputMode::Human
    };
    ui::install(mode, verbosity, cli.color, cli.non_interactive);
    let verbose = cli.verbose;

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
            no_check,
            hot_reload,
            dry_run,
        } => {
            let template = if lib { Template::Widget } else { template };
            let options = commands::create::CreateOptions {
                local,
                skip_check: no_check,
                hot_reload,
                dry_run,
            };
            if interactive || name.is_none() {
                // Interactive mode — newtypes already validated by prompts
                (|| {
                    if !ui::is_interactive() {
                        return Err(crate::error::CliError::NonInteractive {
                            what: "flui create without a project name".into(),
                            hint: "pass the name: flui create <NAME> [--org ORG] [--template T]"
                                .into(),
                        });
                    }
                    let config = commands::create_interactive::interactive_create()?;
                    commands::create::execute(
                        config.name,
                        config.org,
                        config.template,
                        config.platforms.or(platforms),
                        path,
                        options,
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
                        options,
                    )
                })()
            }
        }

        Commands::Run {
            device,
            release,
            no_hot_reload,
            hot_reload: _,
            scene,
            scene_crate,
            package,
            target,
            profile,
        } => {
            if scene {
                // clap enforces both via `requires`; the unwraps document that.
                let scene_crate = scene_crate
                    .expect("BUG: clap `requires` guarantees --scene-crate with --scene");
                let package =
                    package.expect("BUG: clap `requires` guarantees --package with --scene");
                commands::run::execute_scene(&scene_crate, &package, &target, release, verbose)
            } else {
                commands::run::execute(device, release, !no_hot_reload, profile, verbose)
            }
        }

        Commands::Build {
            platform,
            release,
            output,
            universal,
            example,
            package,
            library,
            simulator,
        } => commands::build::execute(
            platform, release, output, universal, example, package, library, simulator,
        ),

        Commands::Test {
            filter,
            unit,
            integration,
            release,
            harness_args,
        } => commands::test::execute(commands::test::TestOptions {
            filter,
            unit,
            integration,
            release,
            harness_args,
        }),

        Commands::Analyze { fix, pedantic } => commands::analyze::execute(fix, pedantic),

        Commands::Doctor {
            verbose: doctor_verbose,
            android,
            ios,
            web,
            fix,
        } => commands::doctor::execute(commands::doctor::DoctorOptions {
            verbose: doctor_verbose || verbose,
            android,
            ios,
            web,
            fix,
        }),

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
            check,
        } => commands::upgrade::execute(self_update, dependencies, check),

        Commands::Platform { subcommand } => match subcommand {
            PlatformSubcommand::Add { platforms } => commands::platform::add(&platforms),
            PlatformSubcommand::Remove { platform, yes } => {
                commands::platform::remove(&platform, yes)
            }
            PlatformSubcommand::List => commands::platform::list(),
        },

        Commands::Format { check } => commands::format::execute(check),

        Commands::Completions { shell } => commands::completions::execute(shell),
    };

    if let Err(e) = result {
        let code = e.exit_code();
        match e {
            // A deliberate cancel is not a failure: no red text, exit 0.
            crate::error::CliError::UserCancelled => {
                let _ = ui::outro_cancel("Cancelled");
            }
            crate::error::CliError::Interrupted => {
                let _ = ui::outro_cancel("Interrupted");
                ui::emit(
                    "error",
                    &serde_json::json!({ "message": "interrupted", "code": code }),
                );
            }
            e => {
                let message = format_error_chain(&e);
                let _ = ui::error(&message);
                ui::emit(
                    "error",
                    &serde_json::json!({ "message": message, "code": code }),
                );
            }
        }
        std::process::exit(code);
    }
}

/// Keep command context and its actionable underlying cause visible together.
fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str("\nCaused by: ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod desktop_error_tests {
    #[test]
    fn command_context_keeps_the_actionable_build_cause() {
        let error = crate::error::CliError::context(
            crate::build::error::BuildError::path_not_found(
                "/custom target/app".into(),
                "Cargo executable absent",
            ),
            "failed to build the binary",
        );
        let text = super::format_error_chain(&error);
        assert!(text.contains("failed to build the binary"));
        assert!(text.contains("/custom target/app"));
        assert!(text.contains("Cargo executable absent"));
    }
}

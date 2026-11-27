use std::path::PathBuf;

use crate::build::app_version::{AppVersion, BuildNumber, PackageNumbers};
use crate::build::error::{BuildError, BuildResult};
use crate::types::DisplayName;

/// Build context containing configuration and paths.
///
/// Use [`BuilderContextBuilder`](crate::build::BuilderContextBuilder) to construct instances.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub(crate) struct BuilderContext {
    /// Root directory of the workspace
    pub(crate) workspace_root: PathBuf,
    /// Target platform to build for
    pub(crate) platform: Platform,
    /// Which cargo package or example to compile.
    pub(crate) target: BuildUnit,
    /// Build profile (debug or release)
    pub(crate) profile: Profile,
    /// Output directory for build artifacts
    pub(crate) output_dir: PathBuf,
    /// Application bundle metadata, when the target stages a platform bundle.
    ///
    /// `None` keeps the backend's plain-artifact behaviour (a bare executable
    /// copy). On macOS a `Some` stages a `.app` whose `Info.plist` names the
    /// application — the bundle a double-clickable, foreground-activatable app
    /// needs, and which a bare Mach-O cannot substitute for.
    pub(crate) bundle: Option<AppBundle>,
    /// The floors the build packages and links with (ADR-0244 §10): FLUI's,
    /// raised where `flui.toml` asks.
    pub(crate) floors: crate::build::app_floor::AppFloors,
    /// The agent's `driver` feature (`flui/driver`, as `--features` takes
    /// it), for an optimized build the agent connects to.
    pub(crate) agent_driver: Option<String>,
}

impl BuilderContext {
    /// The cargo arguments of this build's mode for a package that reaches
    /// FLUI through `framework`: the profile's flag and the features the
    /// mode turns on
    /// ([`FrameworkDependency::mode_features`](crate::build::framework::FrameworkDependency::mode_features)).
    #[must_use]
    pub(crate) fn mode_args(
        &self,
        framework: &crate::build::framework::FrameworkDependency,
    ) -> Vec<String> {
        let mut args: Vec<String> = self
            .profile
            .cargo_flag()
            .map(str::to_owned)
            .into_iter()
            .collect();
        for feature in framework.mode_features(self.profile, self.agent_driver.as_deref()) {
            args.extend(["--features".to_owned(), feature]);
        }
        args
    }

    /// The rustflags cargo is given for `triple` on top of the author's: a
    /// release build rewrites the build machine's source paths
    /// ([`source_paths`](crate::build::source_paths)), a debug build adds
    /// nothing.
    ///
    /// # Errors
    ///
    /// What [`release_rustflags`](crate::build::source_paths::release_rustflags)
    /// answers.
    pub(crate) fn rustflags(
        &self,
        triple: &str,
    ) -> crate::build::error::BuildResult<crate::build::source_paths::CargoRustflags> {
        match self.profile {
            Profile::Debug => Ok(crate::build::source_paths::CargoRustflags::default()),
            Profile::Release => {
                crate::build::source_paths::release_rustflags(&self.workspace_root, triple)
            }
        }
    }
}

/// Identity an application bundle is staged under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppBundle {
    /// The application's name (the `.app` stem and `CFBundleName`).
    pub(crate) name: String,
    /// The name it is shown by (`CFBundleDisplayName`, the desktop entry's
    /// `Name`), when `flui.toml` declares one ([`AppBundle::shown_name`]).
    pub(crate) display_name: Option<DisplayName>,
    /// Reverse-DNS bundle identifier (`CFBundleIdentifier`).
    pub(crate) identifier: String,
    /// The version `flui.toml` declares (`[app] version`); `None` for a
    /// bundle named from its directory, which takes the version of the
    /// package the build compiled ([`AppBundle::package_numbers`]).
    pub(crate) version: Option<AppVersion>,
    /// The build number `flui.toml` declares (`[app] build`).
    pub(crate) build: Option<BuildNumber>,
    /// Whether `identifier` derives from the id `flui.toml` declares, which
    /// every command compiles the application with (`FLUI_APP_ID`); not for
    /// one derived from the directory, which the application never carries.
    pub(crate) declared: bool,
}

impl AppBundle {
    /// Bundle metadata for the application `name` (the `.app` stem and
    /// `CFBundleName`) under `identifier`, which callers derive with
    /// [`AppId::apple_bundle_id`](crate::app_id::AppId::apple_bundle_id); the
    /// identifier is not a declared one until [`declared`](Self::declared),
    /// and the bundle states the built package's version.
    #[must_use]
    pub(crate) fn new(name: &str, identifier: String) -> Self {
        Self {
            name: name.to_string(),
            display_name: None,
            identifier,
            version: None,
            build: None,
            declared: false,
        }
    }

    /// The bundle `app` (`flui.toml`'s `[app]`) declares, under `identifier`,
    /// which callers derive from its id: its names, its version and build
    /// number, the identifier a declared one.
    #[must_use]
    pub(crate) fn of_app(app: &crate::config::AppConfig, identifier: String) -> Self {
        Self {
            name: app.name.to_string(),
            display_name: app.display_name.clone(),
            identifier,
            version: Some(app.version.clone()),
            build: app.build,
            declared: true,
        }
    }

    /// The same bundle, its identifier derived from the declared id, as
    /// [`AppBundle::of_app`] makes it.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn declared(mut self) -> Self {
        self.declared = true;
        self
    }

    /// The name the bundle is shown by: the declared display name, else its
    /// name.
    #[must_use]
    pub(crate) fn shown_name(&self) -> &str {
        self.display_name
            .as_ref()
            .map_or(self.name.as_str(), DisplayName::as_str)
    }

    /// The numbers a bundle built with `profile` states: from the declared
    /// version and build number, else from the version of `package`, the
    /// member the build compiled.
    ///
    /// # Errors
    ///
    /// [`BuildError::InvalidConfig`] when the bundle declares no version and
    /// the build names no package, or the package's version is one no
    /// packager takes; [`BuildError::AppVersion`] for a release build of a
    /// pre-release without a build number.
    pub(crate) fn package_numbers(
        &self,
        package: Option<&BuiltPackage>,
        profile: Profile,
    ) -> BuildResult<PackageNumbers> {
        let version = match (&self.version, package) {
            (Some(version), _) => version.clone(),
            (None, Some(package)) => AppVersion::try_new(&package.version).map_err(|error| {
                BuildError::invalid_config(format!("{}'s version", package.name), error.to_string())
            })?,
            (None, None) => {
                return Err(BuildError::invalid_config(
                    "bundle version",
                    "flui.toml declares no `[app] version` and the build compiled no named \
                     package",
                ));
            }
        };
        Ok(version.package_numbers(self.build, profile)?)
    }
}

/// What cargo should compile within the workspace.
///
/// A desktop build produces an *executable*, and which executable differs by
/// call site: a generated application project has one binary package, while the
/// FLUI source tree is a workspace whose runnable entry points are examples.
/// This is the knob that keeps `DesktopBuilder` from hard-coding one of them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub(crate) enum BuildUnit {
    /// Build the current package's default binary (`cargo build` in `workspace_root`).
    #[default]
    DefaultBinary,
    /// Build the named workspace package's binary.
    Package(String),
    /// Build the named example of the current package
    /// (`cargo build --example NAME`).
    Example(String),
    /// Build a static library for iOS XCFramework delivery.
    Library {
        /// Workspace package name, or current/default package selection.
        package: Option<String>,
    },
}

/// Platform to build for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    /// Android platform with target architectures
    Android {
        /// Target architectures (e.g., "aarch64-linux-android")
        targets: Vec<String>,
    },
    /// iOS platform with target architectures
    Ios {
        /// Target architectures (e.g., "aarch64-apple-ios", "x86_64-apple-ios")
        targets: Vec<String>,
    },
    /// Web/WASM platform
    Web {
        /// What the build states in its page and manifest.
        page: crate::build::web_page::WebPage,
    },
    /// Desktop platform (Windows, macOS, Linux)
    Desktop {
        /// Optional target triple (auto-detected if None)
        target: Option<String>,
    },
}

impl Platform {
    /// Returns the platform name as a string
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        match self {
            Platform::Android { .. } => "android",
            Platform::Ios { .. } => "ios",
            Platform::Web { .. } => "web",
            Platform::Desktop { .. } => "desktop",
        }
    }
}

/// Build profile (debug or release)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Profile {
    /// Debug profile (default) - faster compilation, includes debug symbols
    #[default]
    Debug,
    /// Release profile - optimized, slower compilation
    Release,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl Profile {
    /// Returns the cargo flag for this profile
    ///
    /// Returns `None` for Debug (default), `Some("--release")` for Release
    #[must_use]
    pub(crate) fn cargo_flag(&self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

    /// Returns the profile name as a string
    #[must_use]
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }

    /// The build mode of a run that passes `--release` or names a cargo
    /// profile with `--profile`. Cargo's `release` profile is the release
    /// mode whichever flag names it; any other named profile is not, whatever
    /// it optimizes, so a `profiling` build keeps its spans (ADR-0398 §2).
    /// `flui run` reads every target's mode from here.
    #[must_use]
    pub(crate) fn of_run(release: bool, cargo_profile: Option<&str>) -> Self {
        if release || cargo_profile == Some("release") {
            Profile::Release
        } else {
            Profile::Debug
        }
    }
}

/// Build artifacts produced by Rust compilation
#[derive(Debug)]
pub(crate) struct BuildArtifacts {
    /// Paths to compiled Rust libraries (.a, .so, .dll, .dylib, .wasm)
    pub(crate) rust_libs: Vec<PathBuf>,
    /// Path to a compiled executable, when the target produces one.
    ///
    /// Desktop builds and executable iOS examples produce an executable. Other mobile/web builds produce
    /// libraries consumed by a platform bundle step and leave this `None`.
    pub(crate) executable: Option<PathBuf>,
    /// The workspace member the build compiled, when it selected one;
    /// `None` for a build of what cargo builds by default at the project.
    pub(crate) package: Option<BuiltPackage>,
}

/// The workspace member a build compiled one target of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuiltPackage {
    /// The member's package name.
    pub(crate) name: String,
    /// The member's version, as its manifest states it.
    pub(crate) version: String,
    /// Whether the target built links the member's dev-dependencies as
    /// well, as an example does and a binary does not.
    pub(crate) links_dev_dependencies: bool,
}

/// Delivered file or bundle directory after a platform-specific build.
#[derive(Debug)]
pub(crate) struct FinalArtifacts {
    /// Path to the delivered artifact: a file (APK, WASM, executable, etc.) or
    /// a bundle directory (`.app` or `.xcframework`). iOS library delivery
    /// returns an XCFramework directory.
    pub(crate) app_binary: PathBuf,
    /// File size, or the sum of contained file sizes for a bundle, in bytes.
    pub(crate) size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::{BuildUnit, Platform, Profile};
    use crate::build::BuilderContextBuilder;
    use crate::build::framework::FrameworkDependency;

    /// Every platform's build takes its mode from `mode_args`: the profile's
    /// flag, then the release ceiling through the key the package names the
    /// facade under, or the agent's driver in its place (ADR-0398 §2).
    #[test]
    fn a_build_mode_is_its_profile_flag_and_its_features() {
        let framework = FrameworkDependency::Named {
            key: "ui".to_owned(),
            optional: false,
        };
        for (profile, driver, expected) in [
            (
                Profile::Release,
                None,
                &["--release", "--features", "ui/release-max-level-info"][..],
            ),
            (Profile::Debug, None, &[][..]),
            (
                Profile::Release,
                Some("ui/driver"),
                &["--release", "--features", "ui/driver"][..],
            ),
        ] {
            let ctx = BuilderContextBuilder::new(std::path::PathBuf::from("app"))
                .platform(Platform::Desktop { target: None })
                .target(BuildUnit::DefaultBinary)
                .profile(profile)
                .output_dir(std::path::PathBuf::from("out"))
                .agent_driver(driver.map(str::to_owned))
                .build();
            assert_eq!(
                ctx.mode_args(&framework),
                expected,
                "{profile:?} {driver:?}"
            );
        }
    }

    /// A run's mode is the release mode when either flag names cargo's
    /// `release` profile, and no other profile is, so every target of
    /// `flui run` builds the same thing from the same command line.
    #[test]
    fn a_run_is_the_release_mode_whichever_flag_names_the_release_profile() {
        for (release, cargo_profile, expected) in [
            (true, None, Profile::Release),
            (false, Some("release"), Profile::Release),
            (false, Some("profiling"), Profile::Debug),
            (false, Some("dev"), Profile::Debug),
            (false, None, Profile::Debug),
        ] {
            assert_eq!(
                Profile::of_run(release, cargo_profile),
                expected,
                "--release {release}, --profile {cargo_profile:?}"
            );
        }
    }
}

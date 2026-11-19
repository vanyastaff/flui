// Only the macOS bundle staging below takes a `Path`; on other hosts the
// import would be unused and trip `-D warnings`.
#[cfg(target_os = "macos")]
use std::path::Path;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts};
use crate::build::util::cargo;
use crate::declarations::{Build, Declarations, Selection};

/// Builder for desktop platforms (Windows, macOS, Linux)
#[derive(Debug, Default)]
pub(crate) struct DesktopBuilder;

impl DesktopBuilder {
    /// Create a stateless desktop builder; each operation uses its `BuilderContext`.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }

    /// The host target triple of the toolchain the project in `dir` selects, from
    /// `rustc -vV` under a deadline.
    ///
    /// # Errors
    ///
    /// When `rustc` cannot be started, fails, does not answer in time or
    /// names no host: a guessed triple would build for the wrong target.
    pub(crate) fn detect_host_target(dir: &std::path::Path) -> BuildResult<String> {
        crate::build::util::environment::rust_host_triple(dir)
    }
}

/// Whether `triple` names a desktop Linux target (Android's triples name
/// `linux` too, and are not one).
pub(crate) fn is_linux_triple(triple: &str) -> bool {
    triple.contains("-linux-") && !triple.contains("android")
}

impl DesktopBuilder {
    /// The environment cargo builds the application for `target` with: the
    /// floors `ctx` names, and never an application id. The id reaches the
    /// build only from the project's cargo configuration, which every `flui`
    /// command writes from the `[app] id` of `flui.toml` (ADR-0351), so one
    /// application is compiled with one identity whichever command, target or
    /// example builds it, and an application without a declared id is
    /// compiled with none.
    pub(crate) fn cargo_env(ctx: &BuilderContext, target: &str) -> Vec<(&'static str, String)> {
        ctx.floors.cargo_env(target)
    }

    pub(crate) fn validate_environment() -> BuildResult<()> {
        // Just need cargo
        crate::build::util::check_command_exists("cargo")?;
        Ok(())
    }

    pub(crate) async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let target = match &ctx.platform {
            crate::build::platform::Platform::Desktop { target } => match target {
                Some(t) => t.clone(),
                None => Self::detect_host_target(&ctx.workspace_root)?,
            },
            _ => {
                return Err(BuildError::InvalidPlatform {
                    reason: "Expected Desktop platform".to_string(),
                });
            }
        };

        crate::ui::debug(format!(
            "Building desktop target '{target}' ({:?})",
            ctx.target
        ));
        let selected = cargo::select_target(&ctx.workspace_root, &ctx.target).await?;
        // Before the build: a Linux or Windows application that opens links
        // or files needs the code that keeps one instance of it, read from
        // the member the build compiles.
        let package = selected.built_package();
        if is_linux_triple(&target) || target.contains("-windows-") {
            Declarations::collect(
                Build::at(&ctx.workspace_root).selecting(Selection::of_built(Some(&package))),
                &[&target],
            )?
            .require_one_instance(&target)?;
        }

        let mut args = vec!["build".to_string(), "--target".to_string(), target.clone()];
        args.extend(selected.cargo_args());
        if let Some(profile_flag) = ctx.profile.cargo_flag() {
            args.push(profile_flag.to_string());
        }

        let env = Self::cargo_env(ctx, &target);
        let executable = cargo::build_artifact(&ctx.workspace_root, &args, &selected, &env).await?;

        Ok(BuildArtifacts {
            rust_libs: Vec::new(),
            executable: Some(executable),
            package: Some(package),
        })
    }

    pub(crate) fn build_platform(
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        let executable = artifacts.executable.as_deref().ok_or_else(|| {
            BuildError::Other(
                "Desktop build produced no executable — is the target package a binary?"
                    .to_string(),
            )
        })?;

        // macOS with bundle metadata stages a `.app`; every other case (and
        // macOS without metadata) copies the bare executable. A bundle is what
        // a foreground-activatable, double-clickable app needs; a bare Mach-O
        // launched from a terminal is not equivalent.
        #[cfg(target_os = "macos")]
        if let Some(bundle) = &ctx.bundle {
            return Self::stage_macos_app(ctx, artifacts, executable, bundle);
        }

        // Refused before the output changes: assets no package can carry.
        let assets = crate::build::assets::of_project(&ctx.workspace_root)?;
        std::fs::create_dir_all(&ctx.output_dir)?;

        let file_name = executable.file_name().ok_or_else(|| {
            BuildError::invalid_config(
                "artifacts.executable",
                format!("executable path {} has no file name", executable.display()),
            )
        })?;
        let output_binary = ctx.output_dir.join(file_name);

        std::fs::copy(executable, &output_binary)?;
        // The copy carries the source's mode; cargo's output is already
        // executable on Unix, but a permissions-preserving copy is not
        // guaranteed across every platform, so set the bit explicitly.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&output_binary)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&output_binary, perms)?;
        }

        // Beside the executable, where the host reads them (ADR-0384 §3).
        crate::build::assets::stage(&assets, &ctx.output_dir)?;
        let size_bytes = std::fs::metadata(&output_binary)?.len();

        crate::ui::debug(format!(
            "Desktop executable copied to: {}",
            output_binary.display()
        ));

        // A Linux desktop learns what the application opens from its desktop
        // entry and MIME package, staged beside the executable.
        if let (Some(triple), Some(bundle)) = (Self::linux_target(ctx)?, &ctx.bundle) {
            let declarations = Self::declarations(ctx, artifacts, &triple)?;
            let executable_name = file_name.to_string_lossy();
            for path in crate::build::linux_desktop::stage(
                &ctx.output_dir,
                bundle,
                &executable_name,
                &declarations,
            )? {
                crate::ui::debug(format!("Desktop registration written: {}", path.display()));
            }
        }

        Ok(FinalArtifacts {
            app_binary: output_binary,
            size_bytes,
        })
    }
}

impl DesktopBuilder {
    /// What the crates `artifacts` link for `triple` declare: those of the
    /// member the build compiled, not of its siblings in the workspace.
    fn declarations(
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
        triple: &str,
    ) -> BuildResult<Declarations> {
        Declarations::collect(
            Build::at(&ctx.workspace_root)
                .selecting(Selection::of_built(artifacts.package.as_ref())),
            &[triple],
        )
    }

    /// The Linux triple `ctx` builds for: its named target when that is a
    /// Linux one, else the host's when the build names none and runs on
    /// Linux; `None` for any other desktop.
    fn linux_target(ctx: &BuilderContext) -> BuildResult<Option<String>> {
        match &ctx.platform {
            crate::build::platform::Platform::Desktop {
                target: Some(target),
            } => Ok(is_linux_triple(target).then(|| target.clone())),
            crate::build::platform::Platform::Desktop { target: None }
                if cfg!(target_os = "linux") =>
            {
                Self::detect_host_target(&ctx.workspace_root).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Stage a built executable into a minimal, well-formed macOS `.app`.
    ///
    /// The bundle is what makes the app a real application: `Info.plist` names
    /// it and declares `NSPrincipalClass`, and the executable lives at
    /// `Contents/MacOS/<name>`. `flui build macos` yields a
    /// launchable app rather than a loose binary.
    ///
    /// `CFBundleExecutable` must equal the file placed in `Contents/MacOS`, so
    /// the caller's executable keeps its own file name there regardless of the
    /// bundle's display name.
    #[cfg(target_os = "macos")]
    fn stage_macos_app(
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
        executable: &Path,
        bundle: &crate::build::platform::AppBundle,
    ) -> BuildResult<FinalArtifacts> {
        validate_bundle_name(&bundle.name)?;
        let exe_name = executable.file_name().ok_or_else(|| {
            BuildError::invalid_config(
                "artifacts.executable",
                format!("executable path {} has no file name", executable.display()),
            )
        })?;
        let exe_name = exe_name.to_string_lossy();
        // The release rustc linked the binary for (`MACOSX_DEPLOYMENT_TARGET`),
        // so the bundle and the binary state the same minimum.
        let triple = match &ctx.platform {
            crate::build::platform::Platform::Desktop {
                target: Some(target),
            } => target.clone(),
            _ => format!("{}-apple-darwin", std::env::consts::ARCH),
        };
        // Everything the plist states is decided, and every refusal made,
        // before an earlier bundle is removed: a refused build leaves it.
        let declarations = Self::declarations(ctx, artifacts, &triple)?;
        crate::build::apple_opening::warn_about_types_only_a_linked_crate_declares(&declarations);
        let numbers = bundle.package_numbers(artifacts.package.as_ref(), ctx.profile)?;
        let assets = crate::build::assets::of_project(&ctx.workspace_root)?;
        let plist = macos_info_plist(&MacosBundleFacts {
            bundle,
            numbers: &numbers,
            executable: &exe_name,
            minimum: &ctx.floors.macos(&triple).to_string(),
            declarations: &declarations,
        });

        let app_dir = ctx.output_dir.join(format!("{}.app", bundle.name));
        let contents = app_dir.join("Contents");
        let macos_dir = contents.join("MacOS");
        let resources_dir = contents.join("Resources");

        match std::fs::symlink_metadata(&app_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(BuildError::invalid_config(
                    "bundle output",
                    "Refusing to replace a symbolic link at the application bundle path",
                ));
            }
            Ok(_) => std::fs::remove_dir_all(&app_dir)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        std::fs::create_dir_all(&macos_dir)?;
        std::fs::create_dir_all(&resources_dir)?;

        let staged_exe = macos_dir.join(exe_name.as_ref());
        std::fs::copy(executable, &staged_exe)?;
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&staged_exe)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&staged_exe, perms)?;
        }
        std::fs::write(contents.join("Info.plist"), plist)?;
        // The bundle's resources, where `NSBundle` finds them (ADR-0384 §3).
        crate::build::assets::stage(&assets, &resources_dir)?;

        let size_bytes = calculate_dir_size(&app_dir)?;
        crate::ui::debug(format!("macOS .app bundle staged at {}", app_dir.display()));

        Ok(FinalArtifacts {
            app_binary: app_dir,
            size_bytes,
        })
    }
}

/// What a macOS bundle's `Info.plist` states.
#[cfg(any(target_os = "macos", test))]
struct MacosBundleFacts<'a> {
    bundle: &'a crate::build::platform::AppBundle,
    /// The numbers the bundle states ([`AppBundle::package_numbers`]).
    ///
    /// [`AppBundle::package_numbers`]: crate::build::platform::AppBundle::package_numbers
    numbers: &'a crate::build::app_version::PackageNumbers,
    /// The file name of the executable in `Contents/MacOS`.
    executable: &'a str,
    /// The release the binary was linked for (`LSMinimumSystemVersion`).
    minimum: &'a str,
    /// What the application opens.
    declarations: &'a Declarations,
}

/// The `Info.plist` of a macOS application bundle, as an XML property list.
///
/// The version keys are the iOS bundle's ([`AppVersion::package_numbers`]):
/// `CFBundleVersion` the build number, `CFBundleShortVersionString` the
/// version's `major.minor.patch`, `FLUIVersion` the version as written.
/// `CFBundleDisplayName` is the display name, `CFBundleName` the name. Every
/// value is a property-list string, escaped as the XML renderer escapes each
/// one, never spliced into the document's text.
///
/// [`AppVersion::package_numbers`]: crate::build::app_version::AppVersion::package_numbers
#[cfg(any(target_os = "macos", test))]
fn macos_info_plist(facts: &MacosBundleFacts<'_>) -> String {
    use crate::build::apple_opening::{ApplePlatform, info_plist_keys, plist_xml_document};
    use serde_json::Value;

    let numbers = facts.numbers;
    let mut keys = serde_json::Map::new();
    for (key, value) in [
        ("CFBundlePackageType", Value::from("APPL")),
        ("CFBundleName", Value::from(facts.bundle.name.as_str())),
        (
            "CFBundleDisplayName",
            Value::from(facts.bundle.shown_name()),
        ),
        (
            "CFBundleIdentifier",
            Value::from(facts.bundle.identifier.as_str()),
        ),
        ("CFBundleExecutable", Value::from(facts.executable)),
        (
            "CFBundleVersion",
            Value::from(numbers.apple_bundle_version.as_str()),
        ),
        (
            "CFBundleShortVersionString",
            Value::from(numbers.apple_short_version.as_str()),
        ),
        ("FLUIVersion", Value::from(numbers.as_written.as_str())),
        ("LSMinimumSystemVersion", Value::from(facts.minimum)),
        ("NSPrincipalClass", Value::from("NSApplication")),
        ("NSHighResolutionCapable", Value::Bool(true)),
    ] {
        keys.insert(key.to_owned(), value);
    }
    keys.extend(info_plist_keys(
        &facts.bundle.identifier,
        facts.declarations,
        ApplePlatform::Macos,
    ));
    plist_xml_document(&keys)
}

/// Refuse a bundle name that is not one plain file name, or that holds a
/// character no XML property list can carry.
#[cfg(target_os = "macos")]
fn validate_bundle_name(name: &str) -> BuildResult<()> {
    let mut components = Path::new(name).components();
    if name.contains(['/', '\\', '\0'])
        || !crate::build::util::xml::is_xml_name_text(name)
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(BuildError::invalid_config(
            "bundle.name",
            "Application name must be a single nonempty filename component without separators, \
             control characters, U+FFFE, U+FFFF, '.' or '..'",
        ));
    }
    Ok(())
}

/// Total size of every file under `dir`, recursively.
///
/// Used to report a staged macOS `.app`'s on-disk size to the caller; gated
/// with its only caller, [`DesktopBuilder::stage_macos_app`], so a non-macOS
/// build does not carry it as dead code.
#[cfg(target_os = "macos")]
fn calculate_dir_size(dir: &Path) -> BuildResult<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            total += calculate_dir_size(&path)?;
        } else {
            total += std::fs::metadata(&path)?.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::{MacosBundleFacts, macos_info_plist};
    use crate::build::app_version::{AppVersion, BuildNumber};
    use crate::build::platform::{AppBundle, Profile};
    use crate::declarations::Declarations;
    use crate::types::DisplayName;

    /// The macOS bundle states the version and the build number `flui.toml`
    /// declares, as the iOS bundle does: the build number as
    /// `CFBundleVersion`, `major.minor.patch` as `CFBundleShortVersionString`
    /// and the version as written beside them; the display name is
    /// `CFBundleDisplayName`, and names holding XML's metacharacters stay
    /// well-formed strings. The whole document is the golden, so a value
    /// hard-coded again shows.
    #[test]
    fn the_macos_info_plist_states_the_application_version() {
        let mut bundle = AppBundle::new("R&D <β>", "com.example.notes".to_owned()).declared();
        bundle.version =
            Some(AppVersion::try_new("1.4.2-beta.1+build.7").expect("a SemVer version"));
        bundle.build = Some(BuildNumber::try_new(17).expect("a build number"));
        bundle.display_name =
            Some(DisplayName::try_new("My \"R&D\" Notes").expect("a display name"));
        let numbers = bundle
            .package_numbers(None, Profile::Release)
            .expect("a release with a build number");
        let declarations = Declarations::of_table(
            "notes",
            "schema = 3\nurl-schemes = [\"notes\"]\n\
             [[file-types]]\nextension = \"note\"\nmime = \"application/x-notes-note\"\n\
             name = \"Note\"\n\
             [[file-types]]\nextension = \"png\"\nmime = \"image/png\"\nname = \"Image\"\n",
        );
        let plist = macos_info_plist(&MacosBundleFacts {
            bundle: &bundle,
            numbers: &numbers,
            executable: "notes",
            minimum: "11.0",
            declarations: &declarations,
        });
        let parsed = roxmltree::Document::parse_with_options(
            &plist,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..roxmltree::ParsingOptions::default()
            },
        )
        .expect("the plist is XML");
        let value_of = |key: &str| {
            parsed
                .descendants()
                .find(|node| node.has_tag_name("key") && node.text() == Some(key))
                .and_then(|node| node.next_sibling_element())
                .and_then(|node| node.text())
                .map(str::to_owned)
        };
        for (key, expected) in [
            ("CFBundleVersion", "17"),
            ("CFBundleShortVersionString", "1.4.2"),
            ("FLUIVersion", "1.4.2-beta.1+build.7"),
            ("CFBundleName", "R&D <β>"),
            ("CFBundleDisplayName", "My \"R&D\" Notes"),
        ] {
            assert_eq!(value_of(key).as_deref(), Some(expected), "{key}");
        }
        insta::assert_snapshot!("macos_info_plist", plist);
    }
}

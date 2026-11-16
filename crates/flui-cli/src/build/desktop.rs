// Only the macOS bundle staging below takes a `Path`; on other hosts the
// import would be unused and trip `-D warnings`.
#[cfg(target_os = "macos")]
use std::path::Path;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts};
use crate::build::util::cargo;
#[cfg(target_os = "macos")]
use crate::build::util::xml::xml_escape;
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

impl DesktopBuilder {
    /// The identifier a build for `target` compiles in (`FLUI_APP_ID`), when
    /// it is a Linux build staging the desktop entry of `bundle`: the entry's
    /// own identifier, which names its file and is its `StartupWMClass`, so
    /// the window that carries it as its class and app id matches the entry.
    /// An application without a `flui.toml` has it from its directory alone.
    /// `None` for every other build, which takes the identifier from the
    /// project's cargo configuration.
    pub(crate) fn compiled_identity(
        target: &str,
        bundle: Option<&crate::build::platform::AppBundle>,
    ) -> Option<(&'static str, String)> {
        let bundle = bundle.filter(|_| target.contains("-linux-"))?;
        Some((crate::config::APP_ID_ENV, bundle.identifier.clone()))
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
        if target.contains("-linux-") || target.contains("-windows-") {
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

        let mut env = ctx.floors.cargo_env(&target);
        env.extend(Self::compiled_identity(&target, ctx.bundle.as_ref()));
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
        let is_linux = |triple: &str| triple.contains("-linux-") && !triple.contains("android");
        match &ctx.platform {
            crate::build::platform::Platform::Desktop {
                target: Some(target),
            } => Ok(is_linux(target).then(|| target.clone())),
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

        let exe_name = executable.file_name().ok_or_else(|| {
            BuildError::invalid_config(
                "artifacts.executable",
                format!("executable path {} has no file name", executable.display()),
            )
        })?;
        let exe_name = exe_name.to_string_lossy();

        let staged_exe = macos_dir.join(exe_name.as_ref());
        std::fs::copy(executable, &staged_exe)?;
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&staged_exe)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&staged_exe, perms)?;
        }

        // The release rustc linked the binary for (`MACOSX_DEPLOYMENT_TARGET`),
        // so the bundle and the binary state the same minimum.
        let triple = match &ctx.platform {
            crate::build::platform::Platform::Desktop {
                target: Some(target),
            } => target.clone(),
            _ => format!("{}-apple-darwin", std::env::consts::ARCH),
        };
        // What the application opens, from the crates it links for this target.
        let declarations = Self::declarations(ctx, artifacts, &triple)?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundleDisplayName</key>
    <string>{name}</string>
    <key>CFBundleIdentifier</key>
    <string>{identifier}</string>
    <key>CFBundleExecutable</key>
    <string>{executable}</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>{minimum}</string>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHighResolutionCapable</key>
    <true/>
{opening}</dict>
</plist>
"#,
            name = xml_escape(&bundle.name),
            identifier = xml_escape(&bundle.identifier),
            executable = xml_escape(&exe_name),
            minimum = ctx.floors.macos(&triple),
            opening = crate::build::apple_opening::plist_xml_entries(
                &crate::build::apple_opening::info_plist_keys(
                    &bundle.identifier,
                    &declarations,
                    crate::build::apple_opening::ApplePlatform::Macos,
                ),
                1,
            ),
        );
        std::fs::write(contents.join("Info.plist"), plist)?;

        let size_bytes = calculate_dir_size(&app_dir)?;
        crate::ui::debug(format!("macOS .app bundle staged at {}", app_dir.display()));

        Ok(FinalArtifacts {
            app_binary: app_dir,
            size_bytes,
        })
    }
}

/// Public bundle display names must also be safe single filesystem components.
#[cfg(target_os = "macos")]
fn validate_bundle_name(name: &str) -> BuildResult<()> {
    let mut components = Path::new(name).components();
    if name.contains(['/', '\\', '\0'])
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(BuildError::invalid_config(
            "bundle.name",
            "Application name must be a single nonempty filename component without separators, NUL, '.' or '..'",
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

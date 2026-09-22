// Only the macOS bundle staging below takes a `Path`; on other hosts the
// import would be unused and trip `-D warnings`.
#[cfg(target_os = "macos")]
// Only the macOS bundle staging below takes a `Path`; on other hosts the
// import would be unused and trip `-D warnings`.
#[cfg(target_os = "macos")]
use std::path::Path;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::build::util::cargo;

/// Builder for desktop platforms (Windows, macOS, Linux)
#[derive(Debug, Default)]
pub(crate) struct DesktopBuilder;

impl DesktopBuilder {
    /// Create a stateless desktop builder; each operation uses its `BuilderContext`.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }

    /// Detect the host target triple from `rustc -vV`.
    fn detect_host_target() -> BuildResult<String> {
        // Get host triple from rustc
        let output = std::process::Command::new("rustc")
            .args(["-vV"])
            .output()
            .map_err(|e| BuildError::CommandFailed {
                command: "rustc -vV".to_string(),
                exit_code: -1,
                stderr: e.to_string(),
            })?;

        let output_str = String::from_utf8_lossy(&output.stdout);

        let host = output_str
            .lines()
            .find(|line| line.starts_with("host:"))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string());

        if let Some(target) = host {
            return Ok(target);
        }

        // Fallback to common targets
        let fallback = if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin"
            } else {
                "x86_64-apple-darwin"
            }
        } else {
            "x86_64-unknown-linux-gnu"
        };

        let _ = crate::ui::warning(format!(
            "Could not detect host target from rustc, falling back to {fallback}"
        ));
        Ok(fallback.to_string())
    }
}

impl PlatformBuilder for DesktopBuilder {
    fn validate_environment(&self) -> BuildResult<()> {
        // Just need cargo
        crate::build::util::check_command_exists("cargo")?;
        Ok(())
    }

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let target = match &ctx.platform {
            crate::build::platform::Platform::Desktop { target } => match target {
                Some(t) => t.clone(),
                None => Self::detect_host_target()?,
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

        let mut args = vec!["build".to_string(), "--target".to_string(), target.clone()];
        let selected = cargo::select_target(&ctx.workspace_root, &ctx.target).await?;
        args.extend(selected.cargo_args());
        if let Some(profile_flag) = ctx.profile.cargo_flag() {
            args.push(profile_flag.to_string());
        }

        let executable = cargo::build_artifact(&ctx.workspace_root, &args, &selected).await?;

        Ok(BuildArtifacts {
            rust_libs: Vec::new(),
            executable: Some(executable),
            metadata: serde_json::json!({
                "target": target,
                "kind": "executable",
            }),
        })
    }

    async fn build_platform(
        &self,
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
            return Self::stage_macos_app(ctx, executable, bundle);
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

        Ok(FinalArtifacts {
            app_binary: output_binary,
            size_bytes,
        })
    }
}

impl DesktopBuilder {
    /// Stage a built executable into a minimal, well-formed macOS `.app`.
    ///
    /// The bundle is what makes the app a real application: `Info.plist` names
    /// it and declares `NSPrincipalClass`, and the executable lives at
    /// `Contents/MacOS/<name>`. The `just macos-*` probes build the same shape
    /// by hand; this is the reusable equivalent, so `flui build macos` yields a
    /// launchable app rather than a loose binary.
    ///
    /// `CFBundleExecutable` must equal the file placed in `Contents/MacOS`, so
    /// the caller's executable keeps its own file name there regardless of the
    /// bundle's display name.
    #[cfg(target_os = "macos")]
    fn stage_macos_app(
        ctx: &BuilderContext,
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
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#,
            name = xml_escape(&bundle.name),
            identifier = xml_escape(&bundle.identifier),
            executable = xml_escape(&exe_name),
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

/// Escape the five XML metacharacters in a plist `<string>` value.
///
/// `Info.plist` is XML: an app name like `R&D` interpolated raw
/// (`<string>R&D</string>`) is malformed, and Launch Services can reject or
/// misread the staged `.app`. Escaping the interpolated values keeps any
/// human-readable name valid; the `&` must be escaped first so the entities
/// this function introduces are not escaped a second time.
#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
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

#[cfg(all(test, target_os = "macos"))]
mod xml_escape_tests {
    use super::xml_escape;

    #[test]
    fn metacharacters_are_escaped_so_the_plist_stays_well_formed() {
        assert_eq!(xml_escape("R&D"), "R&amp;D");
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
    }

    #[test]
    fn the_ampersand_is_escaped_first_so_entities_are_not_double_escaped() {
        // If `&` were replaced after `<`, `&lt;` would become `&amp;lt;`.
        assert_eq!(xml_escape("<"), "&lt;");
        assert_eq!(xml_escape("&lt;"), "&amp;lt;");
    }

    #[test]
    fn ordinary_names_pass_through_unchanged() {
        assert_eq!(xml_escape("My Great App"), "My Great App");
        assert_eq!(xml_escape("com.example.app"), "com.example.app");
    }
}

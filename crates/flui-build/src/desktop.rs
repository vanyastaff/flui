use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{
    BuildArtifacts, BuildUnit, BuilderContext, FinalArtifacts, PlatformBuilder, private,
};
use crate::util::process;

/// Builder for desktop platforms (Windows, macOS, Linux)
#[derive(Debug)]
pub struct DesktopBuilder {
    workspace_root: PathBuf,
}

impl DesktopBuilder {
    /// Creates a new `DesktopBuilder`
    ///
    /// # Errors
    ///
    /// Currently infallible, but returns Result for consistency
    pub fn new(workspace_root: &Path) -> BuildResult<Self> {
        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
        })
    }

    /// Resolve the built executable path for `name` under a target/profile dir.
    ///
    /// Cargo writes `name` (plus `.exe` when `target` is a Windows triple)
    /// into `target/<triple>/<profile>/`.
    fn executable_path(&self, target: &str, profile: &str, name: &str) -> PathBuf {
        self.workspace_root
            .join("target")
            .join(target)
            .join(profile)
            .join(executable_file_name(target, name))
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

        tracing::warn!("Could not detect host target from rustc, falling back to {fallback}");
        Ok(fallback.to_string())
    }
}

impl private::Sealed for DesktopBuilder {}

impl PlatformBuilder for DesktopBuilder {
    fn platform_name(&self) -> &'static str {
        "desktop"
    }

    fn validate_environment(&self) -> BuildResult<()> {
        // Just need cargo
        crate::util::check_command_exists("cargo")?;
        Ok(())
    }

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let target = match &ctx.platform {
            crate::platform::Platform::Desktop { target } => match target {
                Some(t) => t.clone(),
                None => Self::detect_host_target()?,
            },
            _ => {
                return Err(BuildError::InvalidPlatform {
                    reason: "Expected Desktop platform".to_string(),
                });
            }
        };

        tracing::info!("Building desktop target '{target}' ({:?})", ctx.target);

        let mut args = vec!["build".to_string(), "--target".to_string(), target.clone()];
        args.extend(ctx.target.cargo_args());
        if let Some(profile_flag) = ctx.profile.cargo_flag() {
            args.push(profile_flag.to_string());
        }
        if !ctx.features.is_empty() {
            args.push("--features".to_string());
            args.push(ctx.features.join(","));
        }

        process::run_command_in_dir("cargo", &args, &ctx.workspace_root).await?;

        let executable = self.resolve_executable(&target, ctx)?;

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

        std::fs::create_dir_all(&ctx.output_dir)?;

        // macOS with bundle metadata stages a `.app`; every other case (and
        // macOS without metadata) copies the bare executable. A bundle is what
        // a foreground-activatable, double-clickable app needs; a bare Mach-O
        // launched from a terminal is not equivalent.
        #[cfg(target_os = "macos")]
        if let Some(bundle) = &ctx.bundle {
            return Self::stage_macos_app(ctx, executable, bundle);
        }

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

        tracing::info!("Desktop executable copied to: {:?}", output_binary);

        Ok(FinalArtifacts {
            app_binary: output_binary,
            size_bytes,
        })
    }

    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        // Note: We don't clean cargo target/ directory as it's shared
        tracing::info!("To clean Cargo build artifacts, run: cargo clean");

        Ok(())
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
        bundle: &crate::platform::AppBundle,
    ) -> BuildResult<FinalArtifacts> {
        let app_dir = ctx.output_dir.join(format!("{}.app", bundle.name));
        let contents = app_dir.join("Contents");
        let macos_dir = contents.join("MacOS");
        let resources_dir = contents.join("Resources");

        if app_dir.exists() {
            std::fs::remove_dir_all(&app_dir)?;
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
        tracing::info!(app = %app_dir.display(), "macOS .app bundle staged");

        Ok(FinalArtifacts {
            app_binary: app_dir,
            size_bytes,
        })
    }

    /// Locate the executable cargo just produced for `ctx.target`.
    fn resolve_executable(&self, target: &str, ctx: &BuilderContext) -> BuildResult<PathBuf> {
        let profile_dir = ctx.profile.as_str();

        let path = match &ctx.target {
            BuildUnit::Example(name) => self
                .workspace_root
                .join("target")
                .join(target)
                .join(profile_dir)
                .join("examples")
                .join(executable_file_name(target, name)),
            BuildUnit::Package(name) => {
                let bin = self
                    .binary_name_from_manifest(name)
                    .unwrap_or_else(|| name.clone());
                self.executable_path(target, profile_dir, &bin)
            }
            BuildUnit::DefaultBinary => {
                let name = Self::binary_name_from_manifest_at(&ctx.workspace_root)?;
                self.executable_path(target, profile_dir, &name)
            }
        };

        if !path.is_file() {
            return Err(BuildError::PathNotFound {
                path,
                context:
                    "Compiled desktop executable not found (does the target produce a binary?)"
                        .to_string(),
            });
        }
        Ok(path)
    }

    /// Resolve a workspace package's binary name from its own manifest.
    ///
    /// Walks `crates/*/Cargo.toml` under the workspace root and matches
    /// `[package].name == name`, then prefers an explicit `[[bin]].name` over
    /// the package name. Returns `None` when no matching manifest is found —
    /// the caller then falls back to cargo's package-name convention.
    fn binary_name_from_manifest(&self, name: &str) -> Option<String> {
        let crates_dir = self.workspace_root.join("crates");
        let entries = std::fs::read_dir(&crates_dir).ok()?;
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Ok(value) = text.parse::<toml::Value>() else {
                continue;
            };
            let package_name = value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(toml::Value::as_str);
            if package_name != Some(name) {
                continue;
            }
            if let Some(bins) = value.get("bin").and_then(toml::Value::as_array)
                && let Some(first) = bins.first()
                && let Some(bin_name) = first.get("name").and_then(toml::Value::as_str)
            {
                return Some(bin_name.to_string());
            }
            return Some(name.to_string());
        }
        None
    }

    /// Resolve the sole package's binary name from the manifest at `dir`.
    ///
    /// Prefers `[[bin]].name`, then `[package].name`. A generated application
    /// has one of the two; when neither parses, the directory's own name is
    /// cargo's documented fallback.
    fn binary_name_from_manifest_at(dir: &Path) -> BuildResult<String> {
        let manifest = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).map_err(|e| {
            BuildError::path_not_found(dir.to_path_buf(), format!("reading Cargo.toml: {e}"))
        })?;
        let value = text.parse::<toml::Value>().map_err(|e| {
            BuildError::invalid_config("Cargo.toml", format!("could not parse manifest: {e}"))
        })?;

        if let Some(bins) = value.get("bin").and_then(toml::Value::as_array)
            && let Some(first) = bins.first()
            && let Some(bin_name) = first.get("name").and_then(toml::Value::as_str)
        {
            return Ok(bin_name.to_string());
        }
        if let Some(package_name) = value
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
        {
            return Ok(package_name.to_string());
        }
        dir.file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or_else(|| {
                BuildError::invalid_config(
                    "workspace_root",
                    format!("{} has no usable directory name", dir.display()),
                )
            })
    }
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

/// The file name cargo emits for a binary, given the *requested* target
/// triple.
///
/// The suffix must come from `target`, never the host: a cross-build
/// (`flui build windows` on macOS or Linux) writes
/// `target/<windows-triple>/<profile>/<name>.exe`, and a Windows host building
/// a Linux target writes a bare `<name>`. Keying off `cfg!(target_os = ...)`
/// checks the CLI host and gets both directions wrong — the build succeeds but
/// the artifact lookup reports `PathNotFound`.
fn executable_file_name(target: &str, name: &str) -> String {
    if target.contains("windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::executable_file_name;

    #[test]
    fn a_windows_target_gets_an_exe_suffix_on_any_host() {
        // The regression: this used to key off `cfg!(target_os = "windows")`,
        // the CLI host. A Linux or macOS host cross-building
        // `x86_64-pc-windows-msvc` searched for a bare name and reported
        // `PathNotFound` after a successful compile.
        assert_eq!(
            executable_file_name("x86_64-pc-windows-msvc", "demo"),
            "demo.exe"
        );
        assert_eq!(
            executable_file_name("aarch64-pc-windows-msvc", "demo"),
            "demo.exe"
        );
    }

    #[test]
    fn a_non_windows_target_has_no_suffix_even_on_a_windows_host() {
        // The inverse direction: a Windows host targeting Linux/macOS must
        // not append `.exe`.
        assert_eq!(
            executable_file_name("x86_64-unknown-linux-gnu", "demo"),
            "demo"
        );
        assert_eq!(executable_file_name("aarch64-apple-darwin", "demo"), "demo");
    }
}

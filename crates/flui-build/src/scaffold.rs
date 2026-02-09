//! Platform scaffolding for new FLUI projects.
//!
//! Provides utilities to create platform-specific directory structures
//! using templates embedded at compile time via `include_str!()`.
//! This ensures `flui-cli` works correctly when installed standalone
//! via `cargo install` without access to the source workspace.

use crate::error::{BuildError, BuildResult};
use std::path::Path;

/// Valid platform names for FLUI projects.
const VALID_PLATFORMS: &[&str] = &["android", "ios", "web", "windows", "linux", "macos"];

// ── Embedded templates ──────────────────────────────────────────────────────

// Android
const ANDROID_MANIFEST: &str = include_str!("../templates/platforms/android/AndroidManifest.xml");
const ANDROID_BUILD_GRADLE: &str = include_str!("../templates/platforms/android/build.gradle.kts");
const ANDROID_ROOT_BUILD_GRADLE: &str =
    include_str!("../templates/platforms/android/root.build.gradle.kts");
const ANDROID_SETTINGS_GRADLE: &str =
    include_str!("../templates/platforms/android/settings.gradle.kts");
const ANDROID_GRADLE_PROPERTIES: &str =
    include_str!("../templates/platforms/android/gradle.properties");
const ANDROID_GITIGNORE: &str = include_str!("../templates/platforms/android/.gitignore");

// iOS
const IOS_INFO_PLIST: &str = include_str!("../templates/platforms/ios/Runner/Info.plist");
const IOS_GITIGNORE: &str = include_str!("../templates/platforms/ios/.gitignore");

// Web
const WEB_INDEX_HTML: &str = include_str!("../templates/platforms/web/index.html");
const WEB_MANIFEST_JSON: &str = include_str!("../templates/platforms/web/manifest.json");

// Desktop
const WINDOWS_GITIGNORE: &str = include_str!("../templates/platforms/windows/.gitignore");
const LINUX_GITIGNORE: &str = include_str!("../templates/platforms/linux/.gitignore");
const MACOS_GITIGNORE: &str = include_str!("../templates/platforms/macos/.gitignore");

// ── Template entry ──────────────────────────────────────────────────────────

/// A single file to write during scaffolding.
struct TemplateFile {
    /// Relative path from the platform directory root.
    rel_path: &'static str,
    /// Raw template content (may contain `{{placeholders}}`).
    content: &'static str,
}

/// Returns the list of template files for a given platform.
fn platform_templates(platform: &str) -> &'static [TemplateFile] {
    match platform {
        "android" => &[
            TemplateFile {
                rel_path: "app/src/main/AndroidManifest.xml",
                content: ANDROID_MANIFEST,
            },
            TemplateFile {
                rel_path: "app/build.gradle.kts",
                content: ANDROID_BUILD_GRADLE,
            },
            TemplateFile {
                rel_path: "build.gradle.kts",
                content: ANDROID_ROOT_BUILD_GRADLE,
            },
            TemplateFile {
                rel_path: "settings.gradle.kts",
                content: ANDROID_SETTINGS_GRADLE,
            },
            TemplateFile {
                rel_path: "gradle.properties",
                content: ANDROID_GRADLE_PROPERTIES,
            },
            TemplateFile {
                rel_path: ".gitignore",
                content: ANDROID_GITIGNORE,
            },
        ],
        "ios" => &[
            TemplateFile {
                rel_path: "Runner/Info.plist",
                content: IOS_INFO_PLIST,
            },
            TemplateFile {
                rel_path: ".gitignore",
                content: IOS_GITIGNORE,
            },
        ],
        "web" => &[
            TemplateFile {
                rel_path: "index.html",
                content: WEB_INDEX_HTML,
            },
            TemplateFile {
                rel_path: "manifest.json",
                content: WEB_MANIFEST_JSON,
            },
        ],
        "windows" => &[TemplateFile {
            rel_path: ".gitignore",
            content: WINDOWS_GITIGNORE,
        }],
        "linux" => &[TemplateFile {
            rel_path: ".gitignore",
            content: LINUX_GITIGNORE,
        }],
        "macos" => &[TemplateFile {
            rel_path: ".gitignore",
            content: MACOS_GITIGNORE,
        }],
        _ => &[],
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Returns the list of valid platform names.
#[must_use]
pub fn valid_platform_names() -> &'static [&'static str] {
    VALID_PLATFORMS
}

/// Check if a platform name is valid.
#[must_use]
pub fn is_valid_platform(name: &str) -> bool {
    VALID_PLATFORMS.contains(&name.to_lowercase().as_str())
}

/// Parameters for template placeholder substitution.
#[derive(Debug)]
pub struct ScaffoldParams<'a> {
    /// Application display name (e.g. "My App").
    pub app_name: &'a str,
    /// Rust library crate name / native lib name (e.g. "`my_app`").
    pub lib_name: &'a str,
    /// Reverse-domain package name (e.g. "`com.example.my_app`").
    pub package_name: &'a str,
}

/// Scaffold platform-specific directories and config files.
///
/// Creates the `platforms/<name>/` directory tree inside `project_dir`
/// using embedded templates. Placeholder values (`{{app_name}}`,
/// `{{lib_name}}`, `{{package_name}}`) are substituted from `params`.
///
/// # Errors
///
/// Returns an error if the platform name is invalid or filesystem operations fail.
pub fn scaffold_platform(
    platform: &str,
    project_dir: &Path,
    params: &ScaffoldParams<'_>,
) -> BuildResult<()> {
    let platform_lower = platform.to_lowercase();

    if !is_valid_platform(&platform_lower) {
        return Err(BuildError::invalid_platform(format!(
            "Invalid platform '{}'. Valid platforms: {}",
            platform,
            VALID_PLATFORMS.join(", ")
        )));
    }

    let dest_dir = project_dir.join("platforms").join(&platform_lower);

    let templates = platform_templates(&platform_lower);
    if templates.is_empty() {
        // Just create the directory (shouldn't happen for known platforms).
        std::fs::create_dir_all(&dest_dir).map_err(|e| {
            BuildError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to create platform directory '{}': {}",
                    dest_dir.display(),
                    e
                ),
            ))
        })?;
        return Ok(());
    }

    for tmpl in templates {
        let file_path = dest_dir.join(tmpl.rel_path);

        // Ensure parent directory exists.
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                BuildError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create directory '{}': {}", parent.display(), e),
                ))
            })?;
        }

        // Substitute placeholders and write.
        let rendered = substitute(tmpl.content, params);
        std::fs::write(&file_path, rendered).map_err(|e| {
            BuildError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to write '{}': {}", file_path.display(), e),
            ))
        })?;
    }

    tracing::debug!(
        platform = %platform_lower,
        files = templates.len(),
        dest = %dest_dir.display(),
        "Scaffolded platform directory"
    );

    Ok(())
}

// ── Placeholder substitution ────────────────────────────────────────────────

/// Replace `{{app_name}}`, `{{lib_name}}`, and `{{package_name}}` in a template string.
fn substitute(template: &str, params: &ScaffoldParams<'_>) -> String {
    template
        .replace("{{app_name}}", params.app_name)
        .replace("{{lib_name}}", params.lib_name)
        .replace("{{package_name}}", params.package_name)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_params() -> ScaffoldParams<'static> {
        ScaffoldParams {
            app_name: "Test App",
            lib_name: "test_app",
            package_name: "com.example.test_app",
        }
    }

    #[test]
    fn test_valid_platform_names() {
        let names = valid_platform_names();
        assert!(names.contains(&"android"));
        assert!(names.contains(&"ios"));
        assert!(names.contains(&"web"));
        assert!(names.contains(&"windows"));
        assert!(names.contains(&"linux"));
        assert!(names.contains(&"macos"));
    }

    #[test]
    fn test_is_valid_platform() {
        assert!(is_valid_platform("android"));
        assert!(is_valid_platform("Android"));
        assert!(is_valid_platform("IOS"));
        assert!(!is_valid_platform("wasm"));
        assert!(!is_valid_platform(""));
    }

    #[test]
    fn test_substitute() {
        let params = test_params();
        let result = substitute(
            "name={{app_name}}, lib={{lib_name}}, pkg={{package_name}}",
            &params,
        );
        assert_eq!(
            result,
            "name=Test App, lib=test_app, pkg=com.example.test_app"
        );
    }

    #[test]
    fn test_substitute_no_placeholders() {
        let params = test_params();
        let result = substitute("no placeholders here", &params);
        assert_eq!(result, "no placeholders here");
    }

    #[test]
    fn test_platform_templates_android() {
        let templates = platform_templates("android");
        assert_eq!(templates.len(), 6);
        assert!(templates
            .iter()
            .any(|t| t.rel_path == "app/src/main/AndroidManifest.xml"));
        assert!(templates
            .iter()
            .any(|t| t.rel_path == "app/build.gradle.kts"));
        assert!(templates
            .iter()
            .any(|t| t.rel_path == "settings.gradle.kts"));
    }

    #[test]
    fn test_platform_templates_ios() {
        let templates = platform_templates("ios");
        assert_eq!(templates.len(), 2);
        assert!(templates.iter().any(|t| t.rel_path == "Runner/Info.plist"));
    }

    #[test]
    fn test_platform_templates_web() {
        let templates = platform_templates("web");
        assert_eq!(templates.len(), 2);
        assert!(templates.iter().any(|t| t.rel_path == "index.html"));
        assert!(templates.iter().any(|t| t.rel_path == "manifest.json"));
    }

    #[test]
    fn test_platform_templates_desktop() {
        for platform in &["windows", "linux", "macos"] {
            let templates = platform_templates(platform);
            assert_eq!(
                templates.len(),
                1,
                "Desktop platform '{}' should have 1 template",
                platform
            );
            assert_eq!(templates[0].rel_path, ".gitignore");
        }
    }

    #[test]
    fn test_scaffold_invalid_platform() {
        let dir = PathBuf::from("/tmp/flui-test-invalid");
        let params = test_params();
        let result = scaffold_platform("fuchsia", &dir, &params);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid platform"));
    }

    #[test]
    fn test_scaffold_android() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let params = test_params();

        scaffold_platform("android", dir.path(), &params)
            .expect("scaffold_platform should succeed");

        let manifest = dir
            .path()
            .join("platforms/android/app/src/main/AndroidManifest.xml");
        assert!(manifest.exists(), "AndroidManifest.xml should exist");
        let content = std::fs::read_to_string(&manifest).expect("read manifest");
        assert!(
            content.contains("Test App"),
            "app_name should be substituted"
        );
        assert!(
            content.contains("test_app"),
            "lib_name should be substituted"
        );

        let settings = dir.path().join("platforms/android/settings.gradle.kts");
        assert!(settings.exists(), "settings.gradle.kts should exist");
        let content = std::fs::read_to_string(&settings).expect("read settings");
        assert!(
            content.contains("Test App"),
            "app_name in settings.gradle.kts"
        );
    }

    #[test]
    fn test_scaffold_web() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let params = test_params();

        scaffold_platform("web", dir.path(), &params).expect("scaffold_platform should succeed");

        let index = dir.path().join("platforms/web/index.html");
        assert!(index.exists(), "index.html should exist");
        let content = std::fs::read_to_string(&index).expect("read index.html");
        assert!(content.contains("Test App"), "app_name in index.html");

        let manifest = dir.path().join("platforms/web/manifest.json");
        assert!(manifest.exists(), "manifest.json should exist");
    }

    #[test]
    fn test_scaffold_ios() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let params = test_params();

        scaffold_platform("ios", dir.path(), &params).expect("scaffold_platform should succeed");

        let plist = dir.path().join("platforms/ios/Runner/Info.plist");
        assert!(plist.exists(), "Info.plist should exist");
        let content = std::fs::read_to_string(&plist).expect("read Info.plist");
        assert!(content.contains("Test App"), "app_name in Info.plist");
    }
}

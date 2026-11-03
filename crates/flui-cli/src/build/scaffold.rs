//! Platform scaffolding for new FLUI projects.
//!
//! Provides utilities to create platform-specific directory structures
//! using templates embedded at compile time via `include_str!()`.
//! This ensures `flui-cli` works correctly when installed standalone
//! via `cargo install` without access to the source workspace.

use crate::build::error::{BuildError, BuildResult};
use std::path::Path;

/// Valid platform names for FLUI projects.
const VALID_PLATFORMS: &[&str] = &["android", "ios", "web", "windows", "linux", "macos"];

// ── Embedded templates ──────────────────────────────────────────────────────

// Android
const ANDROID_BUILD_GRADLE: &str =
    include_str!("../../templates/platforms/android/build.gradle.kts");
const ANDROID_ROOT_BUILD_GRADLE: &str =
    include_str!("../../templates/platforms/android/root.build.gradle.kts");
const ANDROID_SETTINGS_GRADLE: &str =
    include_str!("../../templates/platforms/android/settings.gradle.kts");
const ANDROID_GRADLE_PROPERTIES: &str =
    include_str!("../../templates/platforms/android/gradle.properties");
const ANDROID_GITIGNORE: &str = include_str!("../../templates/platforms/android/.gitignore");
const ANDROID_PROGUARD_RULES: &str =
    include_str!("../../templates/platforms/android/proguard-rules.pro");

// iOS
const IOS_README: &str = include_str!("../../templates/platforms/ios/README.md");
const IOS_GITIGNORE: &str = include_str!("../../templates/platforms/ios/.gitignore");

// Web
const WEB_INDEX_HTML: &str = include_str!("../../templates/platforms/web/index.html");
const WEB_MANIFEST_JSON: &str = include_str!("../../templates/platforms/web/manifest.json");

// Desktop
const WINDOWS_GITIGNORE: &str = include_str!("../../templates/platforms/windows/.gitignore");
const LINUX_GITIGNORE: &str = include_str!("../../templates/platforms/linux/.gitignore");
const MACOS_GITIGNORE: &str = include_str!("../../templates/platforms/macos/.gitignore");

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
                rel_path: "app/build.gradle.kts",
                content: ANDROID_BUILD_GRADLE,
            },
            TemplateFile {
                rel_path: "app/proguard-rules.pro",
                content: ANDROID_PROGUARD_RULES,
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
                rel_path: "README.md",
                content: IOS_README,
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
pub(crate) fn valid_platform_names() -> &'static [&'static str] {
    VALID_PLATFORMS
}

/// Check if a platform name is valid.
#[must_use]
pub(crate) fn is_valid_platform(name: &str) -> bool {
    VALID_PLATFORMS.contains(&name.to_lowercase().as_str())
}

/// Parameters for template placeholder substitution.
#[derive(Debug)]
pub(crate) struct ScaffoldParams<'a> {
    /// Application display name (e.g. "My App").
    pub(crate) app: &'a str,
    /// Rust library crate name / native lib name (e.g. "`my_app`").
    pub(crate) lib: &'a str,
    /// Reverse-domain package name (e.g. "`com.example.my_app`").
    pub(crate) package: &'a str,
}

/// One file [`scaffold_platform`] would write, rendered but not yet on
/// disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaffoldFile {
    /// Path relative to the platform's destination directory
    /// (`platforms/<name>/`), e.g. `app/build.gradle.kts`.
    pub(crate) rel_path: std::path::PathBuf,
    /// The file's contents, with placeholders already substituted.
    pub(crate) contents: String,
}

/// Render the files [`scaffold_platform`] would write for `platform`,
/// without touching the file system.
///
/// This is the shape a preview (`flui create --dry-run`) needs: the exact
/// relative paths and contents a real scaffold would produce, so the
/// preview and the real run can never disagree about what platform
/// scaffolding creates.
///
/// # Errors
///
/// Returns an error if the platform name is invalid.
pub(crate) fn scaffold_platform_plan(
    platform: &str,
    params: &ScaffoldParams<'_>,
) -> BuildResult<Vec<ScaffoldFile>> {
    let platform_lower = platform.to_lowercase();

    if !is_valid_platform(&platform_lower) {
        return Err(BuildError::invalid_platform(format!(
            "invalid platform '{}'; valid platforms: {}",
            platform,
            VALID_PLATFORMS.join(", ")
        )));
    }

    let mut files: Vec<ScaffoldFile> = platform_templates(&platform_lower)
        .iter()
        .map(|tmpl| ScaffoldFile {
            rel_path: std::path::PathBuf::from(tmpl.rel_path),
            contents: substitute(tmpl.content, params),
        })
        .collect();
    if platform_lower == "android" {
        // The manifest every build generates, here before the first build
        // knows what the linked crates declare.
        files.insert(
            0,
            ScaffoldFile {
                rel_path: std::path::PathBuf::from("app/src/main/AndroidManifest.xml"),
                contents: crate::build::android_manifest::render(
                    &crate::build::android_manifest::ManifestFacts {
                        label: params.app,
                        lib_name: params.lib,
                    },
                    &crate::declarations::Declarations::default(),
                ),
            },
        );
    }
    Ok(files)
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
pub(crate) fn scaffold_platform(
    platform: &str,
    project_dir: &Path,
    params: &ScaffoldParams<'_>,
) -> BuildResult<()> {
    let platform_lower = platform.to_lowercase();
    let dest_dir = project_dir.join("platforms").join(&platform_lower);
    let files = scaffold_platform_plan(&platform_lower, params)?;

    if files.is_empty() {
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

    for file in &files {
        let file_path = dest_dir.join(&file.rel_path);

        // Ensure parent directory exists.
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                BuildError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create directory '{}': {}", parent.display(), e),
                ))
            })?;
        }

        std::fs::write(&file_path, &file.contents).map_err(|e| {
            BuildError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to write '{}': {}", file_path.display(), e),
            ))
        })?;
    }

    crate::ui::debug(format!(
        "scaffolded {} file(s) for {platform_lower} into {}",
        files.len(),
        dest_dir.display()
    ));

    Ok(())
}

// ── Placeholder substitution ────────────────────────────────────────────────

/// Replace `{{app_name}}`, `{{lib_name}}`, and `{{package_name}}` in a template string.
fn substitute(template: &str, params: &ScaffoldParams<'_>) -> String {
    template
        .replace("{{app_name}}", params.app)
        .replace("{{lib_name}}", params.lib)
        .replace("{{package_name}}", params.package)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> ScaffoldParams<'static> {
        ScaffoldParams {
            app: "Test App",
            lib: "test_app",
            package: "com.example.test_app",
        }
    }

    #[test]
    fn test_scaffold_platform_plan_matches_scaffold_platform() {
        let params = test_params();

        let planned = scaffold_platform_plan("android", &params).expect("plan android");
        assert_eq!(planned.len(), 7);

        let dir = tempfile::tempdir().expect("failed to create temp dir");
        scaffold_platform("android", dir.path(), &params).expect("scaffold android");

        for file in &planned {
            let written =
                std::fs::read_to_string(dir.path().join("platforms/android").join(&file.rel_path))
                    .unwrap_or_else(|e| panic!("read {}: {e}", file.rel_path.display()));
            assert_eq!(
                written,
                file.contents,
                "plan and real write disagree for {}",
                file.rel_path.display()
            );
        }
    }

    /// The Gradle script and rules `flui create` scaffolds for Android,
    /// whole: what it reads from gradle.properties shows up here.
    #[test]
    fn the_scaffolded_gradle_script_matches_its_golden() {
        let planned = scaffold_platform_plan("android", &test_params()).expect("plan android");
        let file = |path: &str| {
            planned
                .iter()
                .find(|file| file.rel_path == std::path::Path::new(path))
                .unwrap_or_else(|| panic!("{path} is scaffolded"))
                .contents
                .clone()
        };
        insta::assert_snapshot!("app_build_gradle_kts", file("app/build.gradle.kts"));
        insta::assert_snapshot!("app_proguard_rules_pro", file("app/proguard-rules.pro"));
    }
}

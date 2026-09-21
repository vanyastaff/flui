//! Project template generation.
//!
//! This module provides template generation for new FLUI projects using
//! the builder pattern for flexible configuration.
//!
//! # Examples
//!
//! ```ignore
//! use flui_cli::templates::TemplateBuilder;
//! use flui_cli::types::{ProjectName, OrganizationId};
//! use flui_cli::Template;
//!
//! let name = ProjectName::new("my-app")?;
//! let org = OrganizationId::new("com.example")?;
//!
//! TemplateBuilder::new(name, org)
//!     .template(Template::Counter)
//!     .with_git(true)
//!     .generate(&project_dir)?;
//! ```

mod basic;
mod counter;
mod empty;
mod hot_reload;
mod plan;
mod source;
mod widget;

pub use plan::{PlannedFile, ProjectPlan};
pub use source::DependencySource;

use crate::Template;
use crate::error::CliResult;
use crate::types::{OrganizationId, ProjectName};
use flui_build::scaffold::{ScaffoldParams, scaffold_platform_plan};
use std::path::Path;

/// `.gitignore` for a generated project. Part of the plan (not a
/// side-effect of `git init`) so `--dry-run` lists it and a real run and a
/// dry run always agree on the file set.
const GITIGNORE_TEMPLATE: &str = r"# Build artifacts
/target
/build

# Platform-specific
platforms/android/app/build/
platforms/android/.gradle/
platforms/web/pkg/
platforms/ios/build/

# IDE
.vscode/
.idea/
*.swp
*.swo
*.iml

# OS
.DS_Store
Thumbs.db

# FLUI
flui.lock

# Rust
*.pdb
Cargo.lock
";

/// Builder for generating FLUI project templates.
///
/// Uses the builder pattern (C-BUILDER from Rust API Guidelines) for flexible
/// configuration of project generation.
///
/// # Builder Methods
///
/// - [`template`](Self::template) - Set the template type (default: Counter)
/// - [`with_git`](Self::with_git) - Enable/disable git initialization (default: true)
/// - [`with_cargo_check`](Self::with_cargo_check) - Enable/disable cargo check (default: true)
///
/// # Examples
///
/// ```ignore
/// let project = TemplateBuilder::new(name, org)
///     .template(Template::Basic)
///     .with_git(false)
///     .generate(&dir)?;
///
/// println!("Created project at: {}", project.path.display());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBuilder {
    name: ProjectName,
    org: OrganizationId,
    template: Template,
    init_git: bool,
    run_cargo_check: bool,
    source: DependencySource,
    platforms: Vec<String>,
    hot_reload: bool,
}

impl TemplateBuilder {
    /// Create a new template builder with required parameters.
    ///
    /// # Arguments
    ///
    /// * `name` - Validated project name
    /// * `org` - Validated organization ID
    pub fn new(name: ProjectName, org: OrganizationId) -> Self {
        Self {
            name,
            org,
            template: Template::Counter,
            init_git: true,
            run_cargo_check: true,
            source: DependencySource::Registry,
            platforms: Vec::new(),
            hot_reload: false,
        }
    }

    /// Set the template type.
    ///
    /// Default is [`Template::Counter`].
    pub fn template(mut self, template: Template) -> Self {
        self.template = template;
        self
    }

    /// Configure whether to initialize a git repository.
    ///
    /// Default is `true`.
    pub fn with_git(mut self, init: bool) -> Self {
        self.init_git = init;
        self
    }

    /// Select validated framework dependencies (default: crates.io).
    pub fn dependency_source(mut self, source: DependencySource) -> Self {
        self.source = source;
        self
    }

    /// Set the target platforms for scaffolding.
    pub fn platforms(mut self, platforms: Vec<String>) -> Self {
        self.platforms = platforms;
        self
    }

    /// Configure whether to run cargo check after generation.
    ///
    /// Default is `true`.
    pub fn with_cargo_check(mut self, check: bool) -> Self {
        self.run_cargo_check = check;
        self
    }

    /// Generate the Flutter-parity hot-reload workspace (host/worker/types)
    /// instead of a single-crate project.
    ///
    /// Takes precedence over [`template`](Self::template): the hot-reload
    /// layout is a workspace shape, not one of the single-crate templates.
    pub fn hot_reload(mut self, hot_reload: bool) -> Self {
        self.hot_reload = hot_reload;
        self
    }

    /// Build the in-memory plan this configuration produces, without
    /// touching the file system.
    ///
    /// This is what `--dry-run` inspects, and what [`generate`](Self::generate)
    /// writes: the same plan either way, so a dry run shows exactly what a
    /// real run would create — including `.gitignore` and, for a
    /// non-library, non-hot-reload template, the platform scaffolding
    /// (`platforms/<name>/…`) that used to be written directly by
    /// `flui-build` outside the plan. The template match is exhaustive on
    /// purpose — a new [`Template`] variant must get its own generator
    /// here, not fall back to another template's files.
    ///
    /// Platform names in `self.platforms` are assumed valid: they come
    /// from the `Platform` clap enum, whose `Display` output is exactly
    /// `flui_build::scaffold`'s `VALID_PLATFORMS` list, so
    /// `scaffold_platform_plan` never actually sees an unknown name here.
    pub fn plan(&self) -> ProjectPlan {
        let name_str = self.name.as_str();
        let org_str = self.org.as_str();

        let plan = if self.hot_reload {
            hot_reload::generate(name_str, org_str, &self.source)
        } else {
            match self.template {
                Template::Counter => {
                    counter::generate(name_str, org_str, &self.source, &self.platforms)
                }
                Template::Basic => {
                    basic::generate(name_str, org_str, &self.source, &self.platforms)
                }
                Template::Empty => {
                    empty::generate(name_str, org_str, &self.source, &self.platforms)
                }
                Template::Widget => widget::generate(name_str, org_str, &self.source),
            }
        };

        let plan = plan.file(".gitignore", GITIGNORE_TEMPLATE);

        if self.hot_reload || self.template.is_library() {
            return plan;
        }

        self.plan_platform_scaffolds(plan)
    }

    /// Append the rendered platform-scaffold files (`platforms/<name>/…`)
    /// for every platform in `self.platforms` to `plan`, so they are
    /// written by [`ProjectPlan::write`] exactly like every other planned
    /// file, and listed by `--dry-run` exactly like every other planned
    /// file.
    fn plan_platform_scaffolds(&self, mut plan: ProjectPlan) -> ProjectPlan {
        if self.platforms.is_empty() {
            return plan;
        }

        let lib_name = self.name.as_str().replace('-', "_");
        let package_name = format!("{}.{lib_name}", self.org.as_str());
        let params = ScaffoldParams {
            app_name: self.name.as_str(),
            lib_name: &lib_name,
            package_name: &package_name,
        };

        for platform in &self.platforms {
            let files = scaffold_platform_plan(platform, &params).unwrap_or_else(|error| {
                // See the invariant documented on `plan`: a `Platform` clap
                // value always names a platform `flui-build` knows.
                unreachable!("platform '{platform}' from the validated Platform enum: {error}")
            });
            for file in files {
                plan = plan.file(
                    Path::new("platforms").join(platform).join(file.rel_path),
                    file.contents,
                );
            }
        }

        plan
    }

    /// Generate the project from the template.
    ///
    /// This is the terminal method that consumes the builder and writes
    /// the plan — templates, `.gitignore`, and platform scaffolding alike
    /// — to disk. It is a thin wrapper over [`plan`](Self::plan) and
    /// [`ProjectPlan::write`] on purpose: the only way to change what a
    /// real run produces is to change the plan, which is exactly what
    /// `--dry-run` previews.
    ///
    /// # Errors
    ///
    /// Returns an error if template files cannot be written or file system
    /// operations fail.
    pub fn generate(self, dir: &Path) -> CliResult<GeneratedProject> {
        let plan = self.plan();
        plan.write(dir)?;

        Ok(GeneratedProject {
            name: self.name,
            org: self.org,
            template: self.template,
            path: dir.to_path_buf(),
            git_initialized: self.init_git,
            plan,
        })
    }
}

/// Result of successful project generation.
///
/// Contains information about the generated project that can be used
/// for further operations or user feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedProject {
    /// The project name.
    pub name: ProjectName,
    /// The organization ID.
    pub org: OrganizationId,
    /// The template used.
    pub template: Template,
    /// Path where the project was created.
    pub path: std::path::PathBuf,
    /// Whether git was initialized.
    pub git_initialized: bool,
    /// The plan that was written — every file and directory this run
    /// created, in generation order.
    pub plan: ProjectPlan,
}

#[expect(
    dead_code,
    reason = "method reserved for future post-generation reporting"
)]
impl GeneratedProject {
    /// Get the full application ID (e.g., "`com.example.my_app`").
    pub fn app_id(&self) -> String {
        self.org.app_id(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template is pure string formatting over its inputs — no
    /// timestamps, no random ids, no absolute paths (other than a validated
    /// `--local` checkout, which is not exercised here). Two plans built
    /// from identical inputs must therefore be byte-identical, which is what
    /// makes `--dry-run` a trustworthy preview of a real run.
    #[test]
    fn plan_is_deterministic_for_every_template() {
        let templates = [
            Template::Basic,
            Template::Counter,
            Template::Empty,
            Template::Widget,
        ];
        for template in templates {
            let build = || {
                TemplateBuilder::new(
                    ProjectName::new("determinism-check").expect("valid name"),
                    OrganizationId::new("com.example").expect("valid org"),
                )
                .template(template)
                .platforms(vec!["linux".to_string(), "web".to_string()])
                .plan()
            };
            assert_eq!(build(), build(), "{template} plan is not deterministic");
        }
    }

    #[test]
    fn hot_reload_plan_is_deterministic() {
        let build = || {
            TemplateBuilder::new(
                ProjectName::new("determinism-reload").expect("valid name"),
                OrganizationId::new("com.example").expect("valid org"),
            )
            .hot_reload(true)
            .plan()
        };
        assert_eq!(build(), build());
    }
}

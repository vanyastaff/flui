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

use crate::error::CliResult;
use crate::types::{OrganizationId, ProjectName};
use crate::Template;
use std::path::Path;

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

    /// Configure whether to run cargo check after generation.
    ///
    /// Default is `true`.
    #[allow(dead_code)]
    pub fn with_cargo_check(mut self, check: bool) -> Self {
        self.run_cargo_check = check;
        self
    }

    /// Generate the project from the template.
    ///
    /// This is the terminal method that consumes the builder and creates
    /// the project files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Template files cannot be written
    /// - File system operations fail
    pub fn generate(self, dir: &Path) -> CliResult<GeneratedProject> {
        let name_str = self.name.as_str();
        let org_str = self.org.as_str();

        match self.template {
            Template::Counter => counter::generate(dir, name_str, org_str)?,
            Template::Basic => basic::generate(dir, name_str, org_str)?,
            // TODO: Implement specific templates
            Template::Todo => basic::generate(dir, name_str, org_str)?,
            Template::Dashboard => basic::generate(dir, name_str, org_str)?,
            Template::Widget => basic::generate(dir, name_str, org_str)?,
            Template::Plugin => basic::generate(dir, name_str, org_str)?,
            Template::Empty => basic::generate(dir, name_str, org_str)?,
        }

        Ok(GeneratedProject {
            name: self.name,
            org: self.org,
            template: self.template,
            path: dir.to_path_buf(),
            git_initialized: self.init_git,
        })
    }
}

/// Result of successful project generation.
///
/// Contains information about the generated project that can be used
/// for further operations or user feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
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
}

#[allow(dead_code)]
impl GeneratedProject {
    /// Get the full application ID (e.g., "com.example.my_app").
    pub fn app_id(&self) -> String {
        self.org.app_id(&self.name)
    }
}

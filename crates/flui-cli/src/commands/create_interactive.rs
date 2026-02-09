//! Interactive project creation command.
//!
//! This module provides an interactive CLI wizard for creating new FLUI projects
//! using cliclack for beautiful prompts.

use crate::error::{CliError, CliResult};
use crate::runner::{input, select};
use crate::types::{OrganizationId, ProjectName};
use crate::{Platform, Template};
use console::style;

/// Configuration collected from interactive prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Validated project name.
    pub name: ProjectName,
    /// Validated organization ID.
    pub org: OrganizationId,
    /// Selected project template.
    pub template: Template,
    /// Selected target platforms (None means use defaults).
    pub platforms: Option<Vec<Platform>>,
}

/// Run the interactive project creation wizard.
///
/// # Errors
///
/// Returns an error if:
/// - User input is invalid
/// - User cancels the operation
/// - Dialog interaction fails
pub fn interactive_create() -> CliResult<ProjectConfig> {
    cliclack::intro(style(" Create FLUI Project ").on_cyan().black())?;

    // Ask for project name with validation
    let name: String = input("Project name")
        .placeholder("my-app")
        .validate(|input: &String| {
            ProjectName::new(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact()
        .map_err(|_| CliError::UserCancelled)?;

    // Ask for organization with validation
    let org: String = input("Organization (reverse domain notation)")
        .default_input("com.example")
        .validate(|input: &String| {
            OrganizationId::new(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact()
        .map_err(|_| CliError::UserCancelled)?;

    // Ask for template
    let template = select("Choose a template")
        .item(
            Template::Counter,
            "Counter",
            "Simple counter with state management",
        )
        .item(Template::Basic, "Basic", "Minimal FLUI application")
        .item(Template::Todo, "Todo", "Todo list app (coming soon)")
        .item(
            Template::Dashboard,
            "Dashboard",
            "Dashboard UI (coming soon)",
        )
        .item(Template::Widget, "Widget", "Reusable widget package")
        .item(Template::Plugin, "Plugin", "Plugin for extending FLUI")
        .item(Template::Empty, "Empty", "Empty project with essentials")
        .interact()
        .map_err(|_| CliError::UserCancelled)?;

    // Ask for target platforms
    let platforms: Vec<Platform> = cliclack::multiselect("Select target platforms")
        .item(Platform::Windows, "Windows", "Desktop")
        .item(Platform::Linux, "Linux", "Desktop")
        .item(Platform::Macos, "macOS", "Desktop")
        .item(Platform::Android, "Android", "Mobile")
        .item(Platform::Ios, "iOS", "Mobile (macOS only)")
        .item(Platform::Web, "Web", "WASM")
        .required(false)
        .interact()
        .map_err(|_| CliError::UserCancelled)?;

    let platforms = if platforms.is_empty() {
        None
    } else {
        Some(platforms)
    };

    // Validation already happened in the input prompts above,
    // so these constructions are guaranteed to succeed.
    let name = ProjectName::new(&name)?;
    let org = OrganizationId::new(&org)?;

    Ok(ProjectConfig {
        name,
        org,
        template,
        platforms,
    })
}

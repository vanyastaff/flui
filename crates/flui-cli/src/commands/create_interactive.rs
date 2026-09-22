//! Interactive project creation command.
//!
//! This module provides an interactive CLI wizard for creating new FLUI projects
//! over the prompts in `ui::prompt`.

use crate::error::{CliError, CliResult};
use crate::types::{OrganizationId, ProjectName};
use crate::ui;
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
    ui::intro(style(" Create FLUI Project ").on_cyan().black())?;

    // Ask for project name with validation
    let name = ui::prompt::input("Project name", None, |input| {
        ProjectName::new(input)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?
    .ok_or(CliError::UserCancelled)?;

    // Ask for organization with validation
    let org = ui::prompt::input(
        "Organization (reverse domain notation)",
        Some("com.example"),
        |input| {
            OrganizationId::new(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    )?
    .ok_or(CliError::UserCancelled)?;

    // Ask for template
    let template = ui::prompt::select(
        "Choose a template",
        &[
            (
                Template::Counter,
                "Counter",
                "Simple counter with state management",
            ),
            (
                Template::Basic,
                "Basic",
                "Hello, FLUI! with a Material theme",
            ),
            (Template::Empty, "Empty", "Smallest runnable app"),
            (Template::Widget, "Widget", "Reusable widget library"),
        ],
    )?
    .ok_or(CliError::UserCancelled)?;

    // Ask for target platforms
    let platforms: Vec<Platform> = ui::prompt::multiselect(
        "Select target platforms",
        &[
            (Platform::Windows, "Windows", "Desktop"),
            (Platform::Linux, "Linux", "Desktop"),
            (Platform::Macos, "macOS", "Desktop"),
            (Platform::Android, "Android", "Mobile"),
            (Platform::Ios, "iOS", "Mobile (macOS only)"),
            (Platform::Web, "Web", "WASM"),
        ],
    )?
    .ok_or(CliError::UserCancelled)?;

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

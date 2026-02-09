//! Project creation command.
//!
//! This module handles the `flui create` command for generating new FLUI projects.

use crate::error::{CliResult, ResultExt};
use crate::runner::{GitCommand, OutputStyle};
use crate::templates::TemplateBuilder;
use crate::types::{OrganizationId, ProjectName, ProjectPath};
use crate::{Platform, Template};
use console::style;
use std::path::{Path, PathBuf};

/// Execute the create command.
///
/// # Arguments
///
/// * `project_name` - Validated project name
/// * `org_id` - Validated organization ID
/// * `template` - Template to use for project generation
/// * `_platforms` - Target platforms (reserved for future use)
/// * `path` - Optional custom output directory
/// * `_is_lib` - Whether to create a library (reserved for future use)
///
/// # Errors
///
/// Returns an error if:
/// - Directory already exists
/// - Template generation fails
/// - Git initialization fails
#[expect(
    clippy::needless_pass_by_value,
    reason = "mirrors clap argument structure"
)]
pub fn execute(
    project_name: ProjectName,
    org_id: OrganizationId,
    template: Template,
    platforms: Option<Vec<Platform>>,
    path: Option<PathBuf>,
    local: bool,
    _is_lib: bool,
) -> CliResult<()> {
    cliclack::intro(style(" flui create ").on_cyan().black())?;
    cliclack::log::info(format!("Project: {}", style(&project_name).cyan()))?;

    // Create validated project path
    let project_path = ProjectPath::new(&project_name, path)?;
    let project_dir = project_path.as_path();

    // Step 1: Create project directory
    let spinner = cliclack::spinner();
    spinner.start("Creating project directory...");
    std::fs::create_dir_all(project_dir).context("Failed to create project directory")?;
    spinner.stop(format!("{} Created project directory", style("✓").green()));

    // Step 2: Generate project from template
    cliclack::log::info(format!(
        "Template: {}",
        style(template.description()).cyan()
    ))?;

    // Convert Platform enums to string names for the builder.
    let platform_names: Vec<String> = platforms
        .unwrap_or_default()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

    let spinner = cliclack::spinner();
    spinner.start("Generating project files...");
    let _generated = TemplateBuilder::new(project_name.clone(), org_id)
        .template(template)
        .local(local)
        .platforms(platform_names)
        .with_git(false)
        .with_cargo_check(false)
        .generate(project_dir)?;
    spinner.stop(format!("{} Generated project files", style("✓").green()));

    // Step 3: Initialize git repository
    let spinner = cliclack::spinner();
    spinner.start("Initializing git repository...");
    init_git_repo(project_dir)?;
    spinner.stop(format!("{} Initialized git repository", style("✓").green()));

    // Step 4: Run cargo check
    let spinner = cliclack::spinner();
    spinner.start("Running cargo check (this may take a while)...");
    run_cargo_check(project_dir)?;
    spinner.stop(format!("{} Cargo check completed", style("✓").green()));

    // Print next steps
    let next_steps = format!(
        "{}\n  {}\n  {}",
        style("To get started:").bold(),
        style(format!("cd {project_name}")).dim(),
        style("flui run").dim(),
    );
    cliclack::note("Next Steps", next_steps)?;

    cliclack::outro(style(format!("Successfully created '{project_name}'")).green())?;

    Ok(())
}

/// Initialize a git repository in the project directory.
fn init_git_repo(dir: &Path) -> CliResult<()> {
    GitCommand::init()
        .output_style(OutputStyle::Silent)
        .run()
        .ok(); // Ignore git init errors (git might not be installed)

    // Create .gitignore
    std::fs::write(dir.join(".gitignore"), GITIGNORE_TEMPLATE)
        .context("Failed to create .gitignore")?;

    Ok(())
}

/// Run cargo check to validate the generated project.
fn run_cargo_check(dir: &Path) -> CliResult<()> {
    use std::process::Command;

    let output = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(dir)
        .output()
        .context("Failed to run cargo check")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = cliclack::log::warning("cargo check reported issues:");
        let _ = cliclack::log::warning(stderr);
        let _ = cliclack::log::remark("The project was created but may need fixes.");
    }

    Ok(())
}

/// Template for .gitignore file.
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
**/*.rs.bk
*.pdb
Cargo.lock
";

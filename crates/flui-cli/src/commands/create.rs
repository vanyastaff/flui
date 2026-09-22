use crate::error::{CliResult, ResultExt};
use crate::runner::{GitCommand, OutputStyle};
use crate::templates::{DependencySource, PlannedFile, ProjectPlan, TemplateBuilder};
use crate::types::{OrganizationId, ProjectName, ProjectPath};
use crate::ui;
use crate::{Platform, Template};
use console::style;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The options of `flui create`, grouped so adding one is a field,
/// not a positional argument every caller has to count.
#[derive(Debug, Clone, Default)]
pub(crate) struct CreateOptions {
    /// Use local path dependencies instead of crates.io versions.
    pub(crate) local: Option<PathBuf>,
    /// Skip the post-scaffold `cargo check`. The check only reports — it
    /// never fails the command — so skipping it changes nothing about the
    /// scaffold, only how long `create` takes.
    pub(crate) skip_check: bool,
    /// Generate the Flutter-parity hot-reload workspace (host/worker/types).
    pub(crate) hot_reload: bool,
    /// Build the plan and report it without writing anything: no directory,
    /// no git init, no cargo check.
    pub(crate) dry_run: bool,
}

/// Execute the create command.
///
/// # Arguments
///
/// * `project_name` - Validated project name
/// * `org_id` - Validated organization ID
/// * `template` - Template to use for project generation
/// * `platforms` - Target platforms to scaffold and record in `flui.toml`
/// * `path` - Optional custom output directory
/// * `options` - The remaining create-time choices; see [`CreateOptions`]
///
/// # Errors
///
/// Returns an error if:
/// - Directory already exists
/// - Template generation fails
/// - Git initialization fails
pub(crate) fn execute(
    project_name: ProjectName,
    org_id: OrganizationId,
    template: Template,
    platforms: Option<Vec<Platform>>,
    path: Option<PathBuf>,
    options: CreateOptions,
) -> CliResult<()> {
    let CreateOptions {
        local,
        skip_check,
        hot_reload,
        dry_run,
    } = options;

    let project_path = ProjectPath::new(&project_name, path)?;
    let project_dir = project_path.as_path();

    let source = DependencySource::resolve(local.as_deref(), hot_reload)?;

    let platform_names: Vec<String> = platforms
        .unwrap_or_default()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

    let builder = TemplateBuilder::new(project_name.clone(), org_id)
        .template(template)
        .dependency_source(source)
        .platforms(platform_names)
        .hot_reload(hot_reload)
        .with_git(false)
        .with_cargo_check(false);

    if dry_run {
        let plan = builder.plan();
        return report_dry_run(&project_name, template, project_dir, &plan);
    }

    ui::intro(style(" flui create ").on_cyan().black())?;
    ui::info(format!("Project: {}", style(&project_name).cyan()))?;
    ui::emit(
        "create.start",
        &json!({
            "name": project_name.as_str(),
            "template": template.to_string(),
            "path": project_dir,
        }),
    );

    let spinner = ui::spinner();
    spinner.start("Creating project directory...");
    std::fs::create_dir_all(project_dir).context("failed to create project directory")?;
    spinner.stop(format!("{} Created project directory", style("✓").green()));

    ui::info(format!(
        "Template: {}",
        style(template.description()).cyan()
    ))?;

    let spinner = ui::spinner();
    spinner.start("Generating project files...");
    let generated = builder.generate(project_dir)?;
    for file in generated.plan.files() {
        emit_created_file(file);
    }
    spinner.stop(format!("{} Generated project files", style("✓").green()));

    let spinner = ui::spinner();
    spinner.start("Initializing git repository...");
    let git_initialized = init_git_repo(project_dir);
    if git_initialized {
        spinner.stop(format!("{} Initialized git repository", style("✓").green()));
    } else {
        spinner.stop(format!(
            "{} Skipped git init (git not available)",
            style("→").dim()
        ));
    }

    let check = if skip_check {
        ui::info(format!(
            "{} Skipped cargo check (--no-check)",
            style("→").dim()
        ))?;
        "skipped"
    } else {
        let spinner = ui::spinner();
        spinner.start("Running cargo check (this may take a while)...");
        let check_passed = run_cargo_check(project_dir)?;
        if check_passed {
            spinner.stop(format!("{} Cargo check completed", style("✓").green()));
            "passed"
        } else {
            spinner.stop(format!(
                "{} Cargo check found problems — project created but does not yet compile",
                style("⚠").yellow()
            ));
            "failed"
        }
    };

    ui::emit(
        "create.done",
        &json!({
            "path": project_dir,
            "template": template.to_string(),
            "git": git_initialized,
            "check": check,
        }),
    );

    let next_steps = if template.is_library() {
        format!(
            "{}\n  {}\n  {}",
            style("To get started:").bold(),
            style(format!("cd {project_name}")).dim(),
            style("cargo test").dim(),
        )
    } else {
        format!(
            "{}\n  {}\n  {}",
            style("To get started:").bold(),
            style(format!("cd {project_name}")).dim(),
            style("flui run").dim(),
        )
    };
    ui::note("Next Steps", next_steps)?;

    ui::outro(style(format!("Successfully created '{project_name}'")).green())?;

    Ok(())
}

/// Report what `--dry-run` would create, without creating it.
///
/// Emits the same `create.start` / `create.file` / `create.done` events a
/// real run would (with `git: false` and `check: "skipped"`, since neither
/// runs), and prints the file list in human mode. [`ProjectPath::new`] has
/// already checked the target directory does not exist by the time this
/// runs, so the existence guarantee holds for a dry run too.
fn report_dry_run(
    name: &ProjectName,
    template: Template,
    dir: &Path,
    plan: &ProjectPlan,
) -> CliResult<()> {
    ui::intro(style(" flui create (dry run) ").on_cyan().black())?;
    ui::info(format!("Project: {}", style(name).cyan()))?;
    ui::info(format!(
        "Template: {}",
        style(template.description()).cyan()
    ))?;

    ui::emit(
        "create.start",
        &json!({
            "name": name.as_str(),
            "template": template.to_string(),
            "path": dir,
        }),
    );

    let mut lines = Vec::with_capacity(plan.files().len() + plan.dirs().len());
    for file in plan.files() {
        emit_created_file(file);
        lines.push(describe_file(file));
    }
    for dir in plan.dirs() {
        lines.push(format!("{}/", dir.display()));
    }
    ui::note("Files that would be written", lines.join("\n"))?;

    ui::emit(
        "create.done",
        &json!({
            "path": dir,
            "template": template.to_string(),
            "git": false,
            "check": "skipped",
        }),
    );

    ui::outro(style("Dry run — nothing was written").green())?;

    Ok(())
}

/// One `create.file` line for the human-mode dry-run listing.
fn describe_file(file: &PlannedFile) -> String {
    format!("{}  ({} bytes)", file.path.display(), file.contents.len())
}

/// Emit the JSON event for one file the plan wrote (or, for a dry run,
/// would write).
fn emit_created_file(file: &PlannedFile) {
    ui::emit(
        "create.file",
        &json!({
            "path": file.path,
            "bytes": file.contents.len(),
        }),
    );
}

/// Initialize a git repository in the project directory.
///
/// Returns whether a repository was actually created — `git` may simply not
/// be installed, which is not itself a failure of `create`. `.gitignore` is
/// written as part of the [`ProjectPlan`] (see [`TemplateBuilder::plan`]),
/// not here, so it shows up in `--dry-run` like every other generated file.
fn init_git_repo(dir: &Path) -> bool {
    GitCommand::init()
        .current_dir(dir)
        .output_style(OutputStyle::Silent)
        .run()
        .is_ok_and(|result| result.success())
}

/// Run cargo check to validate the generated project.
///
/// Returns `true` when the generated project compiles, `false` when `cargo
/// check` reported problems (in which case the diagnostics are surfaced to the
/// user). A `false` result is not an error — the scaffold itself succeeded — so
/// the caller decides how to report it rather than claiming success blindly.
fn run_cargo_check(dir: &Path) -> CliResult<bool> {
    use std::process::Command;

    let output = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(dir)
        .output()
        .context("failed to run cargo check")?;

    if output.status.success() {
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    ui::warning("cargo check reported issues:")?;
    ui::warning(stderr)?;
    ui::remark("The project was created but needs fixes before it compiles.")?;

    Ok(false)
}

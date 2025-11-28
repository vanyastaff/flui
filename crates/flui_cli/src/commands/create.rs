use crate::templates::TemplateGenerator;
use crate::{Platform, Template};
use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::process::Command;

pub fn execute(
    name: String,
    org: String,
    template: Template,
    _platforms: Option<Vec<Platform>>,
    path: Option<PathBuf>,
    _is_lib: bool,
) -> Result<()> {
    println!(
        "{}",
        style(format!("Creating FLUI project '{}'...", name))
            .green()
            .bold()
    );
    println!();

    // Validate project name
    validate_project_name(&name)?;

    // Determine project directory
    let project_dir = if let Some(custom_path) = path {
        custom_path.join(&name)
    } else {
        PathBuf::from(&name)
    };

    // Check if directory exists
    if project_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", project_dir.display());
    }

    // Create progress spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    // Create project directory
    spinner.set_message("Creating project directory...");
    std::fs::create_dir_all(&project_dir).context("Failed to create project directory")?;

    spinner.finish_and_clear();

    // Generate project from template
    let template_gen = TemplateGenerator::new(name.clone(), org.clone());

    println!("  {} Using template: {:?}", style("✓").green(), template);

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Generating project files...");

    match template {
        Template::Counter => template_gen.generate_counter(&project_dir)?,
        Template::Basic => template_gen.generate_basic(&project_dir)?,
        Template::Todo => template_gen.generate_todo(&project_dir)?,
        Template::Dashboard => template_gen.generate_dashboard(&project_dir)?,
        Template::Widget => template_gen.generate_widget(&project_dir)?,
        Template::Plugin => template_gen.generate_plugin(&project_dir)?,
        Template::Empty => template_gen.generate_empty(&project_dir)?,
    }

    spinner.finish_and_clear();

    // Initialize git repository
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Initializing git repository...");
    init_git_repo(&project_dir)?;
    spinner.finish_and_clear();
    println!("  {} Initialized git repository", style("✓").green());

    // Run cargo check to ensure dependencies are valid
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Running cargo check (this may take a while)...");
    run_cargo_check(&project_dir)?;
    spinner.finish_and_clear();
    println!("  {} Cargo check completed", style("✓").green());

    println!();
    println!(
        "{}",
        style(format!("✓ Successfully created FLUI project '{}'", name))
            .green()
            .bold()
    );
    println!();
    println!("To get started:");
    println!(
        "  {} {}",
        style("$").dim(),
        style(format!("cd {}", name)).cyan()
    );
    println!("  {} {}", style("$").dim(), style("flui run").cyan());
    println!();

    Ok(())
}

fn validate_project_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Project name cannot be empty");
    }

    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "Project name must contain only alphanumeric characters, hyphens, and underscores"
        );
    }

    if name.starts_with(|c: char| c.is_numeric()) {
        anyhow::bail!("Project name cannot start with a number");
    }

    // Reserved Rust keywords
    const RESERVED: &[&str] = &[
        "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
        "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
        "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "typeof",
        "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
    ];

    if RESERVED.contains(&name) {
        anyhow::bail!("Project name cannot be a Rust keyword: {}", name);
    }

    Ok(())
}

fn init_git_repo(dir: &PathBuf) -> Result<()> {
    // Initialize git
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .context("Failed to initialize git repository")?;

    // Create .gitignore
    let gitignore = r"# Build artifacts
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

    std::fs::write(dir.join(".gitignore"), gitignore).context("Failed to create .gitignore")?;

    Ok(())
}

fn run_cargo_check(dir: &PathBuf) -> Result<()> {
    let output = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(dir)
        .output()
        .context("Failed to run cargo check")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!(
            "\n{}",
            style("Warning: cargo check reported issues:").yellow()
        );
        eprintln!("{}", stderr);
        eprintln!(
            "{}",
            style("The project was created but may need fixes.").yellow()
        );
    }

    Ok(())
}

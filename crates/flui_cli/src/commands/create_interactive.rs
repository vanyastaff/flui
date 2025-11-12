use crate::Template;
use anyhow::Result;
use console::style;
use dialoguer::{Input, Select, Confirm};

pub struct ProjectConfig {
    pub name: String,
    pub org: String,
    pub template: Template,
}

pub fn interactive_create() -> Result<ProjectConfig> {
    println!("{}", style("Let's create a new FLUI project!").green().bold());
    println!();

    // Ask for project name
    let name: String = Input::new()
        .with_prompt("Project name")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.is_empty() {
                return Err("Project name cannot be empty");
            }
            if !input.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                return Err("Project name must contain only alphanumeric characters, hyphens, and underscores");
            }
            if input.starts_with(|c: char| c.is_numeric()) {
                return Err("Project name cannot start with a number");
            }
            Ok(())
        })
        .interact_text()?;

    // Ask for organization
    let org: String = Input::new()
        .with_prompt("Organization (reverse domain notation)")
        .default("com.example".into())
        .interact_text()?;

    // Ask for template
    let templates = [
        ("Counter", "Simple counter app with state management"),
        ("Basic", "Minimal FLUI application"),
        ("Todo", "Todo list app (coming soon)"),
        ("Dashboard", "Dashboard UI (coming soon)"),
    ];

    let selection = Select::new()
        .with_prompt("Choose a template")
        .items(templates.iter().map(|(name, desc)| format!("{} - {}", name, desc)).collect::<Vec<_>>().as_slice())
        .default(0)
        .interact()?;

    let template = match selection {
        0 => Template::Counter,
        1 => Template::Basic,
        2 => Template::Todo,
        3 => Template::Dashboard,
        _ => Template::Counter,
    };

    // Confirm
    println!();
    println!("{}", style("Summary:").cyan().bold());
    println!("  Name: {}", style(&name).yellow());
    println!("  Organization: {}", style(&org).yellow());
    println!("  Template: {:?}", style(format!("{:?}", template)).yellow());
    println!();

    let confirmed = Confirm::new()
        .with_prompt("Create project?")
        .default(true)
        .interact()?;

    if !confirmed {
        anyhow::bail!("Project creation cancelled");
    }

    Ok(ProjectConfig { name, org, template })
}

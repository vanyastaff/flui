use super::DependencySource;
use crate::error::{CliResult, ResultExt};
use flui_build::scaffold::{ScaffoldParams, scaffold_platform};
use std::fs;
use std::path::Path;

pub fn generate(
    dir: &Path,
    name: &str,
    org: &str,
    source: &DependencySource,
    platforms: &[String],
) -> CliResult<()> {
    // Create Cargo.toml
    generate_cargo_toml(dir, name, source)?;

    // Create src/main.rs
    generate_main(dir)?;

    // Create flui.toml
    generate_flui_config(dir, name, org, platforms)?;

    // Create README.md
    generate_readme(dir, name)?;

    // Create assets directory
    fs::create_dir_all(dir.join("assets"))?;

    // Scaffold platform directories
    scaffold_platforms(dir, name, org, platforms)?;

    Ok(())
}

fn generate_cargo_toml(dir: &Path, name: &str, source: &DependencySource) -> CliResult<()> {
    let version = env!("CARGO_PKG_VERSION");

    let deps = format!("flui = {}", source.dependency("flui", &[]));
    let mode_comment = if matches!(source, DependencySource::Local(_)) {
        " (local development)"
    } else {
        ""
    };

    let content = format!(
        r#"# FLUI Template v{version}{mode_comment}

# Standalone workspace declaration so this project is not absorbed into
# any parent workspace that may contain the FLUI source tree.
[workspace]

[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.97"

[dependencies]
{deps}

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "debuginfo"
"#
    );

    fs::write(dir.join("Cargo.toml"), content).context("Failed to create Cargo.toml")?;
    Ok(())
}

fn generate_main(dir: &Path) -> CliResult<()> {
    let content = r#"use flui::prelude::*;

fn main() {
    run_app(HelloView);
}

#[derive(Clone, StatelessView)]
struct HelloView;

impl StatelessView for HelloView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Center::new().child(Text::new("Hello, FLUI!"))
    }
}
"#;

    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(src_dir.join("main.rs"), content).context("Failed to create src/main.rs")?;

    Ok(())
}

fn generate_flui_config(dir: &Path, name: &str, org: &str, platforms: &[String]) -> CliResult<()> {
    let platform_list = if platforms.is_empty() {
        r#"["windows", "linux", "macos"]"#.to_string()
    } else {
        let quoted: Vec<String> = platforms.iter().map(|p| format!("\"{p}\"")).collect();
        format!("[{}]", quoted.join(", "))
    };

    let content = format!(
        r#"[app]
name = "{name}"
version = "0.1.0"
organization = "{org}"

[build]
target_platforms = {platform_list}

[assets]
directories = ["assets"]

# [[fonts]]
# family = "Roboto"
# fonts = [
#     {{ asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" }},
# ]
"#
    );

    fs::write(dir.join("flui.toml"), content).context("Failed to create flui.toml")?;
    Ok(())
}

fn generate_readme(dir: &Path, name: &str) -> CliResult<()> {
    let content = format!(
        r"# {name}

A FLUI application.

## Getting Started

```bash
flui run
```

## Build

```bash
flui build desktop --release
```
"
    );

    fs::write(dir.join("README.md"), content).context("Failed to create README.md")?;
    Ok(())
}

/// Scaffold platform directories based on the selected platforms.
fn scaffold_platforms(dir: &Path, name: &str, org: &str, platforms: &[String]) -> CliResult<()> {
    if platforms.is_empty() {
        return Ok(());
    }

    let lib_name = name.replace('-', "_");
    let package_name = format!("{org}.{lib_name}");
    let params = ScaffoldParams {
        app_name: name,
        lib_name: &lib_name,
        package_name: &package_name,
    };

    for platform in platforms {
        scaffold_platform(platform, dir, &params)
            .map_err(|e| crate::error::CliError::build_failed(platform, e.to_string()))?;
    }

    Ok(())
}

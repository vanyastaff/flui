use crate::error::{CliResult, ResultExt};
use flui_build::scaffold::{scaffold_platform, ScaffoldParams};
use std::fs;
use std::path::Path;

pub fn generate(
    dir: &Path,
    name: &str,
    org: &str,
    local: bool,
    platforms: &[String],
) -> CliResult<()> {
    // Create Cargo.toml
    generate_cargo_toml(dir, name, local)?;

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

fn generate_cargo_toml(dir: &Path, name: &str, local: bool) -> CliResult<()> {
    let version = env!("CARGO_PKG_VERSION");

    let deps = if local {
        r#"flui_app = { path = "../../crates/flui_app" }
flui_widgets = { path = "../../crates/flui_widgets" }
flui_core = { path = "../../crates/flui_core" }
flui_types = { path = "../../crates/flui_types" }"#
            .to_string()
    } else {
        format!(
            r#"flui_app = "{version}"
flui_widgets = "{version}"
flui_core = "{version}"
flui_types = "{version}""#
        )
    };

    let mode_comment = if local { " (local development)" } else { "" };

    let content = format!(
        r#"# FLUI Template v{version}{mode_comment}

[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
rust-version = "1.91"

[dependencies]
{deps}

# Logging
tracing = "0.1"

[dev-dependencies]
env_logger = "0.11"

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
    let content = r#"use flui_app::runApp;
use flui_core::prelude::*;
use flui_widgets::*;

fn main() {
    // Initialize logging
    #[cfg(debug_assertions)]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    runApp(MyApp);
}

#[derive(Debug)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        MaterialApp::builder()
            .title("FLUI App")
            .theme(ThemeData::light())
            .home(HomeView)
            .build()
    }
}

#[derive(Debug)]
struct HomeView;

impl View for HomeView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Scaffold::builder()
            .app_bar(
                AppBar::new()
                    .title(Text::new("FLUI App"))
            )
            .body(
                Center::new(
                    Text::new("Hello, FLUI!")
                        .style(
                            TextStyle::new()
                                .font_size(24.0)
                                .font_weight(FontWeight::Bold)
                        )
                )
            )
            .build()
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

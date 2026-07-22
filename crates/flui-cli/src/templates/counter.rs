use crate::error::{CliResult, ResultExt};
use flui_build::scaffold::{ScaffoldParams, scaffold_platform};
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

    // LOCAL mode: path deps assume the project lives at <flui-root>/<subdir>/<name>/
    // so "../../crates/" resolves to the workspace crates directory.
    // PUBLISHED mode: version strings won't resolve until FLUI is on crates.io.
    //
    // flui-view is a required direct dep: the `#[derive(StatelessView)]` macro
    // expands to `::flui_view::View` references that must resolve at the crate root.
    let deps = if local {
        r#"flui-app = { path = "../../crates/flui-app" }
flui-view = { path = "../../crates/flui-view" }
flui-widgets = { path = "../../crates/flui-widgets" }"#
            .to_string()
    } else {
        format!(
            // NOTE: FLUI is not yet published to crates.io.
            // These version strings will not resolve until the crates are released.
            // Use `flui create --local` when working from the FLUI source tree.
            r#"flui-app = "{version}"
flui-view = "{version}"
flui-widgets = "{version}""#
        )
    };

    let mode_comment = if local { " (local development)" } else { "" };

    let content = format!(
        r#"# FLUI Template v{version}{mode_comment}

# Standalone workspace declaration so this project is not absorbed into
# any parent workspace that may contain the FLUI source tree.
[workspace]

[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.96"

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
    // The interactive-counter pattern (StatefulView + GestureDetector rebuild
    // trigger) is not yet ergonomic through the public API. This template shows
    // the widget-composition surface and a static counter display; to add
    // live state see the StatefulView + ViewState pair in the flui-view docs.
    let content = r#"use flui_app::run_app;
use flui_widgets::prelude::*;
use flui_widgets::column;

fn main() {
    run_app(CounterView);
}

#[derive(Clone, StatelessView)]
struct CounterView;

impl StatelessView for CounterView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Center::new().child(Column::new(column![
            Text::new("You have pushed the button this many times:"),
            SizedBox::height(16.0),
            Text::new("0"),
        ]))
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
# Asset directories
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

A FLUI counter application.

## Getting Started

Run the application:

```bash
flui run
```

Build for release:

```bash
flui build desktop --release
```

Run tests:

```bash
flui test
```

## Learn More

- [FLUI Documentation](https://github.com/vanyastaff/flui)
- [Examples](https://github.com/vanyastaff/flui/tree/main/examples)
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

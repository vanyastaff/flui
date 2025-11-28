use crate::error::{CliResult, ResultExt};
use std::fs;
use std::path::Path;

pub fn generate(dir: &Path, name: &str, org: &str) -> CliResult<()> {
    // Create Cargo.toml
    generate_cargo_toml(dir, name)?;

    // Create src/main.rs
    generate_main(dir)?;

    // Create flui.toml
    generate_flui_config(dir, name, org)?;

    // Create README.md
    generate_readme(dir, name)?;

    // Create assets directory
    fs::create_dir_all(dir.join("assets"))?;

    Ok(())
}

fn generate_cargo_toml(dir: &Path, name: &str) -> CliResult<()> {
    let content = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"
rust-version = "1.90"

[dependencies]
flui_app = {{ path = "../../crates/flui_app" }}
flui_widgets = {{ path = "../../crates/flui_widgets" }}
flui_core = {{ path = "../../crates/flui_core" }}
flui_types = {{ path = "../../crates/flui_types" }}

# Logging
tracing = "0.1"

[dev-dependencies]
env_logger = "0.11"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "debuginfo"
"#,
        name
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

fn generate_flui_config(dir: &Path, name: &str, org: &str) -> CliResult<()> {
    let content = format!(
        r#"[app]
name = "{}"
version = "0.1.0"
organization = "{}"

[build]
target_platforms = ["windows", "linux", "macos"]

[assets]
directories = ["assets"]

[fonts]
# Add custom fonts here
"#,
        name, org
    );

    fs::write(dir.join("flui.toml"), content).context("Failed to create flui.toml")?;
    Ok(())
}

fn generate_readme(dir: &Path, name: &str) -> CliResult<()> {
    let content = format!(
        r#"# {}

A FLUI application.

## Getting Started

```bash
flui run
```

## Build

```bash
flui build desktop --release
```
"#,
        name
    );

    fs::write(dir.join("README.md"), content).context("Failed to create README.md")?;
    Ok(())
}

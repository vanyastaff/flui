use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn generate(dir: &Path, name: &str, org: &str) -> Result<()> {
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

fn generate_cargo_toml(dir: &Path, name: &str) -> Result<()> {
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

# Optional: DevTools (debug builds only)
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

fn generate_main(dir: &Path) -> Result<()> {
    let content = r#"use flui_app::runApp;
use flui_core::prelude::*;
use flui_widgets::*;

fn main() {
    // Initialize logging
    #[cfg(debug_assertions)]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    runApp(CounterApp);
}

#[derive(Debug)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        MaterialApp::builder()
            .title("FLUI Counter")
            .theme(ThemeData::light())
            .home(CounterView)
            .build()
    }
}

#[derive(Debug)]
struct CounterView;

impl View for CounterView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create a signal for the counter state
        let count = use_signal(ctx, 0);
        let count_clone = count.clone();

        Scaffold::builder()
            .app_bar(
                AppBar::new()
                    .title(Text::new("FLUI Counter"))
            )
            .body(
                Center::new(
                    Column::new()
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .children(vec![
                            Box::new(Text::new("You have pushed the button this many times:")),
                            Box::new(
                                Text::new(format!("{}", count.get()))
                                    .style(
                                        TextStyle::new()
                                            .font_size(48.0)
                                            .font_weight(FontWeight::Bold)
                                    )
                            ),
                        ])
                )
            )
            .floating_action_button(
                FloatingActionButton::new(Icon::new("add"))
                    .on_pressed(move || {
                        count_clone.update(|c| *c += 1);
                    })
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

fn generate_flui_config(dir: &Path, name: &str, org: &str) -> Result<()> {
    let content = format!(
        r#"[app]
name = "{}"
version = "0.1.0"
organization = "{}"

[build]
target_platforms = ["windows", "linux", "macos"]

[assets]
# Asset directories
directories = ["assets"]

[fonts]
# Custom fonts can be added here
# [[fonts]]
# family = "Roboto"
# fonts = [
#     {{ asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" }},
# ]
"#,
        name, org
    );

    fs::write(dir.join("flui.toml"), content).context("Failed to create flui.toml")?;
    Ok(())
}

fn generate_readme(dir: &Path, name: &str) -> Result<()> {
    let content = format!(
        r#"# {}

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
"#,
        name
    );

    fs::write(dir.join("README.md"), content).context("Failed to create README.md")?;
    Ok(())
}

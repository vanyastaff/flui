use super::{DependencySource, ProjectPlan};

pub fn generate(
    name: &str,
    app_id: &str,
    source: &DependencySource,
    platforms: &[String],
) -> ProjectPlan {
    let lib_name = name.replace('-', "_");
    ProjectPlan::new()
        .file("Cargo.toml", cargo_toml(name, source))
        .file("src/lib.rs", LIB)
        .file("src/main.rs", super::main_rs(&lib_name, "CounterApp"))
        .file("flui.toml", flui_toml(name, app_id, platforms))
        .file("README.md", readme(name))
        .dir("assets")
}

fn cargo_toml(name: &str, source: &DependencySource) -> String {
    let version = env!("CARGO_PKG_VERSION");

    let deps = format!("flui = {}", source.dependency("flui", &[]));
    let android_deps = format!("flui = {}", source.dependency("flui", &["native"]));
    let test_deps = format!("flui = {}", source.dependency("flui", &["testing"]));
    let mode_comment = if matches!(source, DependencySource::Local(_)) {
        " (local development)"
    } else {
        ""
    };
    let lib_table = super::LIB_TABLE;

    format!(
        r#"# FLUI Template v{version}{mode_comment}

# Standalone workspace declaration so this project is not absorbed into
# any parent workspace that may contain the FLUI source tree.
[workspace]

[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.99"

{lib_table}
[dependencies]
{deps}

[target.'cfg(target_os = "android")'.dependencies]
{android_deps}

[dev-dependencies]
{test_deps}

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "debuginfo"
"#
    )
}

const LIB: &str = include_str!("counter_app.rs");

fn flui_toml(name: &str, app_id: &str, platforms: &[String]) -> String {
    let platform_list = if platforms.is_empty() {
        r#"["windows", "linux", "macos"]"#.to_string()
    } else {
        let quoted: Vec<String> = platforms.iter().map(|p| format!("\"{p}\"")).collect();
        format!("[{}]", quoted.join(", "))
    };

    format!(
        r#"[app]
name = "{name}"
version = "0.1.0"
id = "{app_id}"

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
    )
}

fn readme(name: &str) -> String {
    format!(
        r"# {name}

A FLUI counter application. Press Increment to update the count.
The generated test exercises pointer input and checks that state survives a rebuild.

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
    )
}

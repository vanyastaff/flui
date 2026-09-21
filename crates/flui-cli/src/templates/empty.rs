use super::{DependencySource, ProjectPlan};

/// The smallest runnable FLUI app: one `main` that shows one `Text`.
pub fn generate(
    name: &str,
    org: &str,
    source: &DependencySource,
    platforms: &[String],
) -> ProjectPlan {
    ProjectPlan::new()
        .file("Cargo.toml", cargo_toml(name, source))
        .file("src/main.rs", MAIN)
        .file("flui.toml", flui_toml(name, org, platforms))
        .dir("assets")
}

fn cargo_toml(name: &str, source: &DependencySource) -> String {
    let version = env!("CARGO_PKG_VERSION");

    let deps = format!("flui = {}", source.dependency("flui", &[]));
    let mode_comment = if matches!(source, DependencySource::Local(_)) {
        " (local development)"
    } else {
        ""
    };

    format!(
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
    )
}

const MAIN: &str = r#"use flui::prelude::*;

fn main() {
    run_app(HelloView);
}

#[derive(Clone, StatelessView)]
struct HelloView;

impl StatelessView for HelloView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Hello, FLUI!")
    }
}
"#;

fn flui_toml(name: &str, org: &str, platforms: &[String]) -> String {
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
organization = "{org}"

[build]
target_platforms = {platform_list}

[assets]
directories = ["assets"]
"#
    )
}

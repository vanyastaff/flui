use super::{DependencySource, ProjectPlan};

/// The smallest runnable FLUI app: one `main` that shows one `Text`.
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
        .file("src/main.rs", super::main_rs(&lib_name))
        .file("flui.toml", flui_toml(name, app_id, platforms))
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
    let lib_table = super::LIB_TABLE;
    let profiles = super::PROFILES;

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

{profiles}"#
    )
}

const LIB: &str = r#"//! The application, and the entry points the platforms call.
//!
//! [`run`] is where every platform starts: `src/main.rs` calls it on the
//! desktop, iOS and the web, and on Android `#[flui::main]` makes it the
//! entry `NativeActivity` calls once it has loaded this crate as a `cdylib`
//! (`flui build android`).

use flui::prelude::*;

/// Start the application in a window with [`HelloView`] as its root.
#[flui::main]
pub fn run() -> Result<(), flui::LaunchError> {
    flui::run_app(HelloView)
}

#[derive(Clone, StatelessView)]
pub struct HelloView;

impl StatelessView for HelloView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Hello, FLUI!")
    }
}
"#;

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
directories = ["assets"]
"#
    )
}

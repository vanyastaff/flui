use super::{DependencySource, ProjectPlan};

/// A reusable widget library: one `StatelessView` plus a widget test, no
/// `main.rs` and no `[[bin]]` — `flui run` detects the missing binary target
/// via `cargo metadata` and refuses to run it.
pub fn generate(name: &str, org: &str, source: &DependencySource) -> ProjectPlan {
    let lib_name = name.replace('-', "_");

    ProjectPlan::new()
        .file("Cargo.toml", cargo_toml(name, &lib_name, source))
        .file("src/lib.rs", LIB)
        .file("flui.toml", flui_toml(name, org))
        .file("README.md", readme(name))
}

fn cargo_toml(name: &str, lib_name: &str, source: &DependencySource) -> String {
    let version = env!("CARGO_PKG_VERSION");

    let deps = format!("flui = {}", source.dependency("flui", &[]));
    let test_deps = format!("flui = {}", source.dependency("flui", &["testing"]));
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

[lib]
name = "{lib_name}"
path = "src/lib.rs"

[dependencies]
{deps}

[dev-dependencies]
{test_deps}
"#
    )
}

const LIB: &str = r#"//! A reusable FLUI widget.

use flui::prelude::*;

/// Greets `name`.
#[derive(Clone, StatelessView)]
pub struct Greeting {
    /// Who to greet.
    pub name: String,
}

impl StatelessView for Greeting {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(format!("Hello, {}!", self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui::testing::widgets::{lay_out, tight};

    #[test]
    fn greeting_renders_its_name() {
        let app = lay_out(
            Greeting {
                name: "FLUI".to_string(),
            },
            tight(200.0, 100.0),
        );
        assert!(app.find_text("Hello, FLUI!").is_some());
    }
}
"#;

fn flui_toml(name: &str, org: &str) -> String {
    format!(
        r#"[app]
name = "{name}"
version = "0.1.0"
organization = "{org}"
"#
    )
}

fn readme(name: &str) -> String {
    format!(
        r"# {name}

A reusable FLUI widget library.

## Test

```bash
cargo test
```

This is a library crate (no `main.rs`, no `[[bin]]`): `flui run` refuses it.
Depend on `{name}` from an application crate and use its exported widgets.
"
    )
}

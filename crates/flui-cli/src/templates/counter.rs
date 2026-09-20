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
    let test_deps = format!("flui = {}", source.dependency("flui", &["testing"]));
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

[dev-dependencies]
{test_deps}

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
    let content = r#"use std::{cell::Cell, rc::Rc};

use flui::prelude::*;
use flui::view::{RebuildHandle, RebuildReason};
use flui::widgets::{SafeArea, column};

fn main() {
    run_app(CounterApp);
}

#[derive(Clone, StatelessView)]
struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), SafeArea::new().child(CounterView))
    }
}

#[derive(Clone, StatefulView)]
struct CounterView;

struct CounterState {
    count: Rc<Cell<usize>>,
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: Rc::new(Cell::new(0)),
            rebuild: None,
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = Rc::clone(&self.count);
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build");

        // `main_axis_alignment` is what actually centres this, not the `Center`
        // around it. A `Column` fills the height it is given
        // (`main_axis_size` defaults to `MainAxisSize::Max`), so `Center` has
        // no slack to centre it in: the column is as tall as the screen and
        // packs its children at the top (`MainAxisAlignment::Start`, both
        // `FlexStyle` defaults). Flutter's own counter sample passes
        // `mainAxisAlignment: MainAxisAlignment.center` for the same reason.
        Center::new().child(
            Column::new(column![
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(self.count.get().to_string()),
                SizedBox::height(16.0),
                ElevatedButton::new(Text::new("Increment")).on_pressed(move || {
                    count.set(count.get() + 1);
                    rebuild.schedule(RebuildReason::StateChange);
                }),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui::geometry::Size;
    use flui::testing::widgets::{lay_out, tight};

    /// The tree the application is mounted as: the app under the root
    /// `MediaQuery` the app runner attaches it to.
    ///
    /// The runner supplies this root (`flui-app`'s `MediaQueryRoot`), so
    /// `main` does not — but a test that mounts the app directly has to, or it
    /// is not mounting the tree the application runs as. `SafeArea` reads its
    /// insets from that root and panics without one:
    ///
    /// ```text
    /// MediaQuery::of called with no MediaQuery ancestor in the tree
    /// ```
    ///
    /// The data describes the surface the constraints describe, with no
    /// insets, so the geometry assertions below are exact.
    fn app_tree(width: f32, height: f32) -> MediaQuery {
        MediaQuery::new(
            MediaQueryData {
                size: Size::new(px(width), px(height)),
                ..MediaQueryData::default()
            },
            CounterApp,
        )
    }

    #[test]
    fn counter_responds_to_pointer_input() {
        let mut app = lay_out(app_tree(480.0, 320.0), tight(480.0, 320.0));
        assert!(app.find_text("0").is_some());
        for (previous, next) in [("0", "1"), ("1", "2")] {
            let label = app.find_text("Increment").expect("increment button label");
            let offset = app.absolute_offset(label);
            let size = app.size(label);
            let x = offset.dx.get() + size.width.get() / 2.0;
            let y = offset.dy.get() + size.height.get() / 2.0;
            app.dispatch_pointer_down(x, y);
            app.dispatch_pointer_up(x, y);
            // Only scheduled work runs: a missing rebuild request must fail.
            app.tick();
            assert!(
                app.find_text(next).is_some(),
                "counter should display {next}"
            );
            assert!(app.find_text(previous).is_none());
        }
        // The root is swapped for an equal tree, so this rebuilds the whole
        // app — the point being that the count outlives it.
        app.pump_widget(app_tree(480.0, 320.0));
        assert!(
            app.find_text("2").is_some(),
            "state survives a parent rebuild"
        );
        let independent = lay_out(app_tree(480.0, 320.0), tight(480.0, 320.0));
        assert!(
            independent.find_text("0").is_some(),
            "each app owns its state"
        );
    }

    /// The counter is centred on the surface, not merely wrapped in a
    /// `Center`.
    ///
    /// A `Column` fills the height it is given (`MainAxisSize::Max` is the
    /// default) and packs its children at the top (`MainAxisAlignment::Start`),
    /// so a `Center` around it has no slack to centre anything in: the wrap
    /// looks like the centreing and is not. This asserts the geometry the wrap
    /// is meant to produce — the content block's vertical centre lands on the
    /// surface's, within a pixel of rounding.
    #[test]
    fn counter_content_is_centred() {
        const WIDTH: f32 = 480.0;
        const HEIGHT: f32 = 320.0;
        let app = lay_out(app_tree(WIDTH, HEIGHT), tight(WIDTH, HEIGHT));

        let top = app
            .find_text("You have pushed the button this many times:")
            .expect("prompt text");
        let bottom = app.find_text("Increment").expect("increment button label");
        let top_offset = app.absolute_offset(top).dy.get();
        let bottom_edge = app.absolute_offset(bottom).dy.get() + app.size(bottom).height.get();

        let content_centre = (top_offset + bottom_edge) / 2.0;
        assert!(
            (content_centre - HEIGHT / 2.0).abs() <= 1.0,
            "content spans y {top_offset}..{bottom_edge} of a {HEIGHT}pt surface, so its centre is \
             {content_centre} rather than {}; a `Column` fills the height and packs its children at \
             the top unless it is told to centre them (`MainAxisAlignment::Center`)",
            HEIGHT / 2.0
        );
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

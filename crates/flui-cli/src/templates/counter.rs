use super::{DependencySource, FrameworkPackage, ProjectPlan};

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
        .file("src/main.rs", super::main_rs(&lib_name, "CounterView"))
        .file("flui.toml", flui_toml(name, app_id, platforms))
        .file("README.md", readme(name))
        .dir("assets")
}

fn cargo_toml(name: &str, source: &DependencySource) -> String {
    let version = env!("CARGO_PKG_VERSION");

    let deps = format!(
        "flui = {}",
        source.dependency(FrameworkPackage::Facade, &[])
    );
    let test_deps = format!(
        "flui = {}",
        source.dependency(FrameworkPackage::Facade, &["testing"])
    );
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

[dev-dependencies]
{test_deps}

{profiles}"#
    )
}

const LIB: &str = r#"//! The application, and the entry points the platforms call.
//!
//! `src/main.rs` mounts [`CounterView`] on the desktop; `android_main` below
//! is what Android's `NativeActivity` calls once it has loaded this crate as
//! a `cdylib` (`flui build android`).

use flui::prelude::*;
use flui::widgets::SafeArea;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: flui::android_activity::AndroidApp) {
    // `run_app_android` logs why it could not start.
    let _started = flui::run_app_android(app, CounterView, flui::AppLaunch::new());
}

#[derive(Clone, StatefulView)]
pub struct CounterView;

pub struct CounterState {
    count: Signal<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;

        SafeArea::new(
            Center::new().child(
                Column::new((
                    Text::new("You have pushed the button this many times:"),
                    SizedBox::height(16.0),
                    Text::new(count.get(ctx).to_string()),
                    SizedBox::height(16.0),
                    RawButton::new(Text::new("Increment"))
                        .on_press(move |ctx| count.update(ctx, |n| *n += 1)),
                ))
                .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui::testing::{TestWindow, Tester, find};

    #[test]
    fn counter_increments() {
        let mut t = Tester::mount(CounterView);
        t.expect(find::text("0")).to_exist();
        t.tap(&find::button("Increment"));
        t.expect(find::text("1")).to_exist();
        t.tap(&find::button("Increment"));
        t.expect(find::text("2")).to_exist();
    }

    #[test]
    fn each_app_owns_its_state() {
        let mut first = Tester::mount(CounterView);
        first.tap(&find::button("Increment"));
        let mut second = Tester::mount(CounterView);
        second.expect(find::text("0")).to_exist();
    }

    /// The counter is centred on the surface, not merely wrapped in a
    /// `Center`: a `Column` fills the height it is given and packs its
    /// children at the top unless it is told to centre them
    /// (`MainAxisAlignment::Center`).
    #[test]
    fn counter_content_is_centred() {
        let mut t = Tester::mount_in(CounterView, TestWindow::new(480, 320));
        let top = t.rect(&find::text("You have pushed the button this many times:"));
        let bottom = t.rect(&find::button("Increment"));
        let centre = (top.y + bottom.y + bottom.height) / 2.0;
        assert!(
            (centre - 160.0).abs() <= 1.0,
            "the content spans y {}..{} of a 320 pt surface",
            top.y,
            bottom.y + bottom.height
        );
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
The generated tests tap the button and check what the screen shows.

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

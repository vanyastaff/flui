# Platforms

Flutter counterpart: `flutter run -d`, `flutter build`, `defaultTargetPlatform`.

The application written so far runs on every platform FLUI targets. The `flui` CLI builds and
starts it on each:

```bash
flui devices              # what this machine can run on
flui run                  # this desktop
flui run --device <name>  # a phone, a simulator or a browser from `flui devices`
flui build android        # also ios, web, windows, linux, macos
```

[Installation and first run](../getting-started/installation.md) lists what each target needs
installed, and the repository's
[platform status](https://github.com/vanyastaff/flui/blob/main/docs/platform-parity.md) records
what has been run on each.

## Following the platform's conventions

Some details differ by platform: the shortcut a menu shows, where a dialog puts its buttons. A
widget asks which platform's conventions it follows with `PlatformConvention::of(ctx)`, which the
runtime installs above every root:

```rust
use flui::platform::TargetPlatform;
use flui::prelude::*;
use flui::view::environment::PlatformConvention;

/// The save hint under the notes list, in the platform's own words.
#[derive(Clone, StatelessView)]
struct SaveHint;

impl StatelessView for SaveHint {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chord = if PlatformConvention::of(ctx).is_apple() { "⌘S" } else { "Ctrl+S" };
        Text::new(format!("{chord} saves"))
    }
}
# use flui::testing::{TestWindow, Tester, find};
# let on = |platform| TestWindow::new(800, 600).platform(platform);
# Tester::mount_in(SaveHint, on(TargetPlatform::MacOS)).expect(find::text("⌘S saves")).to_exist();
# Tester::mount_in(SaveHint, on(TargetPlatform::Windows)).expect(find::text("Ctrl+S saves")).to_exist();
# // A subtree that names a platform follows it, whatever the window's.
# let mac = PlatformConvention::new(TargetPlatform::MacOS, SaveHint);
# Tester::mount_in(mac, on(TargetPlatform::Windows)).expect(find::text("⌘S saves")).to_exist();
```

The convention is the platform the application runs on, unless a test or a subtree names
another: a test passes `TestWindow::platform`, and a `PlatformConvention::new(platform, child)`
above a subtree changes it for that subtree. `TargetPlatform::current()` is not a substitute: it is
the target the code was compiled for, so it is `Unknown` in a browser whatever the visitor's
system, and in a test it is the target the test was compiled for rather than the platform the
test names.

That completes the notes application: a window, a laid-out screen, state, input, a list, pages,
a theme, its tests and its platforms.

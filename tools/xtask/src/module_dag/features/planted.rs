//! The self-test crates of the feature axis. Crate `f` plants one violation
//! per rule (a grouped case and a path through a re-export among them) beside
//! the legal edges that stay silent; `g` the declaration findings; `h` a
//! declaration that mixes the axes.

use super::super::Planted;

/// The self-test crates of the feature axis.
pub(in crate::module_dag) const PLANTED: [Planted; 3] = [
    Planted {
        name: "f",
        declaration: r#"
features = ["window", "display", "text"]
platforms = ["windows", "apple", "web", "winit", "linux"]
groups = { apple = ["macos", "ios"], linux = ["wayland", "x11"] }
base = ["owner", "version"]
hosts = "hosts"
transparent = ["prelude"]
exceptions = [
    { from = "display::winit", to = "display::linux", exit = "ADR-0001", since = "2026-10-21", reason = "admitted" },
    { from = "text", to = "text::windows", exit = "ADR-0001", since = "2026-10-21", reason = "the rules allow a model its own platform file" },
    { from = "window::winit", to = "window::windows", exit = "ADR-0001", since = "2026-10-21", reason = "the code no longer has the edge" },
    { from = "display::wasm", to = "display::linux", exit = "ADR-0001", since = "2026-10-21", reason = "no such module" },
]
"#,
        files: &[
            (
                "lib.rs",
                r"//! Planted feature crate.
pub mod window;
pub mod display;
pub mod text;
pub mod owner;
pub mod version;
pub mod hosts;
pub mod platform;
pub mod prelude;
#[cfg(test)]
mod root_tests;
pub use hosts::Host;
",
            ),
            // The root is no source: it names `hosts` to pick a host.
            // Test code is exempt.
            ("root_tests.rs", "use crate::hosts::Host;\n"),
            // A crate-level module: unchecked as a source, except that
            // nothing outside `hosts` names it.
            (
                "platform.rs",
                r"pub struct Platform;
pub fn host() -> crate::hosts::Host { crate::hosts::Host }
pub fn window() -> crate::window::windows::WinWindow { crate::window::windows::WinWindow }
use crate::Nowhere;
",
            ),
            // A relay module: a path through it counts as a path to its source.
            (
                "prelude.rs",
                r"pub use crate::display::windows::DisplayWin;
pub use crate::window::Window;
",
            ),
            // Base modules, with their descendants.
            (
                "owner/mod.rs",
                r"mod inner;
pub struct Owner;
use crate::version::Version;
use crate::platform::Platform;
use crate::window::Window;
use crate::hosts::Host;
",
            ),
            (
                "owner/inner.rs",
                r"use super::Owner;
use crate::display::apple::macos::MacDisplay;
",
            ),
            (
                "version.rs",
                r"pub struct Version;
use crate::owner::Owner;
use crate::text::windows::TextWin;
",
            ),
            // The hosts module names everything.
            (
                "hosts/mod.rs",
                r"pub mod windows;
pub struct Host;
use crate::window::windows::WinWindow;
use crate::display::apple::macos::MacDisplay;
use crate::owner::Owner;
use crate::platform::Platform;
",
            ),
            (
                "hosts/windows.rs",
                "pub struct WinHost;\nuse crate::text::windows::TextWin;\n",
            ),
            // `window`: a model, platform files with a nested submodule, and a
            // group that opens into children.
            (
                "window/mod.rs",
                r"pub mod windows;
pub mod winit;
pub mod apple;
pub struct Window;
pub struct State;
pub use windows::WinWindow;
pub use crate::window::apple::Shared;
use crate::display::Display;
use crate::display::windows::DisplayWin;
",
            ),
            (
                "window/windows/mod.rs",
                r"pub mod proc;
pub struct WinWindow;
use super::State;
use crate::display::Display;
use crate::owner::Owner;
use crate::platform::Platform;
use crate::text::windows::TextWin;
use super::winit::WinitWindow;
use super::apple::Shared;
#[cfg(test)]
mod tests {
    use super::super::winit::WinitWindow;
}
",
            ),
            (
                "window/windows/proc.rs",
                r"use super::WinWindow;
use crate::window::State;
use crate::display::windows::DisplayWin;
",
            ),
            (
                "window/winit.rs",
                r"pub struct WinitWindow;
use crate::display::Display;
use super::apple::ios::IosWindow;
use crate::prelude::DisplayWin;
",
            ),
            (
                "window/apple/mod.rs",
                r"pub mod macos;
pub mod ios;
pub struct Shared;
pub use macos::MacWindow;
",
            ),
            (
                "window/apple/macos.rs",
                r"pub struct MacWindow;
use super::Shared;
use crate::window::State;
use super::ios::IosWindow;
",
            ),
            (
                "window/apple/ios.rs",
                "pub struct IosWindow;\nuse super::Shared;\n",
            ),
            // `display`: a model with a submodule, and platform files.
            (
                "display/mod.rs",
                r"pub mod windows;
pub mod winit;
pub mod linux;
pub mod apple;
pub mod source;
pub struct Display;
pub use windows::DisplayWin;
use crate::window::Window;
",
            ),
            (
                "display/source.rs",
                "use crate::window::State;\nuse super::windows::DisplayWin;\n",
            ),
            ("display/windows.rs", "pub struct DisplayWin;\n"),
            (
                "display/winit.rs",
                "pub struct DisplayWinit;\nuse super::linux::DisplayLinux;\n",
            ),
            ("display/linux.rs", "pub struct DisplayLinux;\n"),
            ("display/apple/mod.rs", "pub mod macos;\n"),
            (
                "display/apple/macos.rs",
                r"pub struct MacDisplay;
use crate::window::apple::Shared;
use crate::window::Window;
",
            ),
            // `text`: a model that re-exports its platform file.
            (
                "text/mod.rs",
                r"pub mod windows;
pub struct Text;
pub use crate::text::windows::TextWin;
",
            ),
            (
                "text/windows.rs",
                r"pub struct TextWin;
use crate::hosts::windows::WinHost;
use crate::display::DisplayWin;
use crate::display::Display;
",
            ),
        ],
    },
    Planted {
        name: "g",
        declaration: r#"
features = ["window", "ghost"]
platforms = ["windows", "apple", "apple", "a::b"]
groups = { apple = ["macos", "windows"], mac = ["m"] }
base = ["owner", "window", "x::y"]
hosts = "hosts"
"#,
        files: &[
            (
                "lib.rs",
                "pub mod window;\npub mod owner;\npub mod hosts;\n",
            ),
            ("window.rs", "pub struct Window;\n"),
            ("owner.rs", "pub struct Owner;\n"),
            ("hosts.rs", "pub struct Host;\n"),
        ],
    },
    Planted {
        name: "h",
        declaration: r#"
layers = [["window"]]
platforms = ["windows"]
hosts = "hosts"
"#,
        files: &[
            ("lib.rs", "pub mod window;\npub mod hosts;\n"),
            ("window.rs", "pub struct Window;\n"),
            ("hosts.rs", "pub struct Host;\n"),
        ],
    },
];

/// The finding each planted violation of the feature axis must produce, and
/// no other.
pub(in crate::module_dag) const EXPECTED: [(&str, &str, &str); 28] = [
    ("platform", "hosts", "names hosts"),
    ("owner", "hosts", "names hosts"),
    ("text::windows", "hosts", "names hosts"),
    ("owner", "window", "base names a feature"),
    ("owner", "display::apple::macos", "base names a feature"),
    ("version", "text::windows", "base names a feature"),
    (
        "window",
        "display::windows",
        "names another feature's platform file",
    ),
    (
        "window::windows",
        "text::windows",
        "names another feature's platform file",
    ),
    (
        "window::windows",
        "display::windows",
        "names another feature's platform file",
    ),
    (
        "window::winit",
        "display::windows",
        "names another feature's platform file",
    ),
    (
        "display::apple::macos",
        "window::apple",
        "names another feature's platform file",
    ),
    (
        "text::windows",
        "display::windows",
        "names another feature's platform file",
    ),
    (
        "window::windows",
        "window::winit",
        "names another platform's file",
    ),
    (
        "window::windows",
        "window::apple",
        "names another platform's file",
    ),
    (
        "window::apple::macos",
        "window::apple::ios",
        "names another platform's file",
    ),
    (
        "window::winit",
        "window::apple::ios",
        "names another platform's file",
    ),
    ("platform", "crate::Nowhere", "unattributed"),
    ("text", "text::windows", "stale exception"),
    ("window::winit", "window::windows", "stale exception"),
    ("display::wasm", "display::linux", "exception module"),
    ("ghost", "", "unknown"),
    ("window", "", "duplicate"),
    ("x::y", "", "nested"),
    ("apple", "", "duplicate"),
    ("a::b", "", "nested"),
    ("apple", "windows", "group child is a platform"),
    ("mac", "", "group not a platform"),
    ("layers", "platforms, hosts", "layers with feature keys"),
];

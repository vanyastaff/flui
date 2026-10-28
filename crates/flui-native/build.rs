//! The `cfg` conditions the core host names in many places, each spelled once.

fn main() {
    cfg_aliases::cfg_aliases! {
        // The winit host, on the targets it builds for.
        winit_host: {
            all(
                feature = "winit-backend",
                any(target_os = "windows", target_os = "macos", target_os = "linux")
            )
        },
        // The native Wayland host.
        wayland_host: { all(target_os = "linux", feature = "wayland") },
        // The native X11 host.
        x11_host: { all(target_os = "linux", feature = "x11") },
        // A Linux host whose loop reads the session's own sources: the
        // Settings portal, logind and memory pressure. A Linux host joins it
        // here by its feature.
        linux_host: {
            all(
                target_os = "linux",
                any(feature = "winit-backend", feature = "wayland", feature = "x11")
            )
        },
    }
}

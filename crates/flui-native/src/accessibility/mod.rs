//! Accessibility: the tree a window hands an assistive technology, through an
//! AccessKit adapter of its operating system (ADR-0151 §10).
//!
//! The model is `bridge`, the state an adapter shares with the handlers its
//! platform library calls: activation, the retained tree and the listeners.
//! Beside it, one file per platform holds the adapter a window hands out,
//! the type the accessibility position of its table names:
//!
//! - `windows`: UI Automation (`accesskit_windows`);
//! - `apple`: NSAccessibility on macOS (`accesskit_macos`);
//! - `linux`: AT-SPI (`accesskit_unix`), which every Linux host's windows
//!   hand out (the Wayland, X11 and winit hosts');
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters have no feature: every window of a host that has one hands it
//! out, and each compiles wherever a host of its operating system does
//! (ADR-0151 §15): on Linux, with any of the Wayland, X11 and winit hosts.
//! Android and the Web have none yet: their tables name
//! `Absent<Accessibility>`.

#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    linux_host
))]
mod bridge;
pub(crate) mod headless;
#[cfg(linux_host)]
pub(crate) mod linux;
#[cfg(windows)]
pub(crate) mod windows;

#[cfg(test)]
mod tests {
    use flui_semantics::AssistiveFocusSupport;

    /// No operating system's adapter reports where the screen reader's cursor
    /// is: until one does, a scenario that needs it fails there as gap 36
    /// (`docs/gaps.md`), and this table is where an adapter that starts to
    /// report it says so.
    #[test]
    fn no_native_adapter_reports_the_assistive_cursor_yet() {
        let adapters: &[(&str, AssistiveFocusSupport)] = &[
            #[cfg(linux_host)]
            ("AT-SPI", super::linux::UnixAccessibility::ASSISTIVE_FOCUS),
            #[cfg(windows)]
            (
                "UI Automation",
                super::windows::WindowsAccessibility::ASSISTIVE_FOCUS,
            ),
            #[cfg(target_os = "macos")]
            (
                "NSAccessibility",
                super::apple::MacosAccessibility::ASSISTIVE_FOCUS,
            ),
            #[cfg(target_os = "ios")]
            (
                "UIAccessibility",
                super::apple::IosAccessibility::ASSISTIVE_FOCUS,
            ),
        ];
        for (adapter, support) in adapters {
            assert_eq!(
                *support,
                AssistiveFocusSupport::Unreported,
                "{adapter} now reports the assistive cursor: close gap 36 for it in docs/gaps.md and docs/platform-parity.md"
            );
        }
    }
}

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
//! - `android`: TalkBack (`accesskit_android`), through `FluiView`'s node
//!   provider;
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters have no feature: every window of a host that has one hands it
//! out, and each compiles wherever a host of its operating system does
//! (ADR-0151 §15): on Linux, with any of the Wayland, X11 and winit hosts.
//! The Web has none yet: its table names `Absent<Accessibility>`.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
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

    /// Which adapters report where the screen reader's cursor is. iOS's and
    /// Android's do, through their vendored adapters; elsewhere a scenario that
    /// needs it fails as gap 36 (`docs/gaps.md`), and this table is where an
    /// adapter that starts to report it says so.
    #[test]
    fn each_native_adapter_says_whether_it_reports_the_assistive_cursor() {
        let adapters: &[(&str, AssistiveFocusSupport, AssistiveFocusSupport)] = &[
            #[cfg(linux_host)]
            (
                "AT-SPI",
                super::linux::UnixAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Unreported,
            ),
            #[cfg(windows)]
            (
                "UI Automation",
                super::windows::WindowsAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Unreported,
            ),
            #[cfg(target_os = "macos")]
            (
                "NSAccessibility",
                super::apple::MacosAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Unreported,
            ),
            #[cfg(target_os = "android")]
            (
                "TalkBack",
                super::android::AndroidAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Reported,
            ),
            #[cfg(target_os = "ios")]
            (
                "UIAccessibility",
                super::apple::IosAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Reported,
            ),
        ];
        for (adapter, support, expected) in adapters {
            assert_eq!(
                support, expected,
                "{adapter} changed what it reports of the assistive cursor: update gap 36 in docs/gaps.md and docs/platform-parity.md"
            );
        }
    }
}

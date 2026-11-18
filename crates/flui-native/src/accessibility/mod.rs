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
//!   hand out (the Wayland and X11 hosts');
//! - `android`: TalkBack (`accesskit_android`), through `FluiView`'s node
//!   provider, which answers from `provided_tree`, the adapter on the Java
//!   main thread and the end of its tree when the last service stops;
//! - `web`: a parallel ARIA DOM over the canvas, which the browser's own
//!   accessibility tree reads (ADR-0255 decision 18), with no AccessKit
//!   adapter under it;
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters have no feature: every window of a host that has one hands it
//! out, and each compiles wherever a host of its operating system does
//! (ADR-0151 §15): on Linux, with the Wayland or the X11 host.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32",
    linux_host
))]
mod bridge;
pub(crate) mod headless;
#[cfg(linux_host)]
pub(crate) mod linux;
#[cfg(any(target_os = "android", all(test, not(target_arch = "wasm32"))))]
pub(crate) mod provided_tree;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;

#[cfg(test)]
mod tests {
    use flui_semantics::AssistiveFocusSupport;
    use flui_semantics::platform::RouteChangeSupport;

    use crate::testing::gap::GapId;

    /// Which adapters report where the screen reader's cursor is. None does
    /// on the published AccessKit adapters: `accesskit_ios` and
    /// `accesskit_android` track the cursor and keep it to themselves. A
    /// scenario that needs it fails as [`GapId::AccessibilityBridgeFidelity`],
    /// and this table is where an adapter that starts to report it says so.
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
                AssistiveFocusSupport::Unreported,
            ),
            #[cfg(target_os = "ios")]
            (
                "UIAccessibility",
                super::apple::IosAccessibility::ASSISTIVE_FOCUS,
                AssistiveFocusSupport::Unreported,
            ),
        ];
        for (adapter, support, expected) in adapters {
            assert_eq!(
                support,
                expected,
                "{adapter} changed what it reports of the assistive cursor: update {} and crates/flui-native/platform-parity.toml",
                GapId::AccessibilityBridgeFidelity
            );
        }
    }

    /// The tree ends when the last assistive technology leaves, on every host
    /// whose platform says so: an activated bridge whose platform entry for
    /// that runs drops its retained tree, tells the composition root once, and
    /// a repeated notice tells nobody. A host whose platform never says so
    /// has no entry, and its column of the parity row says ❌. Red-check:
    /// leave the Android stop out of the TalkBack row's entry (the bridge
    /// stays active).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn each_native_adapter_ends_its_tree_when_its_platform_says_nobody_listens() {
        use std::sync::Arc;

        use accesskit::{ActivationHandler, TreeUpdate};
        use parking_lot::Mutex;

        use super::bridge::{Activation, BridgeShared};
        use crate::testing::parity::{Mark, source_mark};

        /// The parity row this table holds the hosts to.
        const ROW: &str = "the-tree-ends-when-no-assistive-technology-listens";

        /// The entry a platform calls when the last assistive technology
        /// leaves, or `None` where it never says.
        type Stop = Option<fn(&Arc<BridgeShared>)>;

        /// An adapter with no tree of its own to end.
        #[derive(Default)]
        struct NoTree;

        impl super::provided_tree::ProvidedAdapter for NoTree {
            type Events = ();

            fn update_if_active(&mut self, _: TreeUpdate) -> Option<()> {
                None
            }
        }

        #[cfg(any(linux_host, target_os = "ios"))]
        fn deactivation(shared: &Arc<BridgeShared>) {
            use accesskit::DeactivationHandler as _;
            super::bridge::Deactivation(Arc::clone(shared)).deactivate_accessibility();
        }

        let adapters: &[(&str, &[&str], Stop)] = &[
            #[cfg(linux_host)]
            ("AT-SPI", &["wayland", "x11"], Some(deactivation)),
            #[cfg(windows)]
            ("UI Automation", &["windows"], None),
            #[cfg(target_os = "macos")]
            ("NSAccessibility", &["macos"], None),
            #[cfg(target_os = "ios")]
            ("UIAccessibility", &["ios"], Some(deactivation)),
            (
                "TalkBack",
                &["android"],
                Some(|shared| {
                    super::provided_tree::ProvidedTree::<NoTree>::new(Arc::clone(shared))
                        .services_stopped();
                }),
            ),
            ("the Web's ARIA mirror", &["web"], None),
        ];
        for (adapter, columns, stop) in adapters {
            for column in *columns {
                let mark = source_mark(ROW, column)
                    .unwrap_or_else(|| panic!("platform-parity.toml has `{ROW}` on `{column}`"));
                assert_eq!(
                    mark == Mark::Missing,
                    stop.is_none(),
                    "{adapter}: `{ROW}` on `{column}` is {mark:?} in crates/flui-native/platform-parity.toml"
                );
            }
            let Some(stop) = stop else {
                continue;
            };
            let shared = Arc::new(BridgeShared::new());
            let heard = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&heard);
            shared.set_activation_listener(Arc::new(move |active| log.lock().push(active)));
            let _ = shared.publish(TreeUpdate {
                nodes: vec![(
                    accesskit::NodeId(1),
                    accesskit::Node::new(accesskit::Role::Window),
                )],
                tree: Some(accesskit::TreeInfo::new(accesskit::NodeId(1))),
                tree_id: accesskit::TreeId::ROOT,
                focus: accesskit::NodeId(1),
            });
            let _ = Activation(Arc::clone(&shared)).request_initial_tree();
            stop(&shared);
            stop(&shared);
            assert!(!shared.is_active(), "{adapter}: the bridge stays active");
            assert!(
                shared.retained().is_none(),
                "{adapter}: the retained tree outlives the last listener"
            );
            assert_eq!(
                *heard.lock(),
                [true, false],
                "{adapter}: what the root heard"
            );
        }
    }

    /// Which adapters raise the platform's own notice of a new screen when the
    /// innermost route changes (ADR-0204 §5). iOS's and Android's do; elsewhere
    /// the route's name is spoken as an announcement, and this table is where
    /// an adapter that gains a notice says so.
    #[test]
    fn each_native_adapter_says_how_a_route_change_is_told() {
        let adapters: &[(&str, RouteChangeSupport, RouteChangeSupport)] = &[
            #[cfg(linux_host)]
            (
                "AT-SPI",
                super::linux::UnixAccessibility::ROUTE_CHANGE,
                RouteChangeSupport::Announced,
            ),
            #[cfg(windows)]
            (
                "UI Automation",
                super::windows::WindowsAccessibility::ROUTE_CHANGE,
                RouteChangeSupport::Announced,
            ),
            #[cfg(target_os = "macos")]
            (
                "NSAccessibility",
                super::apple::MacosAccessibility::ROUTE_CHANGE,
                RouteChangeSupport::Announced,
            ),
            #[cfg(target_os = "android")]
            (
                "TalkBack",
                super::android::AndroidAccessibility::ROUTE_CHANGE,
                RouteChangeSupport::Native,
            ),
            #[cfg(target_os = "ios")]
            (
                "UIAccessibility",
                super::apple::IosAccessibility::ROUTE_CHANGE,
                RouteChangeSupport::Native,
            ),
        ];
        for (adapter, support, expected) in adapters {
            assert_eq!(
                support,
                expected,
                "{adapter} changed how it tells a route change: update {} and crates/flui-native/platform-parity.toml",
                GapId::AccessibilityBridgeFidelity
            );
        }
    }
}

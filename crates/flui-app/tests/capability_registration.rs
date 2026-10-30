//! An application's capability registrations are checked before anything
//! starts (ADR-0152 §1): two plugins for one capability, or two capabilities
//! sharing a name, fail `Application::run` before the platform is created,
//! so no window opens; a launch with them fails `run_app` the same way.

use flui_app::app::{AppRunError, Application};
use flui_app::{AppCapabilities, AppLaunch, LaunchError};
use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::capability::{
    Capability, CapabilityRegistrar, Plugin, ProviderContext, Resolver,
};
use flui_runtime::capability::{CapabilityConflict, ProviderTier};
use flui_view::prelude::*;
use flui_widgets::SizedBox;

struct Inert;

impl ClipboardBackend for Inert {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        let _ = answer.resolve(None);
    }

    fn write_text(&self, _text: String) {}
}

struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<ClipboardCapability>(|cx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, cx.completions().clone()))
        });
    }
}

enum Impostor {}

impl Capability for Impostor {
    type Handle = ();
    const NAME: &'static str = ClipboardCapability::NAME;
}

struct ImpostorPlugin;

impl Plugin for ImpostorPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<Impostor>(|_: &ProviderContext<'_>| Ok(()));
    }
}

fn application() -> Application<SizedBox, impl FnMut(&flui_app::app::AppHandle) -> SizedBox> {
    Application::new(|_: &flui_app::app::AppHandle| -> SizedBox {
        unreachable!("a conflicting application builds no window")
    })
}

pub(crate) fn conflicting_registrations_fail_run_before_the_platform_starts() {
    let two_plugins = application()
        .plugin(ClipboardPlugin)
        .plugin(ClipboardPlugin)
        .run();
    assert!(
        matches!(
            two_plugins,
            Err(AppRunError::CapabilityConflict(
                CapabilityConflict::Duplicate {
                    name: "clipboard",
                    tier: ProviderTier::Plugin,
                }
            ))
        ),
        "{two_plugins:?}"
    );

    let shared_name = application().plugin(ImpostorPlugin).run();
    assert!(
        matches!(
            shared_name,
            Err(AppRunError::CapabilityConflict(
                CapabilityConflict::SharedName { name: "clipboard" }
            ))
        ),
        "{shared_name:?}"
    );

    let two_overrides = application()
        .capability::<ClipboardCapability>(|cx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, cx.completions().clone()))
        })
        .capability::<ClipboardCapability>(|cx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, cx.completions().clone()))
        })
        .run();
    assert!(
        matches!(
            two_overrides,
            Err(AppRunError::CapabilityConflict(
                CapabilityConflict::Duplicate {
                    tier: ProviderTier::Override,
                    ..
                }
            ))
        ),
        "{two_overrides:?}"
    );
}

/// A root the conflicting launch never builds.
#[derive(Clone, StatelessView)]
struct Unbuilt;

impl StatelessView for Unbuilt {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        never_built()
    }
}

pub(crate) fn a_conflicting_launch_fails_run_app_before_the_platform_starts() {
    let launch = AppLaunch::new().capabilities(
        AppCapabilities::new()
            .plugin(ClipboardPlugin)
            .plugin(ClipboardPlugin),
    );
    let launched = flui_app::run_app(Unbuilt, launch);
    assert!(
        matches!(
            launched,
            Err(LaunchError::CapabilityConflict(
                CapabilityConflict::Duplicate {
                    name: "clipboard",
                    tier: ProviderTier::Plugin,
                }
            ))
        ),
        "{launched:?}"
    );
}

fn never_built() -> SizedBox {
    unreachable!("a conflicting launch builds no window")
}

//! An application's capability registrations are checked before anything
//! starts (ADR-0152 §1): two plugins for one capability, or two capabilities
//! sharing a name, fail `Application::run` before the platform is created,
//! so no window opens; the same holds for an application whose root is never built.

use flui_app::Application;
use flui_app::LaunchError;
use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::ClipboardRead;
use flui_platform::capability::{
    Capability, CapabilityRegistrar, Plugin, ProviderContext, Resolver,
};
use flui_runtime::capability::{CapabilityConflict, ProviderTier};
use flui_view::prelude::*;
use flui_widgets::SizedBox;

struct Inert;

impl ClipboardBackend for Inert {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let _ = answer.resolve(Ok(None));
    }

    fn write(&self, _write: flui_platform::ClipboardWrite) {}

    fn concealment(&self) -> flui_platform::Concealment {
        flui_platform::Concealment::Unavailable
    }
}

struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<ClipboardCapability>(|ctx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, ctx))
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

fn application() -> Application {
    Application::new()
}

pub(crate) fn conflicting_registrations_fail_run_before_the_platform_starts() {
    let two_plugins = application()
        .plugin(ClipboardPlugin)
        .plugin(ClipboardPlugin)
        .run(Unbuilt);
    assert!(
        matches!(
            two_plugins,
            Err(LaunchError::CapabilityConflict(
                CapabilityConflict::Duplicate {
                    name: "clipboard",
                    tier: ProviderTier::Plugin,
                }
            ))
        ),
        "{two_plugins:?}"
    );

    let shared_name = application().plugin(ImpostorPlugin).run(Unbuilt);
    assert!(
        matches!(
            shared_name,
            Err(LaunchError::CapabilityConflict(
                CapabilityConflict::SharedName { name: "clipboard" }
            ))
        ),
        "{shared_name:?}"
    );

    let two_overrides = application()
        .capability::<ClipboardCapability>(|ctx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, ctx))
        })
        .capability::<ClipboardCapability>(|ctx: &ProviderContext<'_>| {
            Ok(ClipboardHandle::new(Inert, ctx))
        })
        .run(Unbuilt);
    assert!(
        matches!(
            two_overrides,
            Err(LaunchError::CapabilityConflict(
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
    let launched = Application::new()
        .plugin(ClipboardPlugin)
        .plugin(ClipboardPlugin)
        .run(Unbuilt);
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

//! The scripted capabilities a headless host provides when it is given no registry,
//! and the background lanes it runs on the test's thread.

#[cfg(doc)]
use super::HeadlessHost;

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::scripted::ScenarioDriver;

use flui_runtime::window_system::WindowSystem;

use super::clock::HostClock;

/// The capabilities a headless host provides when it is given no registry,
/// each scripted by the test, so no test reaches the network: what a runner
/// provides on the platform the primary window names, with the features the
/// test's build has. The clipboard and the background lanes always; the
/// primary selection only where the platform has one (Linux, as X11 and
/// Wayland have it); HTTP only with this crate's `http` feature, which the
/// facade's `http` turns on with the runner's. All of them are on the host's
/// scenario, whose clock the host alone drives: HTTP reports an unscripted
/// URL there.
pub(super) struct ScriptedBuiltIns {
    pub(super) clock: HostClock,
    pub(super) clipboard: crate::clipboard::ScriptedClipboard,
    pub(super) primary_selection: crate::primary_selection::ScriptedPrimarySelection,
    pub(super) background: HeadlessBackground,
    #[cfg(feature = "http")]
    pub(super) http: flui_platform::testing::ScriptedHttp,
}

impl ScriptedBuiltIns {
    /// The built-ins on the scenario `driver` moves.
    pub(super) fn on(driver: ScenarioDriver) -> Self {
        let scenario = driver.scenario();
        #[cfg(feature = "http")]
        let http = flui_platform::testing::ScriptedHttp::default();
        #[cfg(feature = "http")]
        http.report_to(scenario.failure_report());
        let clipboard = crate::clipboard::ScriptedClipboard::on(scenario);
        let primary_selection = crate::primary_selection::ScriptedPrimarySelection::on(scenario);
        Self {
            clock: HostClock {
                driver: Rc::new(driver),
            },
            clipboard,
            primary_selection,
            background: HeadlessBackground::default(),
            #[cfg(feature = "http")]
            http,
        }
    }

    /// The providers a runner ships on `platform`, scripted, its widgets
    /// reading `assets`, with `storage` as the host's storage.
    pub(super) fn built_ins(
        &self,
        windows: WindowSystem,
        assets: &flui_platform::assets::AssetBundle,
        storage: impl flui_platform::capability::CapabilityProvider<
            flui_view::persist::StorageCapability,
        >,
    ) -> flui_runtime::capability::BuiltIns {
        let assets = assets.clone();
        let built_ins = flui_runtime::capability::BuiltIns::new(
            windows,
            self.clipboard.clone(),
            self.background.provider(),
            storage,
            move |_: &flui_platform::capability::ProviderContext<'_>| Ok(assets.clone()),
        )
        .primary_selection(self.primary_selection.clone());
        #[cfg(feature = "http")]
        let built_ins = {
            let http = self.http.handle();
            built_ins
                .http(move |_: &flui_platform::capability::ProviderContext<'_>| Ok(http.clone()))
        };
        built_ins
    }
}

/// The host's background lanes: deterministic, on the test's thread, run
/// by [`HeadlessHost::pump`] before each frame unless held.
#[derive(Clone)]
pub(super) struct HeadlessBackground {
    pub(super) executors: flui_runtime::execution::DeterministicExecutors,
    /// The lanes as a widget reaches them, with the admission windows and
    /// shutdown of a hosted run.
    services: Arc<flui_runtime::execution::ExecutionServices>,
    pub(super) held: Rc<std::cell::Cell<bool>>,
}

impl Default for HeadlessBackground {
    fn default() -> Self {
        let executors = flui_runtime::execution::DeterministicExecutors::new();
        let services = Arc::new(flui_runtime::execution::ExecutionServices::with_host(
            executors.host_executors(),
        ));
        Self {
            executors,
            services,
            held: Rc::default(),
        }
    }
}

impl HeadlessBackground {
    /// The provider of the lanes a widget acquires.
    fn provider(
        &self,
    ) -> impl flui_platform::capability::CapabilityProvider<flui_view::background::BackgroundCapability>
    {
        let services = Arc::clone(&self.services);
        move |ctx: &flui_platform::capability::ProviderContext<'_>| {
            Ok(flui_view::background::Background::new(
                Arc::clone(&services) as Arc<dyn flui_platform::executor::Executor>,
                ctx.completions().clone(),
            ))
        }
    }

    /// Queued background jobs and futures not yet finished.
    pub(super) fn pending(&self) -> usize {
        let (jobs, futures) = self.executors.pending();
        jobs + futures
    }

    /// Run queued work until none is runnable, unless held.
    pub(super) fn drain_unless_held(&self) {
        if !self.held.get() {
            let _ran = self.executors.run_until_idle();
        }
    }
}

//! The host executor reaches capability providers of every presentation.

use std::any::TypeId;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use flui_foundation::PresentationId;
use flui_platform::capability::{Capability, CapabilityRegistrar, ProviderContext};
use flui_platform::executor::{ComputeJob, Executor, IoFuture, Lane, RoomFuture, SpawnError};
use flui_runtime::capability::CapabilityRegistry;
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

struct UnavailableExecutor;

impl Executor for UnavailableExecutor {
    fn spawn_compute(&self, _job: ComputeJob) -> Result<(), SpawnError> {
        Err(SpawnError::Unavailable)
    }

    fn spawn_io(&self, _future: IoFuture) -> Result<(), SpawnError> {
        Err(SpawnError::Unavailable)
    }

    fn room(&self, _lane: Lane) -> RoomFuture {
        RoomFuture::ready(Err(SpawnError::Unavailable))
    }
}

enum ExecutorCapability {}

impl Capability for ExecutorCapability {
    type Handle = Option<Arc<dyn Executor>>;
    const NAME: &'static str = "executor-probe";
}

fn registry() -> CapabilityRegistry {
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<ExecutorCapability>(|cx: &ProviderContext<'_>| Ok(cx.executor().cloned()));
    CapabilityRegistry::builder()
        .overrides(registrar)
        .build()
        .expect("one provider")
}

fn runtime(executor: Option<Arc<dyn Executor>>) -> UiRuntime {
    let fonts = flui_painting::FontCollection::new();
    let services = RuntimeHostServices::new(
        Arc::new(|| {}),
        Arc::new(AtomicBool::new(false)),
        &fonts,
        flui_scheduler::ClockSource::Platform,
    )
    .with_capabilities(registry());
    let services = match executor {
        Some(executor) => services.with_executor(executor),
        None => services,
    };
    UiRuntime::try_new(crate::owner_publication::window(), 1.0, services).expect("runtime")
}

fn acquire(runtime: &UiRuntime, presentation: PresentationId) -> Option<Arc<dyn Executor>> {
    let acquired = runtime.enter(|runtime| {
        runtime
            .presentation_widgets_for_test(presentation)
            .with_build_owner(|owner| {
                owner
                    .capabilities()
                    .capabilities
                    .as_ref()
                    .expect("a presentation installs its capability source")
                    .acquire(TypeId::of::<ExecutorCapability>(), ExecutorCapability::NAME)
            })
    });
    *acquired
        .expect("the probe provider answers")
        .downcast::<Option<Arc<dyn Executor>>>()
        .expect("the provider answers its capability's handle")
}

#[test]
fn the_same_host_executor_reaches_initial_and_secondary_presentation_providers() {
    let executor: Arc<dyn Executor> = Arc::new(UnavailableExecutor);
    let expected = Arc::clone(&executor);
    let mut runtime = runtime(Some(executor));

    let primary = acquire(&runtime, runtime.presentation_id())
        .expect("the initial presentation receives the host executor");
    assert!(Arc::ptr_eq(&primary, &expected));

    let secondary = runtime
        .presentation_factory()
        .assemble(crate::owner_publication::window())
        .expect("assemble secondary presentation");
    let secondary = runtime.install_presentation(secondary);
    let secondary = acquire(&runtime, secondary)
        .expect("the secondary presentation receives the host executor");
    assert!(Arc::ptr_eq(&secondary, &expected));
}

#[test]
fn a_runtime_without_a_host_executor_gives_providers_none() {
    let runtime = runtime(None);
    assert!(
        acquire(&runtime, runtime.presentation_id()).is_none(),
        "no hidden executor is created for the provider"
    );
}

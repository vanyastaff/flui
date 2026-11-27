//! What the loop builds from an [`Application`](crate::Application), held as
//! one value (ADR-0393 §4).
//!
//! A development restart drops this value and builds it again from the
//! application its entry hands over. What the loop holds outside it, the
//! host and what the host made before any application code ran (the owner
//! platform, the execution pools, the engine services, the clipboard, the
//! development agents), is kept. Adding state the application configures
//! means adding it here, so that a restart rebuilds it with no rule of its
//! own.

use std::cell::RefCell;

#[cfg(not(target_arch = "wasm32"))]
use crate::app::lifecycle::ServiceRegistry;

use super::FontRegistrations;

/// The loop's state built from the running application.
///
/// The fields are declared in the reverse of the order a desktop run's start
/// builds them (`runner::main_window`), so the scope drops them in reverse:
/// the services first, the main window's controller before the placements
/// and the storage its windows write, the ingress its handles send through
/// last. The fonts the application registers at any time drop beside the
/// controller.
#[derive(Default)]
pub(crate) struct ApplicationScope {
    /// The running application services: the named owner of every
    /// application-lifetime unit of background work. Consulted by
    /// `AppRuntime::should_exit` (a running keep-alive service vetoes exit)
    /// and shut down, cancel then deadline-bounded join, before the pools
    /// that run them close. The pools are the loop's: a host runs its own IO
    /// on them (ADR-0327 §3).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) service_registry: ServiceRegistry,
    /// The application's main window: its factory and installer, its
    /// configuration and its observers.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(in crate::app) main_controller: Option<crate::app::runner::main_window::MainController>,
    /// The fonts the application registered (`AppRuntime::register_font`),
    /// and those still waiting for the loop's engine services.
    pub(super) fonts: RefCell<FontRegistrations>,
    /// The run's window placements: where the host acquires the storage they
    /// are kept in, the windows that remember one and the writes still out
    /// (`runner::placement`).
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(in crate::app) placement: crate::app::runner::placement::PlacementHost,
    /// The flush registry over [`Self::host_storage`], kept outside every UI
    /// runtime: what their documents publish, written on the loop's IO pool
    /// and flushed when the application or the session ends.
    pub(in crate::app) flush_host: Option<flui_view::__runtime::FlushHost>,
    /// The byte storage every UI runtime this loop builds hands its widgets,
    /// resolved from the run's configuration when the host starts
    /// (`AppRuntime::install_host_storage`).
    pub(in crate::app) host_storage: crate::app::capability::StorageSlot,
    /// The providers every UI runtime this loop builds acquires platform
    /// capabilities from (ADR-0152): the application's, checked for conflicts
    /// before the loop starts, or the built-ins alone when a runner starts
    /// without an [`Application`](crate::Application).
    pub(in crate::app) capabilities: Option<flui_runtime::capability::CapabilityRegistry>,
    /// The application-control ingress every `AppHandle` of this application
    /// sends through.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(in crate::app) main_ingress:
        Option<std::sync::Arc<crate::app::application_control::Ingress>>,
}

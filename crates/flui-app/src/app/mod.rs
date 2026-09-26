//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppRuntime` (`runtime.rs`) - the loop-scoped composition root
//! - `UiRealm` - Owns one owner-affine widget session (build/render/gesture
//!   state, the frame pipeline, and the per-presentation semantics/haptics/
//!   clipboard surfaces retired from the former `AppBinding`)
//! - `AppConfig` - Application configuration
//!
//! Application lifecycle state is `flui_scheduler::AppLifecycleState`;
//! the runner drives the scheduler directly.

pub(crate) mod close_request;
mod config;
pub mod direct;
pub(crate) mod execution;
mod frame_failure;
pub(crate) mod hot_reload;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod lifecycle;
pub(crate) mod logging;
pub(crate) mod media_query_root;
mod performance_stats;
pub(crate) mod presentation;
pub(crate) mod presentation_forest;
pub(crate) mod raster_lane;
#[cfg(test)]
pub(crate) mod raster_test_support;
pub mod runner;
pub(crate) mod runtime;
pub(crate) mod ui_realm;
pub(crate) mod window_registry;
#[cfg(test)]
pub(crate) mod window_test_support;

pub use close_request::{CloseRequest, CloseRequestError, CloseRequestHandler, CloseResponse};
pub use config::{AppConfig, DiagnosticsProfile};
pub use direct::run_direct;
pub use execution::{
    ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
    SpawnError,
};
pub use frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport, PanicText, SegmentPhase,
};
#[cfg(not(target_arch = "wasm32"))]
pub use lifecycle::{
    CancellationSignal, JoinTimeout, PublishError, ServiceContext, ServiceDefinition,
    ServiceEvents, ServiceFuture, ServiceLifetime, ServicePublisher, ServiceStartError,
    TaskContext, TaskHandle, TaskOutcome, TaskSpawner, WorkerGeneration, WorkerHandle,
    service_events,
};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use runner::open_secondary_window;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use runner::open_window;
#[cfg(not(target_os = "ios"))]
pub use runner::request_presentation_close;
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};
#[cfg(not(target_os = "ios"))]
pub use runtime::{ExitPolicy, WindowPolicy};

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{LifecycleHook, RecoveredAt};
pub use flui_view::{RootRenderElement, RootRenderView};

mod lifecycle_state;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod application;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) mod application_control;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use application::{AppRunError, Application, StartupWindow};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use application_control::{AppControlError, AppHandle, AppWindowError, MainWindowRequest};

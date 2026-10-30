//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppRuntime` (`runtime.rs`) - the loop-scoped composition root
//! - `UiRuntime` - Owns one owner-affine widget session (build/render/gesture
//!   state, the frame pipeline, and the per-presentation semantics/haptics/
//!   clipboard surfaces retired from the former `AppBinding`)
//! - `AppConfig` - Application configuration
//!
//! Application lifecycle state is `flui_scheduler::AppLifecycleState`;
//! the runner drives the scheduler directly.

pub(crate) mod close_request;
mod config;
pub(crate) mod dev_agent;
pub mod direct;
pub(crate) mod host_memory;
pub(crate) mod hot_reload;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod lifecycle;
pub(crate) mod logging;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) mod placement_recorder;
pub(crate) mod raster_lane;
pub mod runner;
pub(crate) mod runtime;
pub(crate) mod storage_host;
pub(crate) mod window_control;
pub(crate) mod window_registry;
// The ui_runtime core lives in the frame runtime (ADR-0083); these aliases keep
// its `crate::app::…` paths for the runners and the dispatch layer that
// drive it from here.
pub(crate) use flui_runtime::{lifecycle_state, presentation, ui_runtime};
#[cfg(test)]
pub(crate) mod window_test_support;

pub use close_request::{
    CloseReason, CloseRequest, CloseRequestError, CloseRequestHandler, CloseResponse,
};
pub use config::{AppConfig, DiagnosticsProfile, MotionOverride, PointerResampling};
pub use dev_agent::DevAgent;
pub use direct::run_direct;
pub use flui_runtime::frame_failure::{
    DeviceRecovery, FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport, PanicText, SamplingFailure, SamplingFailureOrigin, SamplingStage,
    SegmentPhase,
};
pub use hot_reload::DevReload;
// The recording hook (ADR-0215 §1): the tap and the records it observes.
pub use flui_runtime::owner::{InputOutcome, WindowObservation};
pub use flui_runtime::{
    DecisionRow, HostInput, InputRecord, InputRow, InputTap, InputTapFactory, PreferencesRow,
    RecordingStart, RecordingWindow,
};
#[cfg(not(target_arch = "wasm32"))]
pub use lifecycle::{
    CancellationSignal, JoinTimeout, PublishError, ServiceContext, ServiceDefinition,
    ServiceEvents, ServiceFuture, ServiceLifetime, ServicePublisher, ServiceStartError,
    TaskContext, TaskHandle, TaskOutcome, TaskSpawner, WorkerGeneration, WorkerHandle,
    service_events,
};
pub use runner::EXIT_CODE_HUNG;
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
pub use runner::{FontRegistrationError, register_font};
#[cfg(not(target_os = "ios"))]
pub use runner::{request_presentation_close, window_handle};
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};
#[cfg(not(target_os = "ios"))]
pub use runtime::{ExitPolicy, WindowPolicy};

// The execution services live in the frame runtime (ADR-0083); their public
// paths stay `flui_app::…`.
pub use flui_runtime::execution::{
    ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
    SpawnError,
};

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{LifecycleHook, RecoveredAt};
pub use flui_view::{RootRenderElement, RootRenderView};

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
pub use application_control::{AppControlError, AppHandle, AppWindowError, WindowRequest};

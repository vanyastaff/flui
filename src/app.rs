//! What an application sets up and hands to the runtime: its entry, its configuration, the work
//! it runs off the frame, how its windows close, and what it does when a frame fails.
//!
//! An application starts from [`run_app`] or an [`Application`] built and run from a
//! `#[flui::main]` function. The bindings, the root render view and the
//! embedder are the runtime's composition roots and are not part of this module: a widget
//! reaches the presentation through [`crate::view::BuildContext`] and its lifecycle hooks.

pub use flui_app::app::DevReload;
#[cfg(not(target_os = "ios"))]
pub use flui_app::run_direct;
pub use flui_app::{
    AppConfig, AppId, AppIdentity, Application, BackendFailure, DiagnosticsProfile, EXIT_CODE_HUNG,
    FontRegistrationError, FrameLatency, GpuBackend, LaunchError, MotionOverride,
    PointerResampling, UnsupportedSystem, main, register_font, run_app,
};

/// The entry macro's seam: `#[flui::main]` expands to calls into it.
#[doc(hidden)]
pub use flui_app::__entry;

// Work off the frame: compute and I/O pools, and on native targets services and tasks.
#[cfg(not(target_arch = "wasm32"))]
pub use flui_app::{
    CancellationSignal, JoinTimeout, PublishError, ServiceContext, ServiceDefinition,
    ServiceEvents, ServiceFuture, ServiceLifetime, ServicePublisher, ServiceStartError,
    TaskContext, TaskHandle, TaskOutcome, TaskSpawner, WorkerGeneration, WorkerHandle,
    service_events,
};
pub use flui_app::{
    ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
    SpawnError,
};

// A frame that fails, and the hooks around the application's lifecycle.
pub use flui_app::{
    DeviceRecovery, FailureDisposition, FrameFailureDetail, FrameFailureHandler,
    FrameFailureHandlerFactory, FrameFailureKind, FrameFailureReport, LifecycleHook, PanicText,
    RecoveredAt, SamplingFailure, SamplingFailureOrigin, SamplingStage, SegmentPhase,
};

// Closing windows and ending the application.
#[cfg(not(target_os = "ios"))]
pub use flui_app::ExitPolicy;
#[cfg(not(target_os = "ios"))]
pub use flui_app::{
    CloseReason, CloseRequest, CloseRequestError, CloseRequestHandler, CloseResponse,
    request_presentation_close, window_handle,
};

// Windows an application opens and controls while it runs.
#[cfg(all(not(target_os = "ios"), feature = "unstable-windows"))]
pub use flui_app::WindowPolicy;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::{AppControlError, AppHandle, AppWindowError, WindowRequest};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    feature = "unstable-windows"
))]
pub use flui_app::{open_secondary_window, open_window};

// The input tap a development build installs through `AppConfig::input_tap`, and the rows it
// receives.
pub use flui_app::{
    DecisionRow, HostInput, InputOutcome, InputRecord, InputRow, InputTap, InputTapFactory,
    PreferencesRow, RecordingStart, RecordingWindow, WindowObservation,
};

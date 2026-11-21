//! The hooks the host's table fills for the Wayland loop: the displays and the sources of the host's notices.

use std::path::Path;
use std::sync::Arc;

use flui_platform::DisplaySnapshot;

use super::{LastInput, OutputName, WaylandClipboard, WaylandDrag, WaylandInput};
use crate::error::PlatformError;
use crate::executor::NativeExecutor;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::linux::wayland::outputs::OnOutputs;
use crate::owner::linux::{LinuxNotices, LinuxPreferences};
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::signal::OwnerSignal;
use crate::owner::watch::Watch;
use crate::transfer::linux::wayland::DataDevices;

/// What the host's table fills for the loop (ADR-0151 §10): the
/// displays, the preference source, the notice channel and the sources
/// of the host's notices. The loop reaches each through the hook here
/// and names no feature.
pub(crate) trait WaylandHost: 'static {
    /// The arguments the application was launched with, without its own
    /// name: what a registered scheme or file type hands it
    /// ([`launch_arguments`](crate::owner::launch_arguments)).
    fn launch_arguments() -> Vec<std::ffi::OsString>;

    /// The displays: the outputs of the compositor at `socket`, the one
    /// the loop connects to, read on a connection of their own.
    fn displays(socket: &Path) -> Arc<dyn WaylandDisplays>;

    /// The seats' input, made on the owner thread, which alone reaches
    /// it, recording each user input it reports in `last_input`, which
    /// the windows' activation tokens name.
    fn input(last_input: LastInput) -> Arc<dyn WaylandInput>;

    /// The clipboard, made on the owner thread, which sets its selection
    /// under the user input `last_input` records last.
    fn clipboard(last_input: LastInput, devices: DataDevices) -> Arc<dyn WaylandClipboard>;

    /// Drag and drop over and from the windows, made on the owner thread,
    /// hearing the drags through `devices`, starting sessions under the
    /// pointer grabs `last_input` holds, and calling `wake` to have the
    /// loop send what another thread asked of a drag.
    fn drag(
        devices: DataDevices,
        last_input: LastInput,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Arc<dyn WaylandDrag>;

    /// The channel the loop's system notices take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The Settings portal's preferences, observed on `executor` and
    /// waking the owner behind `signal`; started as the loop runs.
    fn preferences(
        executor: &NativeExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn LinuxPreferences>;

    /// The sources of the host's system notices (logind, the kernel's
    /// memory pressure), started once the owner can deliver what they
    /// queue.
    fn host_notices(executor: &NativeExecutor, signal: &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>;
}

/// The compositor's outputs as the loop and its windows reach them.
pub(crate) trait WaylandDisplays: StaleOnWake {
    /// The snapshot: the last one published while nothing changed, the
    /// outputs read again otherwise.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the compositor cannot be read.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    /// The outputs of `entered` that are `live`, read from one snapshot
    /// (`OnOutputs::read`); an output the snapshot does not hold yet
    /// marks it stale.
    fn on_outputs(&self, entered: &[OutputName], live: &dyn Fn(OutputName) -> bool) -> OnOutputs;

    /// Keep the outputs current, waking the owner behind `signal` when
    /// they change, until the watch is dropped.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
}

//! The storage position of the headless host: a disk root a test names, or
//! values in memory, never the user's data directories.

use flui_platform::AppDataDir;

use super::{DataDirs, DataStore};
use crate::host::{self, Host};
use crate::owner::headless::HeadlessLoop;
use crate::platform::HostPlatform;

/// Where a headless platform keeps an application's values: under the root
/// [`HostPlatform::with_data_root`] names, else in the platform's memory.
#[derive(Debug)]
pub struct HeadlessData;

impl host::Storage<HeadlessLoop> for HeadlessData {
    fn data_store(owner: &HeadlessLoop, app: &AppDataDir) -> Option<DataStore> {
        owner.with_state(|state| {
            Some(match &state.data_root {
                Some(root) => DataStore::on_disk(DataDirs {
                    roaming: root.join("roaming").join(app.as_str()),
                    local: root.join("local").join(app.as_str()),
                }),
                None => DataStore::in_memory(state.memory.entry(app.clone()).or_default().clone()),
            })
        })
    }
}

impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Keep each application's values on disk under `root`, in
    /// `<root>/roaming/<app>` and `<root>/local/<app>`, for a test that reads
    /// back the bytes written. Without it the values stay in this platform's
    /// memory for as long as it lives; a headless platform never writes to
    /// the user's data directories.
    #[must_use]
    pub fn with_data_root(self, root: impl Into<std::path::PathBuf>) -> Self {
        let root: std::path::PathBuf = root.into();
        self.owner()
            .with_state(|state| state.data_root = Some(root));
        self
    }
}

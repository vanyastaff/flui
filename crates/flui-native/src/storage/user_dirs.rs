//! The storage position of a host whose process has the current user's data
//! directories.

use flui_platform::AppDataDir;

use super::DataStore;

/// The current user's data directories ([`data_dirs`](super::data_dirs)):
/// where every host but Android keeps an application's data.
#[derive(Debug)]
pub struct UserDataDirs;

impl<O> crate::host::Storage<O> for UserDataDirs {
    fn data_store(_: &O, app: &AppDataDir) -> Option<DataStore> {
        super::data_dirs(app).map(DataStore::on_disk)
    }
}

//! The storage position of a host whose process has the current user's data
//! directories.

use flui_platform::AppDataDir;

use super::DataStore;

/// The current user's data directories ([`data_dirs`](super::data_dirs)):
/// where every host but Android keeps an application's data, in the
/// process's container when it runs in one. Natively the folders are found
/// at the store's first call, on the thread the host runs it on: the owner
/// thread reads no file for them.
#[derive(Debug)]
pub struct UserDataDirs;

impl<O> crate::host::Storage<O> for UserDataDirs {
    fn data_store(_: &O, app: &AppDataDir) -> Option<DataStore> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Some(DataStore::of_process(app.clone()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            super::data_dirs(app).map(DataStore::on_disk)
        }
    }
}

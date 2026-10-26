//! The storage position of a host whose process has the current user's data
//! directories.

use flui_platform::StorageName;

use super::DataDirs;

/// The current user's data directories ([`data_dirs`](super::data_dirs)):
/// where every host but Android keeps an application's data.
#[derive(Debug)]
pub struct UserDataDirs;

impl<O> crate::host::Storage<O> for UserDataDirs {
    fn data_dirs(_: &O, app: &StorageName) -> Option<DataDirs> {
        super::data_dirs(app)
    }
}

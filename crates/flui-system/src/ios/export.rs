//! The file a save exports: iOS's picker places an existing file, so a
//! save's contents are first written to a file of the name the request
//! gives it, in a folder of its own in the application's temporary
//! directory. The folder goes when the export ends, whichever way: the
//! picker moved the file out, the user cancelled, the dialog was dropped.
//! Both the write and the removal run off the main thread (ADR-0360 §2).

use std::path::{Path, PathBuf};

use flui_platform::SystemFailure;

use super::document_access::io_failure;
use crate::dialog::DialogError;

/// How many folders an export tries in the temporary directory before it
/// gives up: one is taken only by another export of this process, or left
/// by a process of the same id that ended before it removed its own.
const MAX_EXPORT_FOLDERS: u32 = 16;

/// `EEXIST`, Darwin's: every folder an export tried is taken.
const EEXIST: i32 = 17;

/// Write `contents` to a file named `name` in a new folder of
/// `temporary`'s, for the picker to export: the folder and the file. A
/// write that fails leaves no folder behind.
pub(super) fn write_export(
    temporary: &Path,
    name: &str,
    contents: &[u8],
) -> Result<(PathBuf, PathBuf), DialogError> {
    for attempt in 0..MAX_EXPORT_FOLDERS {
        let folder = temporary.join(format!("flui-export-{}-{attempt}", std::process::id()));
        match std::fs::create_dir(&folder) {
            Ok(()) => {
                let file = folder.join(name);
                return match std::fs::write(&file, contents) {
                    Ok(()) => Ok((folder, file)),
                    Err(error) => {
                        // The write's failure is the answer; the clean-up's
                        // is not.
                        let _ = std::fs::remove_dir_all(&folder);
                        Err(io_failure(&error))
                    }
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_failure(&error)),
        }
    }
    Err(DialogError::Failed(SystemFailure::Os(EEXIST)))
}

#[cfg(target_os = "ios")]
pub(super) use dispatched::ExportFile;

#[cfg(target_os = "ios")]
mod dispatched {
    use std::path::{Path, PathBuf};

    use super::write_export;
    use crate::dialog::DialogError;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// A save's file in its folder of the temporary directory; the folder
    /// is removed, on a global queue, when this drops.
    pub(in crate::ios) struct ExportFile {
        folder: PathBuf,
        file: PathBuf,
    }

    impl ExportFile {
        /// Write `contents` to a file named `name` in the application's
        /// temporary directory. Off the main thread only.
        pub(in crate::ios) fn write(name: &str, contents: &[u8]) -> Result<Self, DialogError> {
            write_export(&std::env::temp_dir(), name, contents)
                .map(|(folder, file)| Self { folder, file })
        }

        /// Where the file is.
        pub(in crate::ios) fn path(&self) -> &Path {
            &self.file
        }
    }

    impl Drop for ExportFile {
        fn drop(&mut self) {
            use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier};

            let folder = std::mem::take(&mut self.folder);
            DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
                DispatchQoS::Utility,
            ))
            .exec_async(move || {
                os_entry(
                    "flui-system export removal",
                    (),
                    AfterPanic::Answer(|()| ()),
                    // A folder gone already is no failure to report.
                    move || drop(std::fs::remove_dir_all(folder)),
                );
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::write_export;
    use crate::dialog::DialogError;

    #[test]
    fn an_export_is_written_to_a_folder_of_its_own_and_a_failed_one_leaves_none() {
        let temporary = tempfile::tempdir().expect("a test folder");
        let (first, file) = write_export(temporary.path(), "notes.txt", b"hello").expect("written");
        assert_eq!(std::fs::read(&file).expect("the export"), b"hello");
        assert_eq!(file.parent(), Some(first.as_path()));
        let (second, _) = write_export(temporary.path(), "notes.txt", b"again").expect("written");
        assert_ne!(first, second, "a second export takes a folder of its own");

        // A name the file system takes no file under: the write fails, and
        // its folder goes with it.
        let before = std::fs::read_dir(temporary.path())
            .expect("the folder lists")
            .count();
        let failed = write_export(temporary.path(), "missing/notes.txt", b"lost");
        assert!(
            matches!(failed, Err(DialogError::Failed(SystemFailure::Os(_)))),
            "{failed:?}"
        );
        let after = std::fs::read_dir(temporary.path())
            .expect("the folder lists")
            .count();
        assert_eq!(after, before, "a failed write leaves no folder behind");
    }
}

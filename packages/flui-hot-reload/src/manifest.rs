//! The worker's sidecar manifest: which staged image to load, and the version
//! `flui run` minted for it (ADR-0393 §2).
//!
//! `flui run` writes it beside the canonical worker library after it stages a
//! build; the worker driver reads it before each load. The CLI is published
//! without this package, so it writes the same JSON with a private struct of
//! its own, and its test `the_worker_manifest_is_the_one_flui_hot_reload_reads`
//! reads what it wrote through [`WorkerManifest::read`]: this type is the one
//! statement of the format.

use std::{
    io,
    path::{Path, PathBuf},
};

use flui_sdk::view::dev_reload::{CodeVersion, LayoutEpoch, Revision};
use serde::{Deserialize, Serialize};

/// The sidecar `flui run` writes beside a worker library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerManifest {
    /// The staged image to load in place of the canonical library.
    pub path: PathBuf,
    /// The build's layout epoch, fixed for a `flui run` session.
    pub layout: u64,
    /// The build's revision, new for every staged build.
    pub revision: u64,
}

impl WorkerManifest {
    /// The manifest's file name, beside the canonical library.
    pub const FILE_NAME: &str = ".flui_worker_plugin";

    /// Where the manifest of the canonical library `lib_path` lives.
    #[must_use]
    pub fn beside(lib_path: &Path) -> PathBuf {
        lib_path.parent().map_or_else(
            || PathBuf::from(Self::FILE_NAME),
            |dir| dir.join(Self::FILE_NAME),
        )
    }

    /// Read the manifest of the canonical library `lib_path`. `Ok(None)` when
    /// there is none.
    ///
    /// # Errors
    ///
    /// The manifest exists but cannot be read, or is not this format.
    pub fn read(lib_path: &Path) -> io::Result<Option<Self>> {
        let bytes = match std::fs::read(Self::beside(lib_path)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        serde_json::from_slice::<Self>(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// The version this manifest names; `None` when a field is zero, which
    /// no tool mints.
    #[must_use]
    pub fn version(&self) -> Option<CodeVersion> {
        Some(CodeVersion::new(
            LayoutEpoch::try_new(self.layout)?,
            Revision::try_new(self.revision)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_reads_back_with_its_version() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let lib = dir.path().join("libworker.so");
        assert_eq!(WorkerManifest::read(&lib).expect("no manifest"), None);

        let manifest = WorkerManifest {
            path: dir.path().join("libworker-hot-1.so"),
            layout: 1,
            revision: 2,
        };
        let json = serde_json::to_vec(&manifest).expect("a manifest serializes");
        std::fs::write(WorkerManifest::beside(&lib), json).expect("the manifest is written");
        let read = WorkerManifest::read(&lib)
            .expect("the manifest reads")
            .expect("the manifest is there");
        assert_eq!(read, manifest);
        let version = read.version().expect("both fields are non-zero");
        assert_eq!((version.layout.get(), version.revision.get()), (1, 2));

        std::fs::write(
            WorkerManifest::beside(&lib),
            dir.path().as_os_str().as_encoded_bytes(),
        )
        .expect("an older plain-path manifest is written");
        assert!(
            WorkerManifest::read(&lib).is_err(),
            "the plain-path form of earlier releases is refused"
        );
        assert_eq!(
            WorkerManifest {
                revision: 0,
                ..manifest
            }
            .version(),
            None
        );
    }
}

//! [`DirectoryAssets`]: an [`AssetSource`] over a directory of files.

use std::fmt;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::Poll;

use super::{AssetFuture, AssetKey, AssetLoadError, AssetSource};

/// How many bytes one poll of a read takes from the file.
const READ_CHUNK: usize = 64 * 1024;

/// Reads asset `key` from the file `<root>/<key>`: the `flui_assets/`
/// directory beside a desktop executable or inside an Apple bundle, the
/// project root of a development run, or a test's own directory
/// (ADR-0384 §3–§5).
///
/// A read opens only a regular file, checks its length against the limit
/// before it allocates, and reads it one chunk per poll, yielding between
/// chunks, so a dropped read stops at the next one.
#[derive(Clone)]
pub struct DirectoryAssets {
    root: Arc<Path>,
}

impl DirectoryAssets {
    /// A source whose asset `key` is the file `<root>/<key>`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::from(root.into()),
        }
    }

    /// The directory the keys are joined to.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file asset `key` is read from. An [`AssetKey`] has no empty,
    /// `.` or `..` segment and no `\` or `:`, so the path stays under the
    /// root on every platform.
    fn path_of(&self, key: &AssetKey) -> PathBuf {
        let mut path = self.root.to_path_buf();
        path.extend(key.segments());
        path
    }
}

impl AssetSource for DirectoryAssets {
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>> {
        let path = self.path_of(key);
        let key = key.clone();
        Box::pin(read_in_chunks(path, key, limit))
    }
}

impl fmt::Debug for DirectoryAssets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectoryAssets")
            .field("root", &self.root)
            .finish()
    }
}

async fn read_in_chunks(
    path: PathBuf,
    key: AssetKey,
    limit: u64,
) -> Result<Vec<u8>, AssetLoadError> {
    let failed = |error: std::io::Error| AssetLoadError::from_io(&key, error);
    let not_a_file = || AssetLoadError::NotAFile { key: key.clone() };
    let too_large = || AssetLoadError::TooLarge {
        key: key.clone(),
        limit,
    };
    // Opening a FIFO or a device can block until another party acts, and a
    // read runs on a shared IO lane: only a regular file is opened, checked
    // again once open in case the path was swapped in between.
    if !std::fs::metadata(&path).map_err(failed)?.is_file() {
        return Err(not_a_file());
    }
    let mut file = File::open(&path).map_err(failed)?;
    let metadata = file.metadata().map_err(failed)?;
    if !metadata.is_file() {
        return Err(not_a_file());
    }
    let length = metadata.len();
    if length > limit {
        return Err(too_large());
    }
    // The metadata length is a hint, not a bound: the file can grow while it
    // is read, so every chunk is checked against the limit.
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        let read = match file.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(failed(error)),
        };
        let total = u64::try_from(bytes.len().saturating_add(read)).unwrap_or(u64::MAX);
        if total > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk[..read]);
        yield_now().await;
    }
}

/// Pending once, waking itself: whoever polls the read gets a turn to drop
/// it between chunks.
fn yield_now() -> impl Future<Output = ()> {
    let mut yielded = false;
    std::future::poll_fn(move |ctx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            ctx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

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
    let too_large = || AssetLoadError::TooLarge {
        key: key.clone(),
        limit,
    };
    let mut file = open_regular_file(&path)
        .map_err(failed)?
        .ok_or_else(|| AssetLoadError::NotAFile { key: key.clone() })?;
    let length = file.metadata().map_err(failed)?.len();
    if length > limit {
        return Err(too_large());
    }
    // The metadata length is a hint, not a bound: the file can grow while it
    // is read, so every chunk is checked against the limit, and a length the
    // process cannot hold is an error, not an abort.
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(usize::try_from(length).unwrap_or(usize::MAX))
        .map_err(|_| failed(ErrorKind::OutOfMemory.into()))?;
    let chunk_len =
        usize::try_from(length.saturating_add(1)).map_or(READ_CHUNK, |len| len.min(READ_CHUNK));
    let mut chunk = vec![0_u8; chunk_len];
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

/// The file at `path` open for reading, or `None` when it is not a regular
/// file (a directory, a FIFO, a device).
///
/// Opening a FIFO or a device can block until another party acts, and a read
/// runs on a shared IO lane. On Unix the open does not block
/// (`O_NONBLOCK`, which a regular file's reads ignore), and the open file is
/// what is checked, so a path swapped for a FIFO between a check and the
/// open is refused too. Elsewhere the path is checked before it is opened
/// and the open file again after.
fn open_regular_file(path: &Path) -> std::io::Result<Option<File>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    if !std::fs::metadata(path)?.is_file() {
        return Ok(None);
    }
    let file = options.open(path)?;
    Ok(file.metadata()?.is_file().then_some(file))
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

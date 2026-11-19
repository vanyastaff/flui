//! [`AssetKey`]: the name an application gives one of its assets (ADR-0384 §1).

use std::fmt;
use std::sync::Arc;

/// The most bytes a key may have.
pub const MAX_ASSET_KEY_LEN: usize = 1024;

/// The name of one of an application's assets: its path relative to the
/// project root, with `/` between segments, as `[assets] directories` in
/// `flui.toml` declares it (`assets/logo.png` for `<project>/assets/logo.png`).
///
/// [`try_new`](Self::try_new) admits a key by an allow-list, so a key is
/// never absolute and never leaves its bundle's root: every
/// [`AssetSource`](super::AssetSource) joins it to that root as it is. The
/// same key names the same asset on every platform.
///
/// Cheap to clone: clones share one allocation.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey(Arc<str>);

impl AssetKey {
    /// The key `key`, if it names a path inside a bundle.
    ///
    /// # Errors
    ///
    /// [`AssetKeyError`] when `key` is empty or longer than
    /// [`MAX_ASSET_KEY_LEN`] bytes, has an empty, `.` or `..` segment
    /// (a leading or trailing `/` makes an empty one), or holds a `\`, a
    /// `:` or a control character anywhere.
    pub fn try_new(key: &str) -> Result<Self, AssetKeyError> {
        if key.is_empty() {
            return Err(AssetKeyError::Empty);
        }
        if key.len() > MAX_ASSET_KEY_LEN {
            return Err(AssetKeyError::TooLong { len: key.len() });
        }
        if let Some(character) = key
            .chars()
            .find(|&character| matches!(character, '\\' | ':') || character.is_control())
        {
            return Err(AssetKeyError::Forbidden { character });
        }
        for segment in key.split('/') {
            match segment {
                "" => return Err(AssetKeyError::EmptySegment),
                "." | ".." => return Err(AssetKeyError::DotSegment),
                _ => {}
            }
        }
        Ok(Self(Arc::from(key)))
    }

    /// The key as written, segments joined by `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The key's segments, in order: none is empty, `.` or `..`.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Debug for AssetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AssetKey").field(&self.as_str()).finish()
    }
}

impl fmt::Display for AssetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why [`AssetKey::try_new`] refused a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AssetKeyError {
    /// The key is empty.
    #[error("an asset key is empty")]
    Empty,
    /// The key is longer than [`MAX_ASSET_KEY_LEN`] bytes.
    #[error("an asset key is {len} bytes, over the {MAX_ASSET_KEY_LEN}-byte limit")]
    TooLong {
        /// The key's length in bytes.
        len: usize,
    },
    /// A segment is empty: the key starts or ends with `/`, or has `//`.
    #[error("an asset key has an empty segment")]
    EmptySegment,
    /// A segment is `.` or `..`.
    #[error("an asset key has a `.` or `..` segment")]
    DotSegment,
    /// The key holds a character no key may: `\`, `:` or a control
    /// character.
    #[error("an asset key holds {character:?}")]
    Forbidden {
        /// The first such character.
        character: char,
    },
}

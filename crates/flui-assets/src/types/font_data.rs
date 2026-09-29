//! Font data type for storing loaded font bytes.

use std::sync::Arc;

/// Font data loaded from an asset.
///
/// Contains the raw font bytes (TTF/OTF format) that can be used
/// by text rendering backends.
#[derive(Clone, Debug)]
pub struct FontData {
    /// Raw font bytes (TTF/OTF format)
    pub bytes: Arc<Vec<u8>>,
}

impl FontData {
    /// Creates font data from raw bytes.
    #[inline]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
        }
    }

    /// Returns a reference to the font bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the size of the font data in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the font data is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl PartialEq for FontData {
    /// Font data is equal if it points to the same underlying bytes.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes)
    }
}

impl Eq for FontData {}

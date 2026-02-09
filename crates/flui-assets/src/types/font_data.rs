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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_data_creation() {
        let bytes = vec![0x00, 0x01, 0x00, 0x00, 1, 2, 3, 4];
        let font_data = FontData::from_bytes(bytes.clone());

        assert_eq!(font_data.len(), 8);
        assert_eq!(font_data.as_bytes(), &bytes[..]);
        assert!(!font_data.is_empty());
    }

    #[test]
    fn test_font_data_equality() {
        let bytes = vec![1, 2, 3, 4];
        let font1 = FontData::from_bytes(bytes.clone());
        let font2 = font1.clone();
        let font3 = FontData::from_bytes(bytes);

        assert_eq!(font1, font2); // Same Arc
        assert_ne!(font1, font3); // Different Arc
    }

    #[test]
    fn test_font_data_empty() {
        let font_data = FontData::from_bytes(vec![]);
        assert!(font_data.is_empty());
        assert_eq!(font_data.len(), 0);
    }
}

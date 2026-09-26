//! The faces and variation instances keys name, held by the raster side.

use std::collections::HashMap;
use std::sync::Arc;

use super::key::{FaceKey, VariationId};
use crate::error::RegisterFontError;

/// A font file's bytes, shared with whoever else holds them.
pub type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// The faces and variation instances keys name, kept alive by the raster
/// side.
///
/// Append-only: a face registered once stays for the registry's life, so a
/// key naming it never dangles.
#[derive(Default)]
pub struct FontRegistry {
    faces: HashMap<FaceKey, Face>,
    /// Interned coordinate sets; a [`VariationId`] is an index into this.
    variations: Vec<Box<[i16]>>,
    variation_ids: HashMap<Box<[i16]>, VariationId>,
}

/// One registered face and swash's handle on it.
pub(super) struct Face {
    bytes: FontBytes,
    /// The face's table-directory offset inside `bytes`.
    offset: u32,
    /// swash keys its scaler cache (hinting state included) on this.
    /// `FontRef::from_index` mints a fresh key per call, so it is made once
    /// per face here; otherwise every glyph would rebuild the hinting
    /// instance.
    cache_key: swash::CacheKey,
}

impl Face {
    /// swash's view of the face.
    pub(super) fn font_ref(&self) -> swash::FontRef<'_> {
        swash::FontRef {
            data: (*self.bytes).as_ref(),
            offset: self.offset,
            key: self.cache_key,
        }
    }
}

impl FontRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `face` over `bytes`. A face already present keeps its first
    /// bytes (a no-op).
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] when `bytes` at `face.index` is not a face.
    pub fn register_face(
        &mut self,
        face: FaceKey,
        bytes: FontBytes,
    ) -> Result<(), RegisterFontError> {
        if self.faces.contains_key(&face) {
            return Ok(());
        }
        let index = usize::try_from(face.index).map_err(|_| RegisterFontError)?;
        let (offset, cache_key) = {
            let font =
                swash::FontRef::from_index((*bytes).as_ref(), index).ok_or(RegisterFontError)?;
            (font.offset, font.key)
        };
        self.faces.insert(
            face,
            Face {
                bytes,
                offset,
                cache_key,
            },
        );
        Ok(())
    }

    /// Whether `face` is registered.
    #[must_use]
    pub fn contains_face(&self, face: FaceKey) -> bool {
        self.faces.contains_key(&face)
    }

    /// How many faces are registered.
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// The id for `coords`: `None` for the default instance (empty or
    /// all-zero coordinates); equal slices share one id.
    #[expect(
        clippy::expect_used,
        reason = "2^32 distinct variation instances in one registry is not a reachable state"
    )]
    pub fn intern_variation(&mut self, coords: &[i16]) -> Option<VariationId> {
        if coords.iter().all(|c| *c == 0) {
            return None;
        }
        if let Some(id) = self.variation_ids.get(coords) {
            return Some(*id);
        }
        let id = VariationId::from_index(self.variations.len())
            .expect("BUG: fewer than 2^32 distinct variation instances");
        let coords: Box<[i16]> = coords.into();
        self.variations.push(coords.clone());
        self.variation_ids.insert(coords, id);
        Some(id)
    }

    /// The coordinates `id` was interned from; `None` for an id this registry
    /// did not mint.
    #[must_use]
    pub fn variation(&self, id: VariationId) -> Option<&[i16]> {
        self.variations.get(id.index()).map(|coords| &**coords)
    }

    pub(super) fn face(&self, face: FaceKey) -> Option<&Face> {
        self.faces.get(&face)
    }
}

impl std::fmt::Debug for FontRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontRegistry")
            .field("faces", &self.faces.len())
            .field("variations", &self.variations.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{FontBytes, FontRegistry};
    use crate::parley_text::FaceKey;

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");

    fn roboto() -> FontBytes {
        Arc::new(ROBOTO)
    }

    #[test]
    fn equal_coordinates_intern_to_one_id() {
        let mut registry = FontRegistry::new();
        let bold = registry.intern_variation(&[4096]).expect("not the default");
        let again = registry.intern_variation(&[4096]).expect("not the default");
        let other = registry
            .intern_variation(&[-2048])
            .expect("not the default");
        assert_eq!(bold, again);
        assert_ne!(bold, other);
        assert_eq!(registry.variation(bold), Some(&[4096i16][..]));
        assert_eq!(registry.variation(other), Some(&[-2048i16][..]));
    }

    #[test]
    fn default_coordinates_intern_to_none() {
        let mut registry = FontRegistry::new();
        assert_eq!(registry.intern_variation(&[]), None);
        assert_eq!(registry.intern_variation(&[0, 0]), None);
    }

    #[test]
    fn bytes_that_are_not_a_face_are_refused() {
        let mut registry = FontRegistry::new();
        let face = FaceKey {
            blob_id: 1,
            index: 0,
        };
        let garbage: FontBytes = Arc::new(vec![0x5a_u8; 256]);
        assert!(registry.register_face(face, garbage).is_err());
        let second_face_of_a_single_face_file = FaceKey {
            blob_id: 2,
            index: 1,
        };
        assert!(
            registry
                .register_face(second_face_of_a_single_face_file, roboto())
                .is_err()
        );
        assert_eq!(registry.face_count(), 0);
    }

    #[test]
    fn a_face_registered_twice_keeps_its_first_bytes() {
        let mut registry = FontRegistry::new();
        let face = FaceKey {
            blob_id: 3,
            index: 0,
        };
        let first = roboto();
        registry
            .register_face(face, Arc::clone(&first))
            .expect("Roboto is a face");
        // Different bytes under the same key: ignored, not an error.
        let other: FontBytes = Arc::new(vec![0_u8; 16]);
        registry
            .register_face(face, other)
            .expect("already present");
        assert_eq!(registry.face_count(), 1);
        let held = registry.face(face).expect("registered");
        assert!(Arc::ptr_eq(&held.bytes, &first), "the first bytes are kept");
    }
}

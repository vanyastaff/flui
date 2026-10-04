//! The faces and variation instances keys name, held by the raster side.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Arc;

use super::key::{FaceKey, VariationId};
use crate::display_list::ShapedRun;
use crate::error::RegisterFaceError;

/// A font file's bytes, shared with whoever else holds them.
pub type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// What a run's glyph keys share, resolved against one registry: the face
/// and the interned variation instance. [`FontRegistry::prepare_run`] makes
/// it; [`ShapedRun::placed_glyphs`] keys each glyph with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RunKey {
    pub(crate) face: FaceKey,
    pub(crate) variation: Option<VariationId>,
}

/// The faces and variation instances keys name, kept alive by the raster
/// side.
///
/// Append-only: a face registered once stays for the registry's life, so a
/// key naming it never dangles.
pub struct FontRegistry {
    /// This registry's identity, stamped on every [`VariationId`] it mints: a
    /// random 64-bit value, so two registries collide with negligible
    /// probability and no process-wide counter is needed.
    identity: u64,
    faces: HashMap<FaceKey, Face>,
    owned_sources: bool,
    /// Interned coordinate sets; a [`VariationId`] is an index into this.
    variations: Vec<Box<[i16]>>,
    variation_ids: HashMap<Box<[i16]>, VariationId>,
}

enum FaceBytes {
    Shared(FontBytes),
    Owned(Arc<[u8]>),
}

impl AsRef<[u8]> for FaceBytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Shared(bytes) => (**bytes).as_ref(),
            Self::Owned(bytes) => bytes.as_ref(),
        }
    }
}

/// One registered face and swash's handle on it.
pub(super) struct Face {
    bytes: FaceBytes,
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
            data: self.bytes.as_ref(),
            offset: self.offset,
            key: self.cache_key,
        }
    }
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self {
            // std's randomly keyed hasher over no input: a fresh random value
            // per registry, from state std already keeps.
            identity: RandomState::new().build_hasher().finish(),
            faces: HashMap::new(),
            owned_sources: false,
            variations: Vec::new(),
            variation_ids: HashMap::new(),
        }
    }
}

impl FontRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty registry that copies new faces into concrete owned storage.
    ///
    /// Use for font sources from unloadable images. The source's erased
    /// implementation and backing allocation can retire after registration.
    /// Ordinary registries retain sources so a shaper's weak cache keeps its
    /// blob identity while the registered face remains live.
    #[must_use]
    pub fn with_owned_sources() -> Self {
        Self {
            owned_sources: true,
            ..Self::default()
        }
    }

    /// Adds `face` over `bytes`. Registering a present face again over equal
    /// bytes is a no-op. The default registry retains the source; a registry
    /// constructed with [`Self::with_owned_sources`] copies it on first admission.
    ///
    /// # Errors
    ///
    /// - [`RegisterFaceError::NotAFace`] when `bytes` at `face.index` is not
    ///   a face.
    /// - [`RegisterFaceError::Conflict`] when `face` is already registered
    ///   over different bytes: the key keeps naming the first face, so glyph
    ///   ids of the second are never drawn from the first.
    pub fn register_face(
        &mut self,
        face: FaceKey,
        bytes: FontBytes,
    ) -> Result<(), RegisterFaceError> {
        if let Some(held) = self.faces.get(&face) {
            let (held, new) = (held.bytes.as_ref(), (*bytes).as_ref());
            let same = (held.as_ptr() == new.as_ptr() && held.len() == new.len()) || held == new;
            return if same {
                Ok(())
            } else {
                Err(RegisterFaceError::Conflict)
            };
        }
        let index = usize::try_from(face.index).map_err(|_| RegisterFaceError::NotAFace)?;
        let bytes = if self.owned_sources {
            // Retain neither an unloadable image's erased AsRef vtable nor
            // a slice into that image. Ordinary sources keep their blob alive.
            FaceBytes::Owned(Arc::from((*bytes).as_ref()))
        } else {
            FaceBytes::Shared(bytes)
        };
        let (offset, cache_key) = {
            let font = swash::FontRef::from_index(bytes.as_ref(), index)
                .ok_or(RegisterFaceError::NotAFace)?;
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

    /// Registers the face `run` is shaped in, unless it is already, and
    /// interns its variation coordinates: what every glyph key of the run
    /// shares. One hash lookup per run once the face is held.
    ///
    /// # Errors
    ///
    /// As [`Self::register_face`]: the run's blob holds no face at its index,
    /// or another blob was registered under the same key.
    pub fn prepare_run(&mut self, run: &ShapedRun<'_>) -> Result<RunKey, RegisterFaceError> {
        let face = run.face();
        let key = face.key();
        if !self.faces.contains_key(&key) {
            self.register_face(key, face.blob().bytes().clone())?;
        }
        Ok(RunKey {
            face: key,
            variation: self.intern_variation(run.coords()),
        })
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
        let id = VariationId::from_index(self.identity, self.variations.len())
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
        if id.registry() != self.identity {
            return None;
        }
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

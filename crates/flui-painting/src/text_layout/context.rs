//! The Parley path's font collection and per-realm text context
//! (ADR-0092 §2–§3).
//!
//! One [`FontCollection`] serves the app: every realm builds its own
//! [`TextContext`] from it, and a face registered on the collection reaches
//! every context built from it, including ones built earlier. The collection
//! is add-only: there is no way to remove a face, so a glyph key that names a
//! face never outlives it (ADR-0092 §2).
//!
//! No FLUI lock guards either type. The collection is fontique's shared mode:
//! a registration writes under fontique's own mutex and bumps a version, and
//! each context re-reads the data on its next query after a bump, one atomic
//! load otherwise. A context is owner-thread state used through `&mut`.
//!
//! The types exist in every build so their shape does not depend on features;
//! without `parley` they hold nothing and shape nothing.

use std::fmt;
use std::sync::Arc;

#[cfg(feature = "parley")]
use crate::error::RegisterFontError;
#[cfg(feature = "parley")]
use crate::parley_text::SpanBrush;

/// The app's font collection: shared, add-only, passed explicitly.
///
/// A clone is the same collection ([`FontCollection::ptr_eq`]). Built once by
/// the composition root and handed to every realm, which shapes through a
/// [`TextContext`] of its own.
#[derive(Clone)]
pub struct FontCollection(Arc<FontCollectionInner>);

struct FontCollectionInner {
    /// fontique's collection in shared mode, with no host scan.
    #[cfg(feature = "parley")]
    collection: parley::fontique::Collection,
    /// One source cache shared by every context built from the collection.
    #[cfg(feature = "parley")]
    source_cache: parley::fontique::SourceCache,
}

impl FontCollection {
    /// A new collection.
    ///
    /// With `parley` and `bundled-fonts` it holds the embedded Roboto,
    /// Material Icons and Cupertino Icons faces, and binds the generic
    /// families (sans-serif, serif, monospace, system-ui) to Roboto, so text
    /// shapes the same on every host. Without `parley` it is an empty handle
    /// and loads nothing.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(FontCollectionInner::new()))
    }

    /// Whether `a` and `b` are the same collection.
    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// Adds every face in `font_bytes` to the collection.
    ///
    /// Visible to every [`TextContext`] built from this collection, including
    /// ones built before the call: each sees it at its next shape.
    /// Registration goes through a local clone of fontique's collection, whose
    /// shared mode propagates the change; FLUI adds no lock.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] if the bytes hold no face; nothing is added and
    /// no context re-reads the collection.
    #[cfg(feature = "parley")]
    pub fn register_font(&self, font_bytes: &[u8]) -> Result<(), RegisterFontError> {
        // Checked before fontique sees the bytes: its registration bumps the
        // shared version even when it finds no face, which would make every
        // context deep-copy the collection for nothing.
        if ::swash::FontRef::from_index(font_bytes, 0).is_none() {
            return Err(RegisterFontError);
        }
        let mut collection = self.0.collection.clone();
        let families = collection.register_fonts(
            parley::fontique::Blob::new(Arc::new(font_bytes.to_vec())),
            None,
        );
        if families.is_empty() {
            return Err(RegisterFontError);
        }
        Ok(())
    }
}

impl FontCollectionInner {
    #[cfg(not(feature = "parley"))]
    fn new() -> Self {
        Self {}
    }

    #[cfg(feature = "parley")]
    fn new() -> Self {
        use parley::fontique::{Collection, CollectionOptions, SourceCache};

        #[cfg_attr(
            not(feature = "bundled-fonts"),
            expect(unused_mut, reason = "only the bundled faces are registered here")
        )]
        let mut collection = Collection::new(CollectionOptions {
            shared: true,
            system_fonts: false,
        });
        #[cfg(feature = "bundled-fonts")]
        bind_bundled_faces(&mut collection);
        Self {
            collection,
            source_cache: SourceCache::new_shared(),
        }
    }
}

/// Registers the embedded faces and binds every generic family to Roboto.
#[cfg(all(feature = "parley", feature = "bundled-fonts"))]
fn bind_bundled_faces(collection: &mut parley::fontique::Collection) {
    use parley::fontique::{Blob, GenericFamily};

    use crate::fonts::{CUPERTINO_ICONS, MATERIAL_ICONS_REGULAR, ROBOTO_REGULAR};

    let mut register =
        |bytes: &'static [u8]| collection.register_fonts(Blob::new(Arc::new(bytes)), None);
    let roboto: Vec<_> = register(ROBOTO_REGULAR)
        .into_iter()
        .map(|(family, _)| family)
        .collect();
    register(MATERIAL_ICONS_REGULAR);
    register(CUPERTINO_ICONS);
    for generic in [
        GenericFamily::SansSerif,
        GenericFamily::Serif,
        GenericFamily::Monospace,
        GenericFamily::SystemUi,
    ] {
        collection.set_generic_families(generic, roboto.iter().copied());
    }
}

impl Default for FontCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FontCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontCollection")
            .field("id", &Arc::as_ptr(&self.0))
            .finish_non_exhaustive()
    }
}

/// One realm's text service: Parley's font and layout contexts over a clone
/// of the app's [`FontCollection`].
///
/// Owner-thread state, used through `&mut`. It is `Send`, so a realm can be
/// built on one thread and run on another, and it holds no lock: two realms
/// shape at the same time without waiting on each other.
pub struct TextContext {
    fonts: FontCollection,
    #[cfg(feature = "parley")]
    pub(crate) font_cx: parley::FontContext,
    #[cfg(feature = "parley")]
    pub(crate) layout_cx: parley::LayoutContext<SpanBrush>,
}

impl TextContext {
    /// A context over `fonts`: it sees every face registered on the
    /// collection, before or after this call.
    #[must_use]
    pub fn new(fonts: &FontCollection) -> Self {
        Self {
            fonts: fonts.clone(),
            #[cfg(feature = "parley")]
            font_cx: parley::FontContext {
                collection: fonts.0.collection.clone(),
                source_cache: fonts.0.source_cache.clone(),
            },
            #[cfg(feature = "parley")]
            layout_cx: parley::LayoutContext::new(),
        }
    }

    /// The collection this context was built from.
    #[must_use]
    pub fn fonts(&self) -> &FontCollection {
        &self.fonts
    }
}

impl fmt::Debug for TextContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextContext")
            .field("fonts", &self.fonts)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{FontCollection, TextContext};

    const fn assert_send_sync<T: Send + Sync>() {}
    const fn assert_send<T: Send>() {}

    /// The collection crosses to every realm's thread; a context moves with
    /// its realm.
    const _: () = {
        assert_send_sync::<FontCollection>();
        assert_send::<TextContext>();
    };

    #[cfg(feature = "parley")]
    #[test]
    fn bytes_with_no_face_are_refused() {
        let fonts = FontCollection::new();
        assert!(fonts.register_font(b"not a font").is_err());
        assert!(fonts.register_font(&[]).is_err());
    }
}

//! The host's installed fonts, found by one fontdb scan, with the generic
//! families and the fallback lists FLUI picks for them (ADR-0092 §7).
//!
//! [`HostFonts::scan`] is the only host discovery in FLUI. The app scans once
//! and feeds its [`FontCollection`](super::FontCollection) from the result
//! ([`FontCollection::with_host_fonts`](super::FontCollection::with_host_fonts));
//! nothing here is process-global, and nothing shapes on the scan.

use std::fmt;
use std::sync::Arc;

use super::fallback_chain::FallbackChain;
use super::font_resolve;

/// The host's installed faces, found by one fontdb scan, with the generic
/// families and the fallback lists FLUI picks for them.
///
/// A value: it holds no global, and a collection fed from it keeps no handle
/// on it. Build it with [`HostFonts::scan`] and feed a collection with
/// [`FontCollection::with_host_fonts`](super::FontCollection::with_host_fonts).
pub struct HostFonts {
    db: fontdb::Database,
    generics: HostGenerics,
    chain: FallbackChain,
}

/// The family each generic names on the host.
#[derive(Clone, Debug)]
struct HostGenerics {
    sans_serif: String,
    serif: String,
    monospace: String,
    cursive: String,
    fantasy: String,
}

impl HostFonts {
    /// Scans the host's installed fonts, and picks the generic families
    /// and fallback lists for the host's locale (`en-US` when the host
    /// reports none).
    ///
    /// Reads every font directory the platform names (and fontconfig's
    /// configuration where there is one), parsing each file's name and style
    /// tables; the face data itself is read later, by the collection it
    /// feeds. It costs a few milliseconds on a desktop host; run it once per
    /// app.
    #[must_use]
    pub fn scan() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
        tracing::debug!(faces = db.len(), locale, "scanned the host's fonts");
        Self::from_database(db, &locale)
    }

    /// Host fonts over `db`, as [`HostFonts::scan`] builds them from the
    /// host's own database: each generic bound to a family `db` carries that
    /// can set Latin text, preferring the conventional family where `db`
    /// carries it, and the fallback lists picked for `locale`.
    pub(crate) fn from_database(mut db: fontdb::Database, locale: &str) -> Self {
        use fontdb::Family;

        // The conventional desktop families first; `bind_generic_families`
        // moves only a generic naming a family `db` does not carry.
        db.set_sans_serif_family("Open Sans");
        db.set_serif_family("DejaVu Serif");
        db.set_monospace_family("Noto Sans Mono");
        font_resolve::bind_generic_families(&mut db);
        let generic = |family: Family<'_>| db.family_name(&family).to_owned();
        let generics = HostGenerics {
            sans_serif: generic(Family::SansSerif),
            serif: generic(Family::Serif),
            monospace: generic(Family::Monospace),
            cursive: generic(Family::Cursive),
            fantasy: generic(Family::Fantasy),
        };
        Self {
            db,
            generics,
            chain: FallbackChain::platform(locale),
        }
    }

    /// What a collection is fed from: each font file's path, each in-memory
    /// font's data (shared, not copied), every source's family names, the
    /// generic families' names and the fallback lists.
    pub(crate) fn faces(&self) -> HostFaces {
        use std::collections::HashMap;

        use fontdb::Source;

        #[derive(PartialEq, Eq, Hash)]
        enum SourceKey {
            Path(std::path::PathBuf),
            Blob(usize),
        }

        let mut sources: Vec<HostSource> = Vec::new();
        let mut index: HashMap<SourceKey, usize> = HashMap::new();
        for face in self.db.faces() {
            let (key, data) = match &face.source {
                Source::Binary(data) => (
                    SourceKey::Blob(Arc::as_ptr(data).cast::<()>() as usize),
                    HostData::Blob(Arc::clone(data)),
                ),
                Source::File(path) | Source::SharedFile(path, _) => {
                    (SourceKey::Path(path.clone()), HostData::Path(path.clone()))
                }
            };
            let slot = *index.entry(key).or_insert_with(|| {
                sources.push(HostSource {
                    data,
                    families: Vec::new(),
                });
                sources.len() - 1
            });
            let families = &mut sources[slot].families;
            for (name, _) in &face.families {
                if !families.contains(name) {
                    families.push(name.clone());
                }
            }
        }
        let generics = &self.generics;
        HostFaces {
            sources,
            sans_serif: generics.sans_serif.clone(),
            serif: generics.serif.clone(),
            monospace: generics.monospace.clone(),
            cursive: generics.cursive.clone(),
            fantasy: generics.fantasy.clone(),
            chain: self.chain.clone(),
        }
    }
}

#[cfg(any(test, feature = "testing"))]
impl HostFonts {
    /// The family the sans-serif generic names.
    pub(crate) fn sans_serif_family(&self) -> &str {
        &self.generics.sans_serif
    }

    /// The first family name of every face, each once, in database order.
    pub(crate) fn family_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for face in self.db.faces() {
            if let Some((name, _)) = face.families.first()
                && !names.contains(name)
            {
                names.push(name.clone());
            }
        }
        names
    }

    /// Whether some face maps every character of `text` that is not
    /// whitespace.
    pub(crate) fn covers(&self, text: &str) -> bool {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .all(|c| self.db.faces().any(|face| self.face_maps(face, c)))
    }

    /// Whether every character of `text` that is not whitespace is mapped by
    /// a face of a family a fed collection falls back to: the sans-serif
    /// generic's family, the chain's list for the character's script, or the
    /// chain's common list. There is no walk over every other face (mapping
    /// decision 17).
    ///
    /// With `bundled-fonts` the fed collection keeps the bundled Roboto as its
    /// sans-serif family and every fallback ends in it, so that face is the
    /// one asked, not the family the host's own generic names.
    pub(crate) fn chain_covers(&self, text: &str) -> bool {
        use icu_properties::props::Script as IcuScript;
        use icu_properties::{CodePointMapData, PropertyNamesShort};

        let scripts = CodePointMapData::<IcuScript>::new();
        let names = PropertyNamesShort::<IcuScript>::new();
        text.chars().filter(|c| !c.is_whitespace()).all(|c| {
            let script = names.get(scripts.get(c)).map_or(&[][..], |tag| {
                self.chain
                    .script(parley::fontique::Script::from_str_unchecked(tag))
            });
            let reachable = |name: &str| {
                (!cfg!(feature = "bundled-fonts") && name == self.generics.sans_serif)
                    || script.contains(&name)
                    || self.chain.common().contains(&name)
            };
            bundled_sans_serif_maps(c)
                || self.db.faces().any(|face| {
                    face.families.iter().any(|(name, _)| reachable(name)) && self.face_maps(face, c)
                })
        })
    }

    /// Whether `face` maps `c` to a glyph.
    fn face_maps(&self, face: &fontdb::FaceInfo, c: char) -> bool {
        self.db.with_face_data(face.id, |data, index| {
            swash::FontRef::from_index(data, usize::try_from(index).ok()?)
                .map(|font| font.charmap().map(c) != 0)
        }) == Some(Some(true))
    }
}

/// Whether the bundled Roboto, a bundled build's sans-serif family, maps `c`.
#[cfg(all(any(test, feature = "testing"), feature = "bundled-fonts"))]
fn bundled_sans_serif_maps(c: char) -> bool {
    swash::FontRef::from_index(crate::fonts::ROBOTO_REGULAR, 0)
        .is_some_and(|font| font.charmap().map(c) != 0)
}

/// Without bundled faces the host's sans-serif family is the one asked.
#[cfg(all(any(test, feature = "testing"), not(feature = "bundled-fonts")))]
fn bundled_sans_serif_maps(_: char) -> bool {
    false
}

impl fmt::Debug for HostFonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostFonts")
            .field("faces", &self.db.len())
            .field("generics", &self.generics)
            .field("chain", &self.chain)
            .finish()
    }
}

/// The faces a host scan holds, taken by [`HostFonts::faces`] for a
/// collection to be fed from.
pub(crate) struct HostFaces {
    /// Every font source, in the order the scan first names it.
    pub(crate) sources: Vec<HostSource>,
    /// The family the sans-serif generic names.
    pub(crate) sans_serif: String,
    /// The family the serif generic names.
    pub(crate) serif: String,
    /// The family the monospace generic names.
    pub(crate) monospace: String,
    /// The family the cursive generic names.
    pub(crate) cursive: String,
    /// The family the fantasy generic names.
    pub(crate) fantasy: String,
    /// The fallback lists picked for the host.
    pub(crate) chain: FallbackChain,
}

/// One font file or in-memory font, and every family name its faces carry.
pub(crate) struct HostSource {
    pub(crate) data: HostData,
    pub(crate) families: Vec<String>,
}

/// Where a [`HostSource`]'s bytes are.
pub(crate) enum HostData {
    /// A file, which the collection maps itself.
    Path(std::path::PathBuf),
    /// Bytes held in memory.
    Blob(Arc<dyn AsRef<[u8]> + Send + Sync>),
}

#[cfg(all(test, feature = "bundled-fonts"))]
mod tests {
    use super::{FallbackChain, HostFonts, HostGenerics};

    /// A bundled build's fed collection keeps the bundled Roboto as its
    /// sans-serif family, so coverage asks that face, not the face the
    /// host's own sans-serif generic names: Arabic only the host's generic
    /// family maps is never reached, and Latin the host lacks but Roboto
    /// maps is.
    #[test]
    fn chain_coverage_asks_the_bundled_sans_serif_face() {
        const ARABIC: &str = "FLUI Probe Arabic";
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../assets/fonts/probe-arabic-ligature.ttf").to_vec());
        let host = HostFonts {
            db,
            generics: HostGenerics {
                sans_serif: ARABIC.to_owned(),
                serif: ARABIC.to_owned(),
                monospace: ARABIC.to_owned(),
                cursive: ARABIC.to_owned(),
                fantasy: ARABIC.to_owned(),
            },
            chain: FallbackChain::from_lists(&[], &[]),
        };
        assert!(host.covers("\u{0644}"), "the host face maps lam");
        assert!(
            !host.chain_covers("\u{0644}"),
            "the fed collection never reaches the host's sans-serif family"
        );
        assert!(
            host.chain_covers("a"),
            "the bundled Roboto maps Latin the host lacks"
        );
    }
}

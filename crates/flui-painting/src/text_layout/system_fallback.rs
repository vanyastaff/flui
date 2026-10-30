//! The host's own answer to which faces draw a character a style's family
//! lacks (ADR-0271).
//!
//! A [`SystemFallback`] is plain data: the platform layer computes it off the
//! owner thread, asking the operating system once per script with that
//! script's sample text ([`SystemFallback::queries`]), and a
//! [`FontCollection`](super::FontCollection) merges it ahead of FLUI's own
//! fallback tables. Nothing here calls the operating system.

use std::fmt;
use std::path::PathBuf;

/// An ISO 15924 script code, such as `Latn`, `Arab` or `Hani`: the key a
/// fallback list is kept under.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScriptCode([u8; 4]);

impl ScriptCode {
    /// The script whose four-letter code is `code`, such as `*b"Arab"`.
    #[must_use]
    pub const fn from_bytes(code: [u8; 4]) -> Self {
        Self(code)
    }

    /// The four-letter code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("Zzzz")
    }

    pub(crate) fn to_fontique(self) -> parley::fontique::Script {
        parley::fontique::Script::from_bytes(self.0)
    }
}

impl fmt::Debug for ScriptCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScriptCode({})", self.as_str())
    }
}

impl fmt::Display for ScriptCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A face the platform named: by its file where the platform says which,
/// else by its family name.
///
/// A file names one face exactly. A family name is matched against the
/// families the collection holds, so a platform that spells a family another
/// way than the face's own name table does names nothing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FaceRef {
    /// The face at `index` in the font file at `path`.
    File {
        /// The font file.
        path: PathBuf,
        /// The face's index in a collection file; 0 for a single-face file.
        index: u32,
    },
    /// A family, by the name the platform gave it.
    Family(String),
}

/// The host's fallback answer for one locale: an ordered list of faces per
/// script, the colour emoji faces, and a common list.
///
/// Built by the platform layer and installed with
/// [`FallbackOrder::set_system_fallback`](super::FallbackOrder::set_system_fallback)
/// or a feed's [`HostFontFeed::with_system_fallback`](super::HostFontFeed::with_system_fallback).
/// The lists come first in the collection's fallback order, FLUI's own
/// tables after them. [`SystemFallback::none`] is a host that gave no answer:
/// FLUI's tables alone.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SystemFallback {
    /// The BCP 47 language tag the answer was asked for, such as `ja-JP`.
    /// FLUI's own tables are picked for it too.
    pub locale: String,
    /// The faces the host draws each script with, best first. A script the
    /// host gave no answer for is absent.
    pub scripts: Vec<(ScriptCode, Vec<FaceRef>)>,
    /// The faces the host draws emoji with, best first.
    pub emoji: Vec<FaceRef>,
    /// The faces the host tries for any script after the script's own,
    /// best first.
    pub common: Vec<FaceRef>,
}

impl SystemFallback {
    /// The text the platform is asked about for emoji.
    pub const EMOJI_SAMPLE: &'static str = "\u{1F600}";

    /// No answer from the host for `locale`: a collection given it falls
    /// back on FLUI's tables alone.
    #[must_use]
    pub fn none(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            scripts: Vec::new(),
            emoji: Vec::new(),
            common: Vec::new(),
        }
    }

    /// This answer with `faces` as the host's list for `script`, replacing
    /// any list it had for it.
    #[must_use]
    pub fn with_script(mut self, script: ScriptCode, faces: Vec<FaceRef>) -> Self {
        self.scripts.retain(|(held, _)| *held != script);
        self.scripts.push((script, faces));
        self
    }

    /// This answer with `faces` as the host's emoji faces.
    #[must_use]
    pub fn with_emoji(mut self, faces: Vec<FaceRef>) -> Self {
        self.emoji = faces;
        self
    }

    /// This answer with `faces` as the host's common list.
    #[must_use]
    pub fn with_common(mut self, faces: Vec<FaceRef>) -> Self {
        self.common = faces;
        self
    }

    /// The host's list for `script`, empty when it gave none.
    #[must_use]
    pub fn script(&self, script: ScriptCode) -> &[FaceRef] {
        self.scripts
            .iter()
            .find(|(held, _)| *held == script)
            .map_or(&[], |(_, faces)| faces)
    }

    /// Whether the host gave no answer at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scripts.iter().all(|(_, faces)| faces.is_empty())
            && self.emoji.is_empty()
            && self.common.is_empty()
    }

    /// Every script a fallback list is kept for, with the text to ask the
    /// platform about it: a few characters only that script uses. The
    /// platform layer asks once per entry.
    pub fn queries() -> impl Iterator<Item = (ScriptCode, &'static str)> {
        use parley::fontique::{Script, ScriptExt as _};

        Script::all_samples()
            .iter()
            .map(|(script, sample)| (ScriptCode(script.to_bytes()), *sample))
    }
}

/// What `ask` answers for `locale`, or no answer where it panics.
///
/// The query is the platform layer's code calling the operating system; a
/// panic in it must not take the scanned faces and FLUI's tables with it
/// (ADR-0271 §3). The panic is logged; its payload is leaked rather than
/// dropped here, since its destructor is code of the query's too.
pub(crate) fn ask_contained(
    ask: impl FnOnce(&str) -> SystemFallback,
    locale: &str,
) -> SystemFallback {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    match catch_unwind(AssertUnwindSafe(|| ask(locale))) {
        Ok(answer) => answer,
        Err(payload) => {
            let text = flui_foundation::panic::payload_text(&*payload)
                .unwrap_or("non-string panic payload");
            tracing::error!(
                panic = text,
                locale,
                "the host's fallback query panicked; FLUI's tables alone"
            );
            std::mem::forget(payload);
            SystemFallback::none(locale)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FaceRef, ScriptCode, SystemFallback};

    /// Every script asked about has a sample, the samples include Latin and
    /// Arabic, a list set twice keeps the later one, and an answer with only
    /// empty lists is no answer.
    #[test]
    fn a_system_fallback_is_keyed_by_script() {
        let arab = ScriptCode::from_bytes(*b"Arab");
        let queries: Vec<_> = SystemFallback::queries().collect();
        assert!(queries.iter().all(|(_, sample)| !sample.is_empty()));
        assert!(queries.iter().any(|(script, _)| script.as_str() == "Latn"));
        assert!(queries.iter().any(|(script, _)| *script == arab));

        let face = |name: &str| FaceRef::Family(name.to_owned());
        let answer = SystemFallback::none("ar-EG")
            .with_script(arab, vec![face("First")])
            .with_script(arab, vec![face("Second")]);
        assert_eq!(answer.script(arab), [face("Second")]);
        assert!(!answer.is_empty());
        assert!(
            SystemFallback::none("en-US")
                .with_script(arab, Vec::new())
                .is_empty()
        );
        assert_eq!(format!("{arab:?}"), "ScriptCode(Arab)");
    }
}

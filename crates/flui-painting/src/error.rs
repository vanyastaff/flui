//! The errors this crate returns.

use thiserror::Error;

/// Why [`FontRegistry::register_face`] refused a face.
///
/// [`FontRegistry::register_face`]: crate::parley_text::FontRegistry::register_face
#[cfg(feature = "parley")]
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterFaceError {
    /// The bytes hold no face at the key's index.
    #[error("font data holds no face at the requested index")]
    NotAFace,
    /// The key is already registered over other bytes; a key names one face
    /// for the registry's life, so the first bytes stay.
    #[error("the face key is already registered over different font data")]
    Conflict,
}

/// Font bytes handed to [`SharedFontSystem::register_font`] parsed to zero
/// loadable faces (empty, truncated, or not a font at all).
///
/// [`SharedFontSystem::register_font`]: crate::SharedFontSystem::register_font
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("font data contained no loadable faces")]
pub struct RegisterFontError;

//! The one error this crate returns.

use thiserror::Error;

/// Font bytes handed to [`SharedFontSystem::register_font`] parsed to zero
/// loadable faces (empty, truncated, or not a font at all).
///
/// [`SharedFontSystem::register_font`]: crate::SharedFontSystem::register_font
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("font data contained no loadable faces")]
pub struct RegisterFontError;

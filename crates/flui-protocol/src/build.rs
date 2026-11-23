//! What a development tool tells a window about the build that will replace the running code
//! (`build.report`, ADR-0288 §8).
//!
//! `flui run` reports each rebuild of the application to every window of the running build: that
//! one started, that it succeeded, or that it failed and with which diagnostics. A window draws
//! the report over its scene, beside the performance overlay, and nothing in the application
//! sees it: the method's effect is [`MethodEffect::Annotates`](crate::method::MethodEffect), so an
//! observer may call it.

use crate::tree::WindowId;

/// The most diagnostics one report carries. A build that failed with more says how many it left
/// out ([`BuildState::Failed::omitted`]).
pub const MAX_DIAGNOSTICS: usize = 8;

/// `build.report`: what one window shows of the application's build.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct BuildReportParams {
    /// The window.
    pub window: WindowId,
    /// The build's state.
    pub state: BuildState,
}

impl BuildReportParams {
    /// `state`, shown in `window`.
    #[must_use]
    pub const fn new(window: WindowId, state: BuildState) -> Self {
        Self { window, state }
    }
}

/// Where the application's build is.
///
/// `"building"`, `"succeeded"`, `{"failed": {"diagnostics": [ … ], "omitted": 0}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum BuildState {
    /// A build started; the window keeps running the code it has.
    Building,
    /// The build succeeded: the window shows nothing of it.
    Succeeded,
    /// The build failed; the window keeps running the code it has.
    Failed {
        /// The first [`MAX_DIAGNOSTICS`] errors the compiler reported, in its order. A window
        /// refuses a report with more.
        diagnostics: Vec<Diagnostic>,
        /// How many more the compiler reported.
        omitted: u32,
    },
}

/// One message the compiler reported.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Diagnostic {
    /// The message's first line, as the compiler wrote it.
    pub message: String,
    /// The primary span's start, when the message has one.
    pub location: Option<SourceLocation>,
}

impl Diagnostic {
    /// `message`, at `location`.
    #[must_use]
    pub const fn new(message: String, location: Option<SourceLocation>) -> Self {
        Self { message, location }
    }
}

/// A place in a source file, as the compiler names it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SourceLocation {
    /// The file, relative to the package the compiler built when it is inside it.
    pub file: String,
    /// The line, from 1.
    pub line: u32,
    /// The column, from 1, in characters.
    pub column: u32,
}

impl SourceLocation {
    /// `file:line:column`.
    #[must_use]
    pub const fn new(file: String, line: u32, column: u32) -> Self {
        Self { file, line, column }
    }
}

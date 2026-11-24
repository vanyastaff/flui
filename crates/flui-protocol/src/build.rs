//! What a development tool tells a window about the build that will replace the running code
//! (`build.report`, ADR-0395).
//!
//! `flui run` reports each rebuild of the application to every window of the running build: that
//! one started, that it succeeded, or that it failed and with which diagnostics. A window draws
//! the report over its scene, beside the performance overlay, and nothing in the application
//! sees it: the method's effect is [`MethodEffect::Annotates`](crate::method::MethodEffect), so an
//! observer may call it.

use crate::tree::WindowId;

/// The most diagnostics one report carries. A build that failed with more says how many it left
/// out ([`Diagnostics::omitted`]).
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
    /// The build failed with these errors; the window keeps running the code it has.
    Failed(Diagnostics),
}

/// The errors of a failed build: the first [`MAX_DIAGNOSTICS`] the compiler reported, in its
/// order, and how many more it reported.
///
/// No value holds more than [`MAX_DIAGNOSTICS`], a decoded one included: decoding refuses the
/// element past the bound before it reads it, so a report's size on the wire does not decide how
/// much a window holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(inline))]
pub struct Diagnostics {
    #[cfg_attr(feature = "serde", serde(deserialize_with = "bounded"))]
    #[cfg_attr(feature = "schemars", schemars(length(max = MAX_DIAGNOSTICS)))]
    diagnostics: Vec<Diagnostic>,
    omitted: u32,
}

impl Diagnostics {
    /// No errors.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            omitted: 0,
        }
    }

    /// Adds the compiler's next error: kept while fewer than [`MAX_DIAGNOSTICS`] are, counted in
    /// [`omitted`](Self::omitted) after.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics.push(diagnostic);
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }

    /// The errors kept, in the compiler's order: at most [`MAX_DIAGNOSTICS`].
    #[must_use]
    pub fn kept(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// How many errors the compiler reported past those kept.
    #[must_use]
    pub const fn omitted(&self) -> u32 {
        self.omitted
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, diagnostics: I) {
        for diagnostic in diagnostics {
            self.push(diagnostic);
        }
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(diagnostics: I) -> Self {
        let mut all = Self::new();
        all.extend(diagnostics);
        all
    }
}

/// The kept diagnostics, read up to [`MAX_DIAGNOSTICS`]: an element past them is refused before
/// it is decoded.
#[cfg(feature = "serde")]
fn bounded<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Diagnostic>, D::Error> {
    struct Bounded;

    impl<'de> serde::de::Visitor<'de> for Bounded {
        type Value = Vec<Diagnostic>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "at most {MAX_DIAGNOSTICS} diagnostics")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut kept = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX_DIAGNOSTICS));
            while kept.len() < MAX_DIAGNOSTICS {
                match seq.next_element::<Diagnostic>()? {
                    Some(diagnostic) => kept.push(diagnostic),
                    None => return Ok(kept),
                }
            }
            match seq.next_element::<serde::de::IgnoredAny>()? {
                None => Ok(kept),
                Some(_) => Err(serde::de::Error::invalid_length(MAX_DIAGNOSTICS + 1, &self)),
            }
        }
    }

    deserializer.deserialize_seq(Bounded)
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
    /// The file, as the compiler names it: relative to the workspace root for the workspace's own
    /// files.
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

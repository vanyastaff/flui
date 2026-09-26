//! [`RoutePath`] — a normalized route location — and [`RouteParseError`].

use std::borrow::Cow;
use std::fmt;

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// The bytes a path segment percent-encodes: the WHATWG URL path-segment set
/// (C0 controls, space, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`, `/`, `%`),
/// plus `\`, which special URL schemes read as a separator. Every non-ASCII
/// byte is encoded too.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// A normalized, percent-encoded route location.
///
/// It starts with `/`, has no empty segment, has no trailing `/` except the
/// root's, and carries no `?` query or `#` fragment (those are a later step).
/// Each segment is stored in one canonical encoding, so comparing two
/// `RoutePath`s compares the locations they name: `/tag/a b`, `/tag/a%20b`
/// and `/tag/a%20b/` are the same path.
///
/// ```
/// use flui_widgets::RoutePath;
///
/// let path = RoutePath::root().join("note").join(7);
/// assert_eq!(path.as_str(), "/note/7");
/// assert_eq!(RoutePath::parse("/note/7/")?, path);
/// # Ok::<(), flui_widgets::RouteParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoutePath(String);

impl RoutePath {
    /// The root location, `/`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// Validate and normalize a location.
    ///
    /// # Errors
    ///
    /// [`RouteParseError::Malformed`] when `location` is empty, does not start
    /// with `/`, has an empty segment (`//x`), carries a query or fragment, or
    /// holds an escape that is not `%` and two hex digits or does not decode to
    /// UTF-8.
    pub fn parse(location: &str) -> Result<Self, RouteParseError> {
        let malformed = |reason| RouteParseError::Malformed {
            location: location.to_owned(),
            reason,
        };
        if location.is_empty() {
            return Err(malformed("it is empty"));
        }
        let Some(rest) = location.strip_prefix('/') else {
            return Err(malformed("it does not start with `/`"));
        };
        if location.contains(['?', '#']) {
            return Err(malformed("a query or fragment is not supported"));
        }
        let mut path = Self::root();
        if rest.is_empty() {
            return Ok(path);
        }
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        for segment in rest.split('/') {
            if segment.is_empty() {
                return Err(malformed("it has an empty segment"));
            }
            if !escapes_are_well_formed(segment) {
                return Err(malformed("`%` is not followed by two hex digits"));
            }
            let decoded = percent_decode_str(segment)
                .decode_utf8()
                .map_err(|_| malformed("an escape does not decode to UTF-8"))?;
            path.push_encoded(&decoded);
        }
        Ok(path)
    }

    /// Append one segment, printed through `Display` and percent-encoded.
    ///
    /// A value that prints as the empty string appends nothing: a location has
    /// no empty segment, so a route field that can print empty does not round
    /// trip and needs a non-empty form of its own.
    #[must_use]
    pub fn join(mut self, segment: impl fmt::Display) -> Self {
        let segment = segment.to_string();
        if !segment.is_empty() {
            self.push_encoded(&segment);
        }
        self
    }

    /// The location as text, percent-encoded.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The decoded segments, first to last; the root has none.
    pub fn segments(&self) -> impl Iterator<Item = Cow<'_, str>> + '_ {
        self.0
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy())
    }

    /// Every prefix of this path, root first and this path last: `/`, `/a`,
    /// `/a/b` for `/a/b`.
    pub fn prefixes(&self) -> impl Iterator<Item = RoutePath> + '_ {
        let ends = self
            .0
            .match_indices('/')
            .map(|(index, _)| index)
            .skip(1)
            .chain(std::iter::once(self.0.len()));
        std::iter::once(Self::root()).chain(
            ends.filter(|&end| end > 1)
                .map(|end| Self(self.0[..end].to_owned())),
        )
    }

    fn push_encoded(&mut self, decoded: &str) {
        if self.0.len() > 1 {
            self.0.push('/');
        }
        self.0.extend(utf8_percent_encode(decoded, SEGMENT));
    }
}

impl fmt::Display for RoutePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether every `%` in `segment` starts a `%XX` escape of two hex digits.
fn escapes_are_well_formed(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
    })
}

/// Why a location did not produce a route.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteParseError {
    /// The text is not a route location at all.
    #[error("{location:?} is not a route location: {reason}")]
    Malformed {
        /// The text that was given.
        location: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The location is well formed, but no route matches it.
    #[error("no route matches {path}")]
    NoMatch {
        /// The location that matched nothing.
        path: RoutePath,
    },
    /// A route's pattern matches, but one segment does not parse as the field
    /// it fills.
    #[error("segment {segment:?} of {path} is not a valid `{field}`")]
    Param {
        /// The location being parsed.
        path: RoutePath,
        /// The field the segment was meant to fill.
        field: &'static str,
        /// The decoded segment that did not parse.
        segment: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn malformed(location: &str) -> bool {
        matches!(
            RoutePath::parse(location),
            Err(RouteParseError::Malformed { .. })
        )
    }

    #[test]
    fn route_path_rejects_malformed_locations() {
        for location in [
            "", "note", "//", "//x", "/a//b", "/a?b", "/a#b", "/%ZZ", "/%4", "/%C3",
        ] {
            assert!(malformed(location), "{location:?} must be Malformed");
        }
    }

    #[test]
    fn route_path_normalizes_a_trailing_slash() {
        let with = RoutePath::parse("/note/1/").expect("a trailing slash is allowed");
        let without = RoutePath::parse("/note/1").expect("a plain location parses");
        assert_eq!(with, without);
        assert_eq!(with.as_str(), "/note/1");
        assert_eq!(
            RoutePath::parse("/").expect("the root parses"),
            RoutePath::root()
        );
    }

    #[test]
    fn route_path_canonicalizes_each_segment_encoding() {
        let raw = RoutePath::parse("/tag/a b").expect("a raw space is re-encoded");
        let lower = RoutePath::parse("/tag/a%20b").expect("an escape is kept");
        assert_eq!(raw, lower);
        assert_eq!(raw.as_str(), "/tag/a%20b");
        let slash = RoutePath::parse("/tag/a%2fc").expect("an encoded slash stays one segment");
        assert_eq!(slash.as_str(), "/tag/a%2Fc");
        assert_eq!(slash.segments().collect::<Vec<_>>(), ["tag", "a/c"]);
    }

    #[test]
    fn join_encodes_and_segments_decode() {
        let path = RoutePath::root().join("tag").join("a b/c").join("é");
        assert_eq!(path.as_str(), "/tag/a%20b%2Fc/%C3%A9");
        assert_eq!(path.segments().collect::<Vec<_>>(), ["tag", "a b/c", "é"]);
        assert_eq!(RoutePath::root().join("").as_str(), "/");
    }

    #[test]
    fn prefixes_run_from_the_root_to_the_path() {
        let path = RoutePath::parse("/a/b/c").expect("parses");
        let prefixes: Vec<String> = path.prefixes().map(|p| p.as_str().to_owned()).collect();
        assert_eq!(prefixes, ["/", "/a", "/a/b", "/a/b/c"]);
        let root: Vec<RoutePath> = RoutePath::root().prefixes().collect();
        assert_eq!(root, [RoutePath::root()]);
    }
}

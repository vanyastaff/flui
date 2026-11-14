//! [`RoutePath`] — a normalized route location — and [`RouteParseError`].

use std::borrow::Cow;
use std::fmt;

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// Bytes a route segment percent-encodes: FLUI's route-path escaping set,
/// (C0 controls, space, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`), plus
/// `/` and `%` to preserve segment boundaries and escapes, and `\`, which
/// special URL schemes read as a separator. Non-ASCII bytes are encoded too.
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

/// Bytes a query key or value percent-encodes: the segment set without `/`
/// and `?`, which a query may hold as data, plus `&`, `=` and `+`, which
/// delimit pairs or would read as a space.
const QUERY_COMPONENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'\\')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// A normalized, percent-encoded route location: a path and an optional
/// query.
///
/// The path starts with `/`, has no empty segment and no trailing `/` except
/// the root's. The query is a list of `key=value` pairs in the order given; a
/// key may repeat, and an empty value prints as the bare key. There is no
/// `#` fragment. Each segment, key and value is stored in one canonical
/// encoding, so comparing two `RoutePath`s compares the locations they name:
/// `/tag/a b`, `/tag/a%20b` and `/tag/a%20b/` are the same path, and so are
/// `/search?q=a+b` and `/search?q=a%20b`.
///
/// Escapes are decoded exactly once. In the path a literal `+` stays `+`; in
/// the query it is a space, as a form and Flutter's `Uri.queryParameters`
/// read it. Encoded `/`, `?`, `#` and backslashes remain data within one
/// segment, while malformed escapes and invalid UTF-8 are rejected. This is
/// route-location normalization, not whole-URL parsing: `.` and `..` remain
/// literal route values rather than resolving relative to another location.
///
/// ```
/// use flui_view::navigation::RoutePath;
///
/// let path = RoutePath::root().join("note").join(7);
/// assert_eq!(path.as_str(), "/note/7");
/// assert_eq!(RoutePath::parse("/note/7/")?, path);
///
/// let search = RoutePath::root().join("search").with_query("q", "a b");
/// assert_eq!(search.as_str(), "/search?q=a%20b");
/// assert_eq!(RoutePath::parse("/search?q=a+b")?, search);
/// # Ok::<(), flui_view::navigation::RouteParseError>(())
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
    /// with `/`, has an empty segment (`//x`), carries a fragment, or holds an
    /// escape that is not `%` and two hex digits or does not decode to UTF-8.
    pub fn parse(location: &str) -> Result<Self, RouteParseError> {
        let malformed = |reason| RouteParseError::Malformed {
            location: location.to_owned(),
            reason,
        };
        if location.is_empty() {
            return Err(malformed("it is empty"));
        }
        if location.contains('#') {
            return Err(malformed("a fragment is not supported"));
        }
        let (path_part, query) = match location.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (location, None),
        };
        let Some(rest) = path_part.strip_prefix('/') else {
            return Err(malformed("it does not start with `/`"));
        };
        let mut path = Self::root();
        let rest = rest.trim_suffix('/');
        if !rest.is_empty() {
            for segment in rest.split('/') {
                if segment.is_empty() {
                    return Err(malformed("it has an empty segment"));
                }
                path.push_encoded(&decode(segment, false).map_err(malformed)?);
            }
        }
        for pair in query.into_iter().flat_map(|query| query.split('&')) {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = decode(key, true).map_err(malformed)?;
            let value = decode(value, true).map_err(malformed)?;
            path.push_query(&key, &value);
        }
        Ok(path)
    }

    /// Append one path segment, printed through `Display` and
    /// percent-encoded. A query, if any, stays after it.
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

    /// Append the query pair `key=value`, each printed through `Display` and
    /// percent-encoded, after the pairs already there. A pair whose key and
    /// value both print empty appends nothing, as an empty segment does.
    #[must_use]
    pub fn with_query(mut self, key: impl fmt::Display, value: impl fmt::Display) -> Self {
        self.push_query(&key.to_string(), &value.to_string());
        self
    }

    /// The location as text, percent-encoded.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The decoded path segments, first to last; the root has none.
    pub fn segments(&self) -> impl Iterator<Item = Cow<'_, str>> + '_ {
        self.path_part()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy())
    }

    /// The decoded query pairs, in order; a repeated key appears once per
    /// pair.
    pub fn query(&self) -> impl Iterator<Item = (Cow<'_, str>, Cow<'_, str>)> + '_ {
        self.query_part()
            .into_iter()
            .flat_map(|query| query.split('&'))
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (
                    percent_decode_str(key).decode_utf8_lossy(),
                    percent_decode_str(value).decode_utf8_lossy(),
                )
            })
    }

    /// Every prefix of this location, root first and this location last: `/`,
    /// `/a`, `/a/b?q=1` for `/a/b?q=1`. Only the last carries the query.
    pub fn prefixes(&self) -> impl Iterator<Item = RoutePath> + '_ {
        let path = self.path_part();
        let ends = path.match_indices('/').map(|(index, _)| index).skip(1);
        let root = (path.len() > 1).then(Self::root);
        root.into_iter()
            .chain(ends.map(|end| Self(path[..end].to_owned())))
            .chain(std::iter::once(self.clone()))
    }

    /// The path, without the query.
    fn path_part(&self) -> &str {
        self.0.split_once('?').map_or(&self.0, |(path, _)| path)
    }

    /// The encoded query, without its `?`; `None` when there is none.
    fn query_part(&self) -> Option<&str> {
        self.0.split_once('?').map(|(_, query)| query)
    }

    fn push_encoded(&mut self, decoded: &str) {
        let query = self.0.find('?').map(|at| self.0.split_off(at));
        if self.0.len() > 1 {
            self.0.push('/');
        }
        self.0.extend(utf8_percent_encode(decoded, SEGMENT));
        if let Some(query) = query {
            self.0.push_str(&query);
        }
    }

    fn push_query(&mut self, key: &str, value: &str) {
        // A pair with neither key nor value would print a bare `?` or `&`,
        // which parses back to no pair: it is no pair here either.
        if key.is_empty() && value.is_empty() {
            return;
        }
        self.0.push(if self.0.contains('?') { '&' } else { '?' });
        self.0.extend(utf8_percent_encode(key, QUERY_COMPONENT));
        if !value.is_empty() {
            self.0.push('=');
            self.0.extend(utf8_percent_encode(value, QUERY_COMPONENT));
        }
    }
}

impl fmt::Display for RoutePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `text` with its escapes decoded once, and `+` read as a space when
/// `plus_is_space` (a query component).
fn decode(text: &str, plus_is_space: bool) -> Result<Cow<'_, str>, &'static str> {
    if !escapes_are_well_formed(text) {
        return Err("`%` is not followed by two hex digits");
    }
    let spaced = if plus_is_space && text.contains('+') {
        Cow::Owned(text.replace('+', " "))
    } else {
        Cow::Borrowed(text)
    };
    let decoded = percent_decode_str(&spaced)
        .decode_utf8()
        .map_err(|_| "an escape does not decode to UTF-8")?;
    Ok(Cow::Owned(decoded.into_owned()))
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

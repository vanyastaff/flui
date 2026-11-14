//! A URL the platform opened for the application, and which of them the
//! application declared it handles (ADR-0330 §5, ADR-0307).

use super::{NavigationRefused, RouteParseError, RoutePath};

/// A URL the platform opened for the application: a link the user followed,
/// a custom-scheme URL another application launched, or one the command
/// line carried. Split as RFC 3986, appendix B splits it, with the path and
/// query turned into the [`RoutePath`] the application's Router opens.
///
/// The path is the URL's path, as Flutter and go_router read `uri.path`: the
/// authority is not part of it, so `myapp:///note/3` and
/// `https://example.com/note/3` open `/note/3`, while `myapp://note/3` opens
/// `/3`, its `note` being the host. An empty path opens `/`, and a path with
/// no leading `/` (`myapp:note/3`) opens as if it had one. The fragment
/// addresses nothing in a route location and is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedUrl {
    /// The scheme, in lower case.
    scheme: String,
    /// The authority's host, in lower case, without user information, port or
    /// a trailing dot; `None` when the URL has no authority or an empty host.
    host: Option<String>,
    path: RoutePath,
}

impl OpenedUrl {
    /// The longest URL, in bytes, that [`parse`](Self::parse) reads: a URL
    /// the platform hands over is untrusted input, and its length is checked
    /// before any of it is copied.
    pub const MAX_LEN: usize = flui_platform::MAX_OPENED_URL_LEN;

    /// Split `url` into its scheme, host and route location.
    ///
    /// # Errors
    ///
    /// [`OpenUrlRefused::TooLong`] for a URL over [`Self::MAX_LEN`] bytes;
    /// [`OpenUrlRefused::Malformed`] for text with no scheme or a scheme RFC
    /// 3986 does not allow; [`OpenUrlRefused::Path`] when the path and query
    /// are not a route location (an empty segment, a bad escape).
    pub fn parse(url: &str) -> Result<Self, OpenUrlRefused> {
        if url.len() > Self::MAX_LEN {
            return Err(OpenUrlRefused::TooLong { len: url.len() });
        }
        let malformed = |reason| OpenUrlRefused::Malformed {
            url: url.to_owned(),
            reason,
        };
        let Some((scheme, rest)) = url
            .find([':', '/', '?', '#'])
            .filter(|&at| at > 0 && url.as_bytes()[at] == b':')
            .map(|at| (&url[..at], &url[at + 1..]))
        else {
            return Err(malformed("it has no scheme"));
        };
        if !is_scheme(scheme) {
            return Err(malformed(
                "its scheme is not a letter followed by letters, digits, `+`, `-` or `.`",
            ));
        }
        let rest = match rest.split_once('#') {
            Some((before, _fragment)) => {
                tracing::debug!(
                    scheme,
                    "an opened URL's fragment addresses no route and is dropped"
                );
                before
            }
            None => rest,
        };
        let (rest, query) = match rest.split_once('?') {
            Some((before, query)) => (before, Some(query)),
            None => (rest, None),
        };
        let (host, path) = match rest.strip_prefix("//") {
            Some(after) => {
                let end = after.find('/').unwrap_or(after.len());
                (host_of(&after[..end]), &after[end..])
            }
            None => (None, rest),
        };
        let mut location = String::with_capacity(path.len() + query.map_or(0, str::len) + 2);
        if !path.starts_with('/') {
            location.push('/');
        }
        location.push_str(path);
        if let Some(query) = query {
            location.push('?');
            location.push_str(query);
        }
        let path = RoutePath::parse(&location).map_err(OpenUrlRefused::Path)?;
        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
            host,
            path,
        })
    }

    /// The scheme, in lower case: `https`, or an application's own `myapp`.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The host, in lower case, or `None` when the URL names none.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// The route location the URL opens.
    #[must_use]
    pub fn path(&self) -> &RoutePath {
        &self.path
    }
}

/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`, RFC 3986 §3.1.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// The host of `authority` (`userinfo@host:port`), lower-cased, without a
/// trailing dot.
fn host_of(authority: &str) -> Option<String> {
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host_port)| host_port);
    let host = if host_port.starts_with('[') {
        // An IP literal keeps its colons; the port follows the bracket.
        host_port
            .find(']')
            .map_or(host_port, |end| &host_port[..=end])
    } else {
        host_port
            .rsplit_once(':')
            .map_or(host_port, |(host, _port)| host)
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The URLs an application declares it opens: its own schemes and the web
/// domains it is associated with, as `[package.metadata.flui.declares]`
/// lists them under `url-schemes` and `associated-domains` (ADR-0307).
///
/// The platform hands an application only the URLs it registered for, but a
/// command line or a second instance can carry any text, so the UI runtime
/// admits an opened URL only through this list. Empty by default: an
/// application that declares nothing opens no URL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkDeclarations {
    url_schemes: &'static [&'static str],
    associated_domains: &'static [&'static str],
}

impl LinkDeclarations {
    /// No scheme and no domain.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            url_schemes: &[],
            associated_domains: &[],
        }
    }

    /// The application's own URL schemes (`myapp`), compared without regard
    /// to case. `http` and `https` here admit nothing: a web URL is admitted
    /// by its domain.
    #[must_use]
    pub const fn url_schemes(self, url_schemes: &'static [&'static str]) -> Self {
        Self {
            url_schemes,
            ..self
        }
    }

    /// The web domains whose `http` and `https` URLs the application opens
    /// (`example.com`), each compared whole and without regard to case: a
    /// subdomain is a domain of its own.
    #[must_use]
    pub const fn associated_domains(self, associated_domains: &'static [&'static str]) -> Self {
        Self {
            associated_domains,
            ..self
        }
    }

    /// Whether the application declared `url`: an `http` or `https` URL by
    /// its host, any other by its scheme.
    ///
    /// # Errors
    ///
    /// [`OpenUrlRefused::UndeclaredDomain`] for a web URL whose host is not an
    /// associated domain, or that has none; [`OpenUrlRefused::UndeclaredScheme`]
    /// for any other scheme the application did not declare.
    pub fn admit(&self, url: &OpenedUrl) -> Result<(), OpenUrlRefused> {
        let declared = |list: &[&str], value: &str| {
            list.iter()
                .any(|entry| entry.trim_end_matches('.').eq_ignore_ascii_case(value))
        };
        if matches!(url.scheme(), "http" | "https") {
            return match url.host() {
                Some(host) if declared(self.associated_domains, host) => Ok(()),
                host => Err(OpenUrlRefused::UndeclaredDomain {
                    host: host.map(str::to_owned),
                }),
            };
        }
        if declared(self.url_schemes, url.scheme()) {
            Ok(())
        } else {
            Err(OpenUrlRefused::UndeclaredScheme {
                scheme: url.scheme().to_owned(),
            })
        }
    }
}

/// Why a URL the platform opened was not applied. The stack is unchanged.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenUrlRefused {
    /// The URL is longer than [`OpenedUrl::MAX_LEN`] bytes, and none of it
    /// was read.
    #[error(
        "the URL is {len} bytes, over the {} an opened URL may hold",
        OpenedUrl::MAX_LEN
    )]
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The text is not an absolute URL.
    #[error("{url:?} is not a URL: {reason}")]
    Malformed {
        /// The text that was given.
        url: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The URL's path and query are not a route location.
    #[error("the URL's path is not a route location: {0}")]
    Path(RouteParseError),
    /// The scheme is not one of the application's
    /// [`url_schemes`](LinkDeclarations::url_schemes).
    #[error("the application declares no URL scheme {scheme:?}")]
    UndeclaredScheme {
        /// The scheme, in lower case.
        scheme: String,
    },
    /// A web URL whose host is not one of the application's
    /// [`associated_domains`](LinkDeclarations::associated_domains).
    #[error("the application declares no associated domain {host:?}")]
    UndeclaredDomain {
        /// The host, in lower case, or `None` for a web URL with none.
        host: Option<String>,
    },
    /// The URL was admitted, and its presentation's URL owner refused the
    /// navigation.
    #[error(transparent)]
    Navigation(#[from] NavigationRefused),
}

impl OpenUrlRefused {
    /// What kind of refusal this is, in words that name no part of the URL,
    /// for a log line: an opened link may carry a token, which its
    /// [`Display`](std::fmt::Display) would print.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::TooLong { .. } => "too long",
            Self::Malformed { .. } => "not a URL",
            Self::Path(_) => "not a route location",
            Self::UndeclaredScheme { .. } => "undeclared scheme",
            Self::UndeclaredDomain { .. } => "undeclared domain",
            Self::Navigation(_) => "navigation refused",
        }
    }
}

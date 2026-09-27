//! What `#[derive(Routable)]` generates calls: no semver guarantee, and no
//! caller but the generated code.

use std::borrow::Cow;
use std::fmt::Display;
use std::str::FromStr;

use super::path::{RouteParseError, RoutePath};

/// The decoded segments of `path`, first to last.
#[must_use]
pub fn segments(path: &RoutePath) -> Vec<Cow<'_, str>> {
    path.segments().collect()
}

/// Parses a candidate's fields and remembers the first field that did not
/// parse, so a location that no candidate matches in full reports it.
#[derive(Debug)]
pub struct Matcher<'a> {
    path: &'a RoutePath,
    first_param_error: Option<RouteParseError>,
}

impl<'a> Matcher<'a> {
    /// A matcher for `path`.
    #[must_use]
    pub fn new(path: &'a RoutePath) -> Self {
        Self {
            path,
            first_param_error: None,
        }
    }

    /// `segment` parsed as the field `field`, or `None` after recording the
    /// failure.
    pub fn field<T: FromStr>(&mut self, field: &'static str, segment: &str) -> Option<T> {
        let parsed = segment.parse().ok();
        if parsed.is_none() {
            self.first_param_error
                .get_or_insert_with(|| RouteParseError::Param {
                    path: self.path.clone(),
                    field,
                    segment: segment.to_owned(),
                });
        }
        parsed
    }

    /// Why no candidate matched: the first field that did not parse in a
    /// candidate whose literals matched, else `NoMatch`.
    #[must_use]
    pub fn finish(self) -> RouteParseError {
        self.first_param_error
            .unwrap_or_else(|| RouteParseError::NoMatch {
                path: self.path.clone(),
            })
    }
}

/// Compiles only when `T` can fill a route segment; the derive calls it with
/// each field's span, so a missing bound is reported on the field.
#[inline]
pub fn assert_segment<T: FromStr + Display>() {}

//! What `#[derive(Routable)]` generates calls: no semver guarantee, and no
//! caller but the generated code.

use std::borrow::Cow;
use std::fmt::Display;
use std::str::FromStr;

use flui_view::navigation::{RouteParseError, RoutePath};

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

    /// The first value of the query key `key` parsed as the field `field`:
    /// `Some(None)` without the key, `None` after recording a value that did
    /// not parse.
    pub fn query_optional<T: FromStr>(
        &mut self,
        field: &'static str,
        key: &str,
    ) -> Option<Option<T>> {
        let path = self.path;
        let value = path
            .query()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value);
        match value {
            None => Some(None),
            Some(value) => self.field(field, &value).map(Some),
        }
    }

    /// Every value of the query key `key`, in order, parsed as the field
    /// `field`; `None` after recording the first that did not parse.
    pub fn query_repeated<T: FromStr>(&mut self, field: &'static str, key: &str) -> Option<Vec<T>> {
        let path = self.path;
        let values: Vec<Cow<'_, str>> = path
            .query()
            .filter(|(candidate, _)| candidate == key)
            .map(|(_, value)| value)
            .collect();
        values
            .iter()
            .map(|value| self.field(field, value))
            .collect::<Option<Vec<T>>>()
    }

    /// The first value of the query key `key` parsed as the field `field`,
    /// or `T::default()` without the key; `None` after recording a value that
    /// did not parse.
    pub fn query_defaulted<T: FromStr + Default>(
        &mut self,
        field: &'static str,
        key: &str,
    ) -> Option<T> {
        self.query_optional(field, key)
            .map(Option::unwrap_or_default)
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

/// Compiles only when `T` can fill a `#[query]` field that is neither
/// `Option` nor `Vec`, which takes its default without the key.
#[inline]
pub fn assert_query_defaulted<T: FromStr + Display + Default>() {}

//! Where a value was created: the caller of a `#[track_caller]` constructor.
//!
//! A site is a display field for diagnostics, errors and traces. It never takes part in
//! identity or equality. It is kept in debug builds and in any build that enables the
//! `creation-site` feature, and is zero-sized otherwise, so profiling release builds and
//! agents can ask for sites without every release build paying for them.

use std::fmt;
use std::panic::Location;

/// Where a value was created.
///
/// Constructors and builders that are the documented way to make a value take
/// `#[track_caller]` and call [`CreationSite::here`]. A value made by struct literal has
/// [`CreationSite::unknown`].
#[derive(Clone, Copy, Default)]
pub struct CreationSite(
    #[cfg(any(debug_assertions, feature = "creation-site"))] Option<&'static Location<'static>>,
);

impl CreationSite {
    /// Whether this build keeps sites: debug builds, or the `creation-site` feature.
    pub const KEPT: bool = cfg!(any(debug_assertions, feature = "creation-site"));

    /// The site of the caller. Inside a `#[track_caller]` function this is that
    /// function's caller.
    #[must_use]
    #[track_caller]
    pub fn here() -> Self {
        #[cfg(any(debug_assertions, feature = "creation-site"))]
        {
            Self(Some(Location::caller()))
        }
        #[cfg(not(any(debug_assertions, feature = "creation-site")))]
        {
            Self()
        }
    }

    /// No site: the value was not made by a tracked constructor.
    #[must_use]
    pub const fn unknown() -> Self {
        #[cfg(any(debug_assertions, feature = "creation-site"))]
        {
            Self(None)
        }
        #[cfg(not(any(debug_assertions, feature = "creation-site")))]
        {
            Self()
        }
    }

    /// The recorded location, when this build keeps sites and the value has one.
    #[must_use]
    pub fn location(self) -> Option<&'static Location<'static>> {
        #[cfg(any(debug_assertions, feature = "creation-site"))]
        {
            self.0
        }
        #[cfg(not(any(debug_assertions, feature = "creation-site")))]
        {
            None
        }
    }

    /// The source file, when known.
    #[must_use]
    pub fn file(self) -> Option<&'static str> {
        self.location().map(Location::file)
    }

    /// The 1-based line, when known.
    #[must_use]
    pub fn line(self) -> Option<u32> {
        self.location().map(Location::line)
    }

    /// The 1-based column, when known.
    #[must_use]
    pub fn column(self) -> Option<u32> {
        self.location().map(Location::column)
    }
}

impl fmt::Debug for CreationSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.location() {
            Some(location) => write!(f, "CreationSite({location})"),
            None => f.write_str("CreationSite(unknown)"),
        }
    }
}

/// `file:line:col`, or nothing when the site is unknown.
impl fmt::Display for CreationSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.location() {
            Some(location) => write!(f, "{location}"),
            None => Ok(()),
        }
    }
}

/// Sites never take part in equality: two values are the same value wherever they were made.
impl PartialEq for CreationSite {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for CreationSite {}

impl std::hash::Hash for CreationSite {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn made() -> CreationSite {
        CreationSite::here()
    }

    #[test]
    fn a_tracked_constructor_records_its_caller_not_itself() {
        let site = made();
        let expected_line = line!() - 1;
        if CreationSite::KEPT {
            assert_eq!(site.line(), Some(expected_line));
            assert!(
                site.file()
                    .is_some_and(|file| file.ends_with("creation_site.rs"))
            );
            assert_eq!(
                site.to_string(),
                format!(
                    "{}:{}:{}",
                    site.file().unwrap(),
                    expected_line,
                    site.column().unwrap()
                )
            );
        } else {
            assert_eq!(site.location(), None);
        }
    }

    #[test]
    fn an_unknown_site_has_no_location_and_displays_nothing() {
        let site = CreationSite::unknown();
        assert_eq!(site.location(), None);
        assert_eq!(site.to_string(), "");
        assert_eq!(CreationSite::default().location(), None);
    }

    #[test]
    fn sites_do_not_take_part_in_equality() {
        assert_eq!(made(), CreationSite::unknown());
    }
}

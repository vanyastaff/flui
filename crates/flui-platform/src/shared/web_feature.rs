//! Browser capabilities, detected by what the page can call.
//!
//! A browser's version says nothing reliable about what it offers: a feature is
//! behind a flag, withdrawn, or available only in secure contexts. The platform
//! therefore asks for the capability itself, once, and keeps the answer as a
//! [`WebFeature`]: a zero-sized proof that the named function exists. Code that
//! calls the function takes the proof as a parameter, so it cannot be written
//! without the check having happened, and an absent feature gives no handle at
//! all rather than a `false` to forget.

use std::marker::PhantomData;

/// A browser function a page may or may not have, named by its path from the
/// global object.
pub(crate) trait Feature {
    /// The path of the function, dot separated: `matchMedia`,
    /// `navigator.permissions.query`.
    const PATH: &'static str;
}

/// `window.matchMedia`: media queries and their change events.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MatchMedia;

impl Feature for MatchMedia {
    const PATH: &'static str = "matchMedia";
}

/// `window.getScreenDetails`: every screen of the machine (Window Management).
#[derive(Debug, Clone, Copy)]
pub(crate) struct GetScreenDetails;

impl Feature for GetScreenDetails {
    const PATH: &'static str = "getScreenDetails";
}

/// `navigator.permissions.query`: read a permission without asking for it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PermissionsQuery;

impl Feature for PermissionsQuery {
    const PATH: &'static str = "navigator.permissions.query";
}

/// Where a capability is looked up: the browser's global object, or a fake in a
/// test.
pub(crate) trait FeatureProbe {
    /// Whether `path` resolves to a function.
    fn has_function(&self, path: &str) -> bool;
}

/// Proof that the browser has feature `F`.
///
/// Obtainable only from [`detect`](Self::detect), which found the function.
#[derive(Debug)]
pub(crate) struct WebFeature<F> {
    feature: PhantomData<fn() -> F>,
}

impl<F> Clone for WebFeature<F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for WebFeature<F> {}

impl<F: Feature> WebFeature<F> {
    /// The proof, or `None` when the browser lacks the feature.
    pub(crate) fn detect(probe: &impl FeatureProbe) -> Option<Self> {
        probe.has_function(F::PATH).then_some(Self {
            feature: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Feature, FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery, WebFeature};

    /// A browser that offers exactly these functions.
    struct Browser(&'static [&'static str]);

    impl FeatureProbe for Browser {
        fn has_function(&self, path: &str) -> bool {
            self.0.contains(&path)
        }
    }

    #[test]
    fn web_feature_absent_gives_no_handle() {
        let desktop_chrome = Browser(&[
            MatchMedia::PATH,
            GetScreenDetails::PATH,
            PermissionsQuery::PATH,
        ]);
        assert!(WebFeature::<GetScreenDetails>::detect(&desktop_chrome).is_some());
        assert!(WebFeature::<PermissionsQuery>::detect(&desktop_chrome).is_some());

        // Firefox and Safari have no Window Management API; the detection does
        // not depend on any version string, only on the function.
        let without_window_management = Browser(&[MatchMedia::PATH, PermissionsQuery::PATH]);
        assert!(WebFeature::<GetScreenDetails>::detect(&without_window_management).is_none());
        assert!(WebFeature::<MatchMedia>::detect(&without_window_management).is_some());

        let bare = Browser(&[]);
        assert!(WebFeature::<MatchMedia>::detect(&bare).is_none());
        assert!(WebFeature::<PermissionsQuery>::detect(&bare).is_none());
    }

    #[test]
    fn a_feature_is_found_by_its_whole_path() {
        // `navigator.permissions` without `query` is not the capability.
        let partial = Browser(&["navigator.permissions"]);
        assert!(WebFeature::<PermissionsQuery>::detect(&partial).is_none());
    }
}

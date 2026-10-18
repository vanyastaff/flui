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
    /// What the path must resolve to.
    const KIND: FeatureKind;
}

/// What a feature path resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureKind {
    /// A callable function.
    Function,
    /// A property that exists, even when its value is `null` (an event handler
    /// slot such as `onchange` is `null` until set).
    Property,
}

/// `window.matchMedia`: media queries and their change events.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MatchMedia;

impl Feature for MatchMedia {
    const PATH: &'static str = "matchMedia";
    const KIND: FeatureKind = FeatureKind::Function;
}

/// `window.getScreenDetails`: every screen of the machine (Window Management).
#[derive(Debug, Clone, Copy)]
pub(crate) struct GetScreenDetails;

impl Feature for GetScreenDetails {
    const PATH: &'static str = "getScreenDetails";
    const KIND: FeatureKind = FeatureKind::Function;
}

/// `navigator.permissions.query`: read a permission without asking for it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PermissionsQuery;

impl Feature for PermissionsQuery {
    const PATH: &'static str = "navigator.permissions.query";
    const KIND: FeatureKind = FeatureKind::Function;
}

/// `window.screen.onchange`: the screen object raises `change` when its size,
/// pixel ratio or position changes (Chromium; Window Management not needed).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenChange;

impl Feature for ScreenChange {
    const PATH: &'static str = "screen.onchange";
    const KIND: FeatureKind = FeatureKind::Property;
}

/// Where a capability is looked up: the browser's global object, or a fake in a
/// test.
pub(crate) trait FeatureProbe {
    /// Whether `path` resolves to something of `kind`.
    fn has(&self, path: &str, kind: FeatureKind) -> bool;
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
        probe.has(F::PATH, F::KIND).then_some(Self {
            feature: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Feature, FeatureKind, FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery,
        ScreenChange, WebFeature,
    };

    /// A browser that offers exactly these functions, and has these properties
    /// (present even when their value is `null`).
    struct Browser(&'static [&'static str], &'static [&'static str]);

    impl FeatureProbe for Browser {
        fn has(&self, path: &str, kind: FeatureKind) -> bool {
            match kind {
                FeatureKind::Function => self.0.contains(&path),
                FeatureKind::Property => self.1.contains(&path),
            }
        }
    }

    #[test]
    fn web_feature_absent_gives_no_handle() {
        let desktop_chrome = Browser(
            &[
                MatchMedia::PATH,
                GetScreenDetails::PATH,
                PermissionsQuery::PATH,
            ],
            &[],
        );
        assert!(WebFeature::<GetScreenDetails>::detect(&desktop_chrome).is_some());
        assert!(WebFeature::<PermissionsQuery>::detect(&desktop_chrome).is_some());

        // Firefox and Safari have no Window Management API; the detection does
        // not depend on any version string, only on the function.
        let without_window_management = Browser(&[MatchMedia::PATH, PermissionsQuery::PATH], &[]);
        assert!(WebFeature::<GetScreenDetails>::detect(&without_window_management).is_none());
        assert!(WebFeature::<MatchMedia>::detect(&without_window_management).is_some());

        let bare = Browser(&[], &[]);
        assert!(WebFeature::<MatchMedia>::detect(&bare).is_none());
        assert!(WebFeature::<PermissionsQuery>::detect(&bare).is_none());
    }

    #[test]
    fn a_feature_is_found_by_its_whole_path() {
        // `navigator.permissions` without `query` is not the capability.
        let partial = Browser(&["navigator.permissions"], &[]);
        assert!(WebFeature::<PermissionsQuery>::detect(&partial).is_none());
    }

    #[test]
    fn a_null_handler_slot_is_a_present_property() {
        // `screen.onchange` is `null` where `change` is supported: the property
        // exists, no function does. A function feature is not satisfied by a
        // property of the same name.
        let chromium = Browser(&[], &[ScreenChange::PATH]);
        assert!(WebFeature::<ScreenChange>::detect(&chromium).is_some());
        let without = Browser(&[ScreenChange::PATH], &[]);
        assert!(WebFeature::<ScreenChange>::detect(&without).is_none());
        let property_only = Browser(&[], &[MatchMedia::PATH]);
        assert!(WebFeature::<MatchMedia>::detect(&property_only).is_none());
    }
}

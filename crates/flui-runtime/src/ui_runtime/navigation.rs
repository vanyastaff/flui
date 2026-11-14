//! Navigation addressed to a presentation from outside its widgets (ADR-0330 §5).

use flui_foundation::PresentationId;
use flui_view::navigation::{
    LinkDeclarations, NavigationIntent, NavigationRefused, OpenUrlRefused, OpenedUrl,
};

use super::UiRuntime;

impl UiRuntime {
    /// Apply `intent` to presentation `presentation`'s URL owner, the
    /// outermost `Router` of its tree, as Flutter's Router applies the route
    /// information the platform reports. This is where a location a
    /// development tool asks for enters the tree, and where
    /// [`open_url`](Self::open_url) hands an admitted URL's path. The owner
    /// runs inside this UI runtime's owner scope, and its edit is built on
    /// the next frame.
    ///
    /// # Errors
    ///
    /// [`NavigationRefused::NoPresentation`] when this UI runtime hosts no
    /// such presentation; [`NavigationRefused::NoUrlOwner`] when its tree has
    /// no `Router`; and whatever the Router refuses with
    /// ([`NavigationRefused::NoMatch`] for a path none of its routes match,
    /// [`NavigationRefused::Busy`] in the middle of another edit or a build).
    /// A refused intent leaves the stack unchanged.
    pub fn navigate(
        &self,
        presentation: PresentationId,
        intent: NavigationIntent,
    ) -> Result<(), NavigationRefused> {
        let slot = self
            .presentations
            .get(presentation)
            .map(|state| state.url_owner.clone())
            .ok_or(NavigationRefused::NoPresentation)?;
        self.enter(|_| slot.navigate(intent))
    }

    /// Open `url`, a URL the platform opened for presentation
    /// `presentation`, as Flutter's engine hands a deep link to its Router:
    /// the URL is split ([`OpenedUrl::parse`]), admitted by the application's
    /// [`LinkDeclarations`], and its path is applied as a
    /// [`NavigationOp::Go`](flui_view::navigation::NavigationOp::Go), so the
    /// pages it shares with the current stack stay. A URL that arrives before
    /// the presentation's first frame, when no `Router` has mounted yet,
    /// becomes the location the first Router
    /// opens on (a cold start). Every host and the headless one open URLs
    /// through here, so the length bound, the declaration check and the cold
    /// start hold on every platform.
    ///
    /// # Errors
    ///
    /// The [`OpenUrlRefused`] of a URL that is too long, malformed or not
    /// declared, and [`OpenUrlRefused::Navigation`] with what
    /// [`navigate`](Self::navigate) refuses (`NoUrlOwner` only after the
    /// first frame). A refused URL leaves the stack unchanged.
    pub fn open_url(&self, presentation: PresentationId, url: &str) -> Result<(), OpenUrlRefused> {
        let url = OpenedUrl::parse(url)?;
        self.link_declarations.get().admit(&url)?;
        let slot = self
            .presentations
            .get(presentation)
            .map(|state| state.url_owner.clone())
            .ok_or(NavigationRefused::NoPresentation)?;
        let path = url.path().clone();
        Ok(self.enter(|_| slot.open_at(path))?)
    }

    /// Set the URLs the application declares it opens, which
    /// [`open_url`](Self::open_url) admits from then on. The runner sets
    /// them from the application's `[package.metadata.flui.declares]`
    /// before the platform delivers the first URL.
    pub fn set_link_declarations(&self, declarations: LinkDeclarations) {
        self.link_declarations.set(declarations);
    }
}

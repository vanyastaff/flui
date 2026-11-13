//! Navigation addressed to a presentation from outside its widgets (ADR-0330 §5).

use flui_foundation::PresentationId;
use flui_view::navigation::{NavigationIntent, NavigationRefused};

use super::UiRuntime;

impl UiRuntime {
    /// Apply `intent` to presentation `presentation`'s URL owner, the
    /// outermost `Router` of its tree, as Flutter's Router applies the route
    /// information the platform reports. This is where a URL the platform
    /// opens for a window, or a location a development tool asks for, enters
    /// the tree. The owner runs inside this UI runtime's owner scope, and its
    /// edit is built on the next frame.
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
}

//! Attaching root widgets to a presentation.

use super::UiRuntime;
#[cfg(any(test, feature = "testing"))]
use flui_foundation::PresentationId;
use flui_view::environment::{FocusTreeRoot, GestureArenaScope, PlatformConvention, VsyncScope};

impl UiRuntime {
    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build. Forwards
    /// to [`flui_view::__runtime::WidgetsBinding::attach_root_widget`] — the single
    /// root-bootstrap path; every runner entry point calls it, not a separate
    /// hand-rolled wiring.
    ///
    /// # Implicit-animation and gesture-arena auto-wrap
    ///
    /// The UI runtime automatically wraps `view` in a [`VsyncScope`] backed by
    /// [`Self::vsync`] and a [`GestureArenaScope`] backed by this
    /// presentation's gesture arena before handing it to the element tree
    /// (see `Self::attach_root_widget_entered`'s body) — every implicitly-
    /// animated widget and every `GestureDetector` below the root joins this
    /// UI runtime's own registry/arena with no app-author boilerplate. Never
    /// mount a second `VsyncScope`/`GestureArenaScope` at the root with a
    /// *different* registry/arena — this UI runtime ticks/arbitrates its own
    /// while descendants register into the other, leaving them frozen or
    /// competing in an arena nothing closes.
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::__runtime::AttachError`] the underlying
    /// [`flui_view::__runtime::WidgetsBinding::attach_root_widget`] returns.
    pub fn attach_root_widget<V>(&self, view: &V) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| ui_runtime.attach_root_widget_entered(view))
    }

    fn attach_root_widget_entered<V>(
        &self,
        view: &V,
    ) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Install this ui_runtime's three exact owner-driven capabilities.
        // GestureArenaScope is outermost so every descendant recognizer
        // shares one binding-driven arena, VsyncScope carries the animation
        // registry, and FocusTreeRoot publishes this presentation's exact focus
        // tree. These wrappers have no render object, so the render root is
        // unchanged.
        // The root MediaQuery publishes the ui_runtime's live platform data
        // (size, device pixel ratio, brightness) to the whole user subtree;
        // the ui_runtime's resize/appearance arms write the shared source and the
        // wrapper republishes.
        let with_media_query = crate::media_query_root::MediaQueryRoot::new(
            std::rc::Rc::clone(self.media_query()),
            flui_view::ViewExt::boxed(view.clone()),
        );
        let focused = FocusTreeRoot::new(PlatformConvention::new(
            self.presentations.primary().platform,
            with_media_query,
        ));
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated)
            .settings(self.presentations.primary().gesture_settings.provider())
            .wheel_preferences(self.presentations.primary().wheel_preferences.provider());
        self.presentations
            .primary()
            .widgets()
            .attach_root_widget(&wrapped)?;
        self.request_redraw();
        tracing::debug!("Root widget attached");
        Ok(())
    }

    /// [`Self::attach_root_widget`], but targets an arbitrary RESIDENT
    /// presentation instead of always the primary one — for tests that need a
    /// SECOND presentation to carry real, paintable content. A frame paints
    /// nothing for a presentation with nothing attached (no layer tree), so a
    /// multi-presentation test proving telemetry attribution follows
    /// the presentation that actually produced (not `primary()`
    /// unconditionally) needs the non-primary presentation to genuinely
    /// paint, not merely flip a dirty bit.
    ///
    /// # Panics
    ///
    /// Panics if `id` does not name a presentation this UI runtime currently
    /// hosts.
    #[cfg(any(test, feature = "testing"))]
    pub fn attach_root_widget_to_for_test<V>(
        &self,
        id: PresentationId,
        view: &V,
    ) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| {
            let presentation = ui_runtime.presentations.get(id).expect(
                "BUG: attach_root_widget_to_for_test given a presentation id this ui_runtime does \
                 not host",
            );
            let focused = FocusTreeRoot::new(view.clone());
            let animated = VsyncScope::new(presentation.vsync(), focused);
            let wrapped = GestureArenaScope::new(presentation.gestures().arena().clone(), animated)
                .settings(presentation.gesture_settings.provider())
                .wheel_preferences(presentation.wheel_preferences.provider());
            presentation.widgets().attach_root_widget(&wrapped)?;
            ui_runtime.request_redraw_for(presentation);
            tracing::debug!(?id, "Root widget attached (non-primary, test-only)");
            Ok(())
        })
    }

    /// [`Self::attach_root_widget`], but for the resident
    /// presentation `id`: the root is wrapped in THAT presentation's own
    /// `MediaQuery` (its window's size and ratio, rewritten by its addressed
    /// resize), vsync and gesture arena. What a headless host gives a
    /// secondary window, so its subtree sees the same environment as the
    /// primary's.
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::__runtime::AttachError`] from
    /// [`flui_view::__runtime::WidgetsBinding::attach_root_widget`].
    ///
    /// # Panics
    ///
    /// Panics if `id` does not name a presentation this UI runtime currently
    /// hosts.
    #[cfg(any(test, feature = "testing"))]
    pub fn attach_root_widget_to<V>(
        &self,
        id: PresentationId,
        view: &V,
    ) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| {
            let presentation = ui_runtime.presentations.get(id).expect(
                "BUG: attach_root_widget_to given a presentation id this ui_runtime does \
                 not host",
            );
            let with_media_query = crate::media_query_root::MediaQueryRoot::new(
                std::rc::Rc::clone(&presentation.media_query),
                flui_view::ViewExt::boxed(view.clone()),
            );
            let focused = FocusTreeRoot::new(PlatformConvention::new(
                presentation.platform,
                with_media_query,
            ));
            let animated = VsyncScope::new(presentation.vsync(), focused);
            let wrapped = GestureArenaScope::new(presentation.gestures().arena().clone(), animated)
                .settings(presentation.gesture_settings.provider())
                .wheel_preferences(presentation.wheel_preferences.provider());
            presentation.widgets().attach_root_widget(&wrapped)?;
            ui_runtime.request_redraw_for(presentation);
            tracing::debug!(?id, "Root widget attached (addressed)");
            Ok(())
        })
    }
}

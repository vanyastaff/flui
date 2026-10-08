//! Attaching root widgets to a presentation.

use super::UiRuntime;
#[cfg(any(test, feature = "test-support"))]
use flui_foundation::PresentationId;
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

impl UiRuntime {
    // ========================================================================
    // Root attach (moved from the retired `AppBinding`)
    // ========================================================================

    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build. Forwards
    /// to [`flui_view::WidgetsBinding::attach_root_widget`] — the single
    /// root-bootstrap path; every runner entry point calls
    /// [`Self::attach_root_widget_with_size`] (this method's sized sibling)
    /// instead, not a separate hand-rolled wiring.
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
    /// Forwards every [`flui_view::AttachError`] the underlying
    /// [`flui_view::WidgetsBinding::attach_root_widget`] returns.
    // Test-only: the desktop and mobile runners use the sized variant.
    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_root_widget<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| ui_runtime.attach_root_widget_entered(view))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn attach_root_widget_entered<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Install this ui_runtime's three exact owner-driven capabilities.
        // GestureArenaScope is outermost so every descendant recognizer
        // shares one binding-driven arena, VsyncScope carries the animation
        // registry, and FocusRoot publishes this presentation's exact focus
        // tree. These wrappers have no render object, so the render root is
        // unchanged.
        // The root MediaQuery publishes the ui_runtime's live platform data
        // (size, device pixel ratio, brightness) to the whole user subtree;
        // the ui_runtime's resize/appearance arms write the shared source and the
        // wrapper republishes.
        let with_media_query = crate::media_query_root::MediaQueryRoot::new(
            std::rc::Rc::clone(self.media_query()),
            flui_view::view::ViewExt::boxed(view.clone()),
        );
        let focused = FocusRoot::new(with_media_query);
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_root_widget_to_for_test<V>(
        &self,
        id: PresentationId,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| {
            let presentation = ui_runtime.presentations.get(id).expect(
                "BUG: attach_root_widget_to_for_test given a presentation id this ui_runtime does \
                 not host",
            );
            let focused = FocusRoot::new(view.clone());
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

    /// [`Self::attach_root_widget_with_size`], but for the resident
    /// presentation `id`: the root is wrapped in THAT presentation's own
    /// `MediaQuery` (its window's size and ratio, rewritten by its addressed
    /// resize), vsync and gesture arena, and its root view is born at
    /// `width` × `height`. What a headless host gives a secondary window, so
    /// the secondary's subtree sees the same environment as the primary's.
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::AttachError`] from
    /// [`flui_view::WidgetsBinding::attach_root_widget_with_size`].
    ///
    /// # Panics
    ///
    /// Panics if `id` does not name a presentation this UI runtime currently
    /// hosts.
    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_root_widget_with_size_to<V>(
        &self,
        id: PresentationId,
        view: &V,
        width: f64,
        height: f64,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| {
            let presentation = ui_runtime.presentations.get(id).expect(
                "BUG: attach_root_widget_with_size_to given a presentation id this ui_runtime does \
                 not host",
            );
            let with_media_query = crate::media_query_root::MediaQueryRoot::new(
                std::rc::Rc::clone(&presentation.media_query),
                flui_view::view::ViewExt::boxed(view.clone()),
            );
            let focused = FocusRoot::new(with_media_query);
            let animated = VsyncScope::new(presentation.vsync(), focused);
            let wrapped = GestureArenaScope::new(presentation.gestures().arena().clone(), animated)
                .settings(presentation.gesture_settings.provider())
                .wheel_preferences(presentation.wheel_preferences.provider());
            presentation
                .widgets()
                .attach_root_widget_with_size(&wrapped, width, height)?;
            ui_runtime.request_redraw_for(presentation);
            tracing::debug!(
                ?id,
                width,
                height,
                "Root widget attached (sized, addressed)"
            );
            Ok(())
        })
    }

    /// Attach a root widget sizing the root view to an explicit logical
    /// `width` × `height` — the platform window's surface size.
    ///
    /// Identical to `attach_root_widget` (a `test-support` entry point)
    /// except the root `RenderView` is born at the real window size instead
    /// of the framework's fallback default. This is the runner's bootstrap
    /// entry point, and wraps the root in the same `GestureArenaScope`,
    /// `VsyncScope`, `FocusRoot` and `MediaQuery` as that method (see its
    /// doc for the auto-wrap invariants).
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::AttachError`] from
    /// [`flui_view::WidgetsBinding::attach_root_widget_with_size`].
    pub fn attach_root_widget_with_size<V>(
        &self,
        view: &V,
        width: f64,
        height: f64,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|ui_runtime| {
            ui_runtime.attach_root_widget_with_size_entered(view, width, height)
        })
    }

    fn attach_root_widget_with_size_entered<V>(
        &self,
        view: &V,
        width: f64,
        height: f64,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Same root MediaQuery as the production attach path — the sized
        // variant must not present a different ambient environment.
        let with_media_query = crate::media_query_root::MediaQueryRoot::new(
            std::rc::Rc::clone(self.media_query()),
            flui_view::view::ViewExt::boxed(view.clone()),
        );
        let focused = FocusRoot::new(with_media_query);
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated)
            .settings(self.presentations.primary().gesture_settings.provider())
            .wheel_preferences(self.presentations.primary().wheel_preferences.provider());
        self.presentations
            .primary()
            .widgets()
            .attach_root_widget_with_size(&wrapped, width, height)?;
        self.request_redraw();
        tracing::debug!(width, height, "Root widget attached (sized)");
        Ok(())
    }
}

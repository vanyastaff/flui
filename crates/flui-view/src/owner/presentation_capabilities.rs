//! The presentation capabilities a UI runtime installs on its [`BuildOwner`]
//! at construction, which `LifecycleContext` hands to widgets. A bare owner
//! has none of them, so a headless tree observes `None` honestly.

use super::BuildOwner;

/// What a presentation hands its widgets beyond the build itself: the
/// handles `LifecycleContext` exposes from `init_state` and
/// `did_change_dependencies` (ADR-0078).
///
/// One structure, so a new capability is a field here, a `LifecycleContext`
/// method, and the installer in the UI runtime. A bare owner holds the
/// `Default`: every capability absent, which widgets observe as `None`
/// rather than as a silent no-op.
///
/// The UI runtime installs it through [`BuildOwner::capabilities_mut`]
/// while it assembles a presentation; every handle is a cheap clone of a
/// shared one.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct PresentationCapabilities {
    /// The presentation's IME/text-input attach-detach capability, minted by
    /// its `TextInputOwner`. Absent on a headless tree, which has nothing
    /// to deliver IME events to.
    pub text_input: Option<flui_interaction::TextInputHandle>,
    /// The presentation's plain-text clipboard, over its platform's clipboard.
    pub clipboard: Option<flui_interaction::ClipboardHandle>,
    /// The presentation's announcer, over its semantics pipeline's
    /// announcement queue. Absent on a headless tree.
    pub announcer: Option<flui_rendering::semantics::Announcer>,
    /// Whether the presentation's accessibility adapter says where the
    /// assistive technology's cursor is. `Unreported` on a headless tree.
    pub assistive_focus: flui_rendering::semantics::AssistiveFocusSupport,
    /// The presentation's haptics host. Widgets acquire it through
    /// `LifecycleContext::haptics`, which applies the enclosing `HapticsMode`.
    pub haptics: Option<flui_interaction::HapticsHandle>,
    /// The presentation's drag-and-drop over its runtime's drag sessions.
    pub drag_and_drop: Option<flui_interaction::drag::DragAndDrop>,
    /// The presentation's external-texture registry.
    pub textures: Option<flui_rendering::layer::TextureRegistry>,
    /// The UI runtime's byte storage; absent where the platform has no files
    /// or the application configured none.
    pub storage: Option<std::sync::Arc<dyn flui_platform::Storage>>,
    /// The factory that renders a recovered failure in this presentation's
    /// tree, or `None` for the default [`ErrorView`](crate::ErrorView). It
    /// applies from the next recovery on.
    pub error_view_builder: Option<crate::view::ErrorViewBuilder>,
}

impl BuildOwner {
    /// Join the UI runtime's [`BuildInProgress`](crate::BuildInProgress) cell, so
    /// a write that reaches this owner's graph while another presentation of
    /// the same runtime builds is refused (ADR-0074). Called once, at
    /// presentation assembly, before any element builds.
    pub fn set_build_in_progress(&mut self, shared: crate::BuildInProgress) {
        self.reactive().share_build_in_progress(shared);
    }

    /// The presentation capabilities installed on this owner.
    #[must_use]
    pub fn capabilities(&self) -> &PresentationCapabilities {
        &self.capabilities
    }

    /// The presentation capabilities, for the UI runtime that assembles this
    /// owner's presentation to install them.
    pub fn capabilities_mut(&mut self) -> &mut PresentationCapabilities {
        &mut self.capabilities
    }
}

impl std::fmt::Debug for PresentationCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationCapabilities")
            .field("text_input", &self.text_input.is_some())
            .field("clipboard", &self.clipboard.is_some())
            .field("announcer", &self.announcer.is_some())
            .field("assistive_focus", &self.assistive_focus)
            .field("haptics", &self.haptics.is_some())
            .field("drag_and_drop", &self.drag_and_drop.is_some())
            .field("textures", &self.textures.is_some())
            .field("storage", &self.storage.is_some())
            .field("error_view_builder", &self.error_view_builder.is_some())
            .finish()
    }
}

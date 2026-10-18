//! The presentation capabilities a UI runtime installs on its [`BuildOwner`]
//! at construction, which `LifecycleContext` hands to widgets. A bare owner
//! has none of them, so a headless tree observes `None` honestly.

use super::BuildOwner;

impl BuildOwner {
    /// Install the binding's IME/text-input attach-detach capability.
    ///
    /// Called during `UiRuntime` construction with the weak handle minted by that
    /// presentation's `TextInputOwner`. `HeadlessBinding` installs none, so
    /// headless-tree tests observe `LifecycleContext::text_input_handle() == None`
    /// honestly rather than accepting attaches nobody delivers events to.
    pub fn set_text_input_handle(&mut self, handle: flui_interaction::TextInputHandle) {
        self.text_input_handle = Some(handle);
    }

    /// Install the presentation's plain-text clipboard.
    ///
    /// Called during presentation construction with a handle over the
    /// UI runtime's platform clipboard, so `LifecycleContext::clipboard_handle`
    /// answers `Some` under every UI runtime.
    pub fn set_clipboard_handle(&mut self, handle: flui_interaction::ClipboardHandle) {
        self.clipboard_handle = Some(handle);
    }

    /// Install the presentation's haptics handle.
    ///
    /// Called during presentation construction with a handle over the
    /// presentation's haptics host, so `LifecycleContext::haptics` answers
    /// `Some` under every UI runtime.
    pub fn set_haptics_handle(&mut self, handle: flui_interaction::HapticsHandle) {
        self.haptics_handle = Some(handle);
    }

    /// Install the presentation's drag-and-drop.
    ///
    /// Called during presentation construction, so
    /// `LifecycleContext::drag_and_drop` answers `Some` under every UI runtime.
    pub fn set_drag_and_drop(&mut self, drag_and_drop: flui_interaction::drag::DragAndDrop) {
        self.drag_and_drop = Some(drag_and_drop);
    }

    /// Install the presentation's external-texture registry, which
    /// `LifecycleContext::texture_registry` hands to widgets.
    pub fn set_texture_registry(&mut self, registry: flui_rendering::layer::TextureRegistry) {
        self.texture_registry = Some(registry);
    }

    /// The presentation's external-texture registry, if one was installed.
    #[must_use]
    pub fn texture_registry(&self) -> Option<&flui_rendering::layer::TextureRegistry> {
        self.texture_registry.as_ref()
    }

    /// Install the UI runtime's byte storage.
    ///
    /// Called during presentation construction when the UI runtime has storage,
    /// so `LifecycleContext::storage` answers `Some` under it.
    pub fn set_storage(&mut self, storage: std::sync::Arc<dyn flui_platform_api::Storage>) {
        self.storage = Some(storage);
    }
}

//! PlatformWindow trait contract — per-window API surface with callbacks.
//!
//! Design contract for the implementation phase.

use std::sync::Arc;

pub trait PlatformWindow: Send + Sync {
    // === Size & Bounds (existing + enhanced) ===
    fn physical_size(&self) -> Size<DevicePixels>;
    fn logical_size(&self) -> Size<Pixels>;
    fn bounds(&self) -> Bounds<Pixels>; // NEW
    fn content_size(&self) -> Size<Pixels>; // NEW
    fn window_bounds(&self) -> WindowBounds; // NEW
    fn scale_factor(&self) -> f32; // Changed: f64 → f32 (GPUI compat)

    // === State Query (existing + enhanced) ===
    fn is_focused(&self) -> bool;
    fn is_visible(&self) -> bool;
    fn is_maximized(&self) -> bool; // NEW
    fn is_fullscreen(&self) -> bool; // NEW
    fn is_active(&self) -> bool; // NEW
    fn is_hovered(&self) -> bool; // NEW

    // === Input State (NEW) ===
    fn mouse_position(&self) -> Point<Pixels>;
    fn modifiers(&self) -> Modifiers;

    // === Appearance (NEW) ===
    fn appearance(&self) -> WindowAppearance;
    fn display(&self) -> Option<Arc<dyn PlatformDisplay>>;

    // === Title (NEW) ===
    fn get_title(&self) -> String;
    fn set_title(&self, title: &str);

    // === Window Control (NEW) ===
    fn activate(&self);
    fn minimize(&self);
    fn maximize(&self);
    fn restore(&self);
    fn toggle_fullscreen(&self);
    fn resize(&self, size: Size<Pixels>);
    fn close(&self);
    fn request_redraw(&self);
    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance);

    // === Callback Registration (NEW — core architecture) ===
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>);
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>);
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>);
    fn on_moved(&self, callback: Box<dyn FnMut() + Send>);
    fn on_close(&self, callback: Box<dyn FnOnce() + Send>);
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>);
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>);
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>);
    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>);

    // === GPU Integration (existing) ===
    // HasWindowHandle + HasDisplayHandle from raw-window-handle already implemented
    fn as_any(&self) -> &dyn std::any::Any {
        panic!("not implemented")
    }

    // === Test Support ===
    #[cfg(any(test, feature = "test-support"))]
    fn as_test(&mut self) -> Option<&mut TestWindow> {
        None
    }
}

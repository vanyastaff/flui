//! macOS window (NSWindow) implementation

use super::view;
use crate::config::WindowConfiguration;
use crate::shared::PlatformHandlers;
use crate::traits::*;
use anyhow::{Context, Result};
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use cocoa::appkit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::NSRect;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

/// macOS window wrapper around NSWindow
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: id,

    /// Window state
    state: Arc<Mutex<WindowState>>,

    /// Reference to all windows
    windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Platform handlers
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Window configuration
    _config: WindowConfiguration,
}

unsafe impl Send for MacOSWindow {}
unsafe impl Sync for MacOSWindow {}

/// Mutable window state
struct WindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<Pixels>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: f64,
}

impl MacOSWindow {
    /// Create a new macOS window
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
        config: WindowConfiguration,
    ) -> Result<Arc<Self>> {
        unsafe {
            // Convert logical size to NSRect
            let frame = NSRect::new(
                cocoa::foundation::NSPoint::new(0.0, 0.0),
                cocoa::foundation::NSSize::new(
                    options.size.width.0 as f64,
                    options.size.height.0 as f64,
                ),
            );

            // Build window style mask
            let mut style_mask = NSWindowStyleMask::NSClosableWindowMask
                | NSWindowStyleMask::NSMiniaturizableWindowMask;

            if options.decorated {
                style_mask |= NSWindowStyleMask::NSTitledWindowMask;
            }

            if options.resizable {
                style_mask |= NSWindowStyleMask::NSResizableWindowMask;
            }

            // Create NSWindow
            let ns_window: id = msg_send![class!(NSWindow), alloc];
            let ns_window: id = msg_send![ns_window,
                initWithContentRect: frame
                styleMask: style_mask
                backing: NSBackingStoreType::NSBackingStoreBuffered
                defer: false as i8
            ];

            if ns_window == nil {
                return Err(anyhow::anyhow!("Failed to create NSWindow"));
            }

            // Set window title
            let title = cocoa::foundation::NSString::alloc(nil);
            let title = cocoa::foundation::NSString::init_str(title, &options.title);
            let _: () = msg_send![ns_window, setTitle: title];

            // Get backing scale factor
            let scale: f64 = msg_send![ns_window, backingScaleFactor];

            // Make window visible if requested
            if options.visible {
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: nil];
            }

            // Center window on screen
            let _: () = msg_send![ns_window, center];

            let window = Arc::new(Self {
                ns_window,
                state: Arc::new(Mutex::new(WindowState {
                    bounds: Bounds {
                        origin: Point::new(
                            flui_types::geometry::px(frame.origin.x as f32),
                            flui_types::geometry::px(frame.origin.y as f32),
                        ),
                        size: options.size,
                    },
                    scale_factor: scale,
                })),
                windows_map: Arc::clone(&windows_map),
                handlers: Arc::clone(&handlers),
                _config: config,
            });

            // Create content view for input events
            let content_view = view::create_content_view(frame, scale, Arc::downgrade(&handlers));
            let _: () = msg_send![ns_window, setContentView: content_view];

            // Enable mouse tracking for mouse moved events
            view::enable_mouse_tracking(content_view);

            // Make content view first responder to receive keyboard events
            let _: () = msg_send![ns_window, makeFirstResponder: content_view];

            // Set window delegate for lifecycle events
            let delegate = create_window_delegate(Arc::downgrade(&window));
            let _: () = msg_send![ns_window, setDelegate: delegate];

            // Store in windows map
            let window_id = ns_window as u64;
            windows_map.lock().insert(window_id, Arc::clone(&window));

            tracing::info!(
                "Created NSWindow {:p} with size {}x{} (scale: {})",
                ns_window,
                options.size.width.0,
                options.size.height.0,
                scale
            );

            Ok(window)
        }
    }

    /// Get the NSWindow handle
    pub fn ns_window(&self) -> id {
        self.ns_window
    }
}

impl PlatformWindow for MacOSWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor;
        Size::new(
            flui_types::geometry::device_px((logical.width.0 * scale as f32).round()),
            flui_types::geometry::device_px((logical.height.0 * scale as f32).round()),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn scale_factor(&self) -> f64 {
        let state = self.state.lock();
        state.scale_factor
    }

    fn request_redraw(&self) {
        unsafe {
            // Tell the window's content view to redraw
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                let _: () = msg_send![content_view, setNeedsDisplay: true];
            }
        }
    }

    fn is_focused(&self) -> bool {
        unsafe {
            let is_key: bool = msg_send![self.ns_window, isKeyWindow];
            is_key
        }
    }

    fn is_visible(&self) -> bool {
        unsafe {
            let is_visible: bool = msg_send![self.ns_window, isVisible];
            is_visible
        }
    }

    fn set_title(&self, title: &str) {
        unsafe {
            let ns_title = cocoa::foundation::NSString::alloc(nil);
            let ns_title = cocoa::foundation::NSString::init_str(ns_title, title);
            let _: () = msg_send![self.ns_window, setTitle: ns_title];
        }
    }

    fn set_size(&self, size: Size<Pixels>) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                frame.origin,
                cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64),
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: true];

            // Update state
            let mut state = self.state.lock();
            state.bounds.size = size;
        }
    }

    fn close(&self) {
        unsafe {
            let _: () = msg_send![self.ns_window, close];
        }
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for MacOSWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::ptr::NonNull;

        let ns_window = self.ns_window as *mut std::ffi::c_void;
        let handle = AppKitWindowHandle::new(NonNull::new(ns_window).unwrap());

        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for MacOSWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}

impl Clone for MacOSWindow {
    fn clone(&self) -> Self {
        Self {
            ns_window: self.ns_window,
            state: Arc::clone(&self.state),
            windows_map: Arc::clone(&self.windows_map),
            handlers: Arc::clone(&self.handlers),
            _config: self._config.clone(),
        }
    }
}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        // Only cleanup if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Closing NSWindow {:p}", self.ns_window);

            // Remove from windows map
            let window_id = self.ns_window as u64;
            self.windows_map.lock().remove(&window_id);

            unsafe {
                // Release NSWindow (will be deallocated by Cocoa)
                let _: () = msg_send![self.ns_window, release];
            }
        }
    }
}

// ============================================================================
// Cross-Platform Window Trait Implementation
// ============================================================================

use crate::window::{
    RawWindowHandle as CrossRawWindowHandle, Window as WindowTrait, WindowId as CrossWindowId,
    WindowState,
};

impl WindowTrait for MacOSWindow {
    fn id(&self) -> CrossWindowId {
        CrossWindowId::new(self.ns_window as u64)
    }

    fn title(&self) -> String {
        unsafe {
            let ns_title: id = msg_send![self.ns_window, title];
            let c_str: *const i8 = msg_send![ns_title, UTF8String];
            if c_str.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(c_str)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }

    fn set_title(&mut self, title: &str) {
        unsafe {
            let ns_title = cocoa::foundation::NSString::alloc(nil);
            let ns_title = cocoa::foundation::NSString::init_str(ns_title, title);
            let _: () = msg_send![self.ns_window, setTitle: ns_title];
        }
    }

    fn position(&self) -> Point<Pixels> {
        let state = self.state.lock();
        state.bounds.origin
    }

    fn set_position(&mut self, position: Point<Pixels>) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                cocoa::foundation::NSPoint::new(position.x.0 as f64, position.y.0 as f64),
                frame.size,
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: true];

            // Update state
            let mut state = self.state.lock();
            state.bounds.origin = position;
        }
    }

    fn size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn set_size(&mut self, size: Size<Pixels>) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                frame.origin,
                cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64),
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: true];

            // Update state
            let mut state = self.state.lock();
            state.bounds.size = size;
        }
    }

    fn state(&self) -> WindowState {
        unsafe {
            let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];

            if is_minimized {
                WindowState::Minimized
            } else if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                WindowState::Fullscreen
            } else if is_zoomed {
                WindowState::Maximized
            } else {
                WindowState::Normal
            }
        }
    }

    fn set_state(&mut self, state: WindowState) {
        unsafe {
            match state {
                WindowState::Normal => {
                    // Restore from minimized
                    let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![self.ns_window, deminiaturize: nil];
                    }

                    // Restore from maximized
                    let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
                    if is_zoomed {
                        let _: () = msg_send![self.ns_window, zoom: nil];
                    }

                    // Exit fullscreen
                    let style_mask: cocoa::appkit::NSWindowStyleMask =
                        msg_send![self.ns_window, styleMask];
                    if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                        let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
                    }
                }
                WindowState::Minimized => {
                    let _: () = msg_send![self.ns_window, miniaturize: nil];
                }
                WindowState::Maximized => {
                    // First restore from minimized if needed
                    let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![self.ns_window, deminiaturize: nil];
                    }

                    // Then zoom (maximize)
                    let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
                    if !is_zoomed {
                        let _: () = msg_send![self.ns_window, zoom: nil];
                    }
                }
                WindowState::Fullscreen => {
                    let style_mask: cocoa::appkit::NSWindowStyleMask =
                        msg_send![self.ns_window, styleMask];
                    if !style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                        let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
                    }
                }
            }
        }
    }

    fn is_visible(&self) -> bool {
        unsafe {
            let is_visible: bool = msg_send![self.ns_window, isVisible];
            is_visible
        }
    }

    fn set_visible(&mut self, visible: bool) {
        unsafe {
            if visible {
                let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: nil];
            } else {
                let _: () = msg_send![self.ns_window, orderOut: nil];
            }
        }
    }

    fn is_resizable(&self) -> bool {
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSResizableWindowMask)
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if resizable {
                style_mask |= NSWindowStyleMask::NSResizableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSResizableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn is_minimizable(&self) -> bool {
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSMiniaturizableWindowMask)
        }
    }

    fn set_minimizable(&mut self, minimizable: bool) {
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if minimizable {
                style_mask |= NSWindowStyleMask::NSMiniaturizableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSMiniaturizableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn is_closable(&self) -> bool {
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSClosableWindowMask)
        }
    }

    fn set_closable(&mut self, closable: bool) {
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if closable {
                style_mask |= NSWindowStyleMask::NSClosableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSClosableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn focus(&mut self) {
        unsafe {
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: nil];
        }
    }

    fn is_focused(&self) -> bool {
        unsafe {
            let is_key: bool = msg_send![self.ns_window, isKeyWindow];
            is_key
        }
    }

    fn close(&mut self) {
        unsafe {
            let _: () = msg_send![self.ns_window, close];
        }
    }

    fn request_redraw(&mut self) {
        unsafe {
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                let _: () = msg_send![content_view, setNeedsDisplay: true];
            }
        }
    }

    fn set_min_size(&mut self, size: Option<Size<Pixels>>) {
        unsafe {
            if let Some(size) = size {
                let ns_size =
                    cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![self.ns_window, setMinSize: ns_size];
            } else {
                // Set to zero to remove constraint
                let ns_size = cocoa::foundation::NSSize::new(0.0, 0.0);
                let _: () = msg_send![self.ns_window, setMinSize: ns_size];
            }
        }
    }

    fn set_max_size(&mut self, size: Option<Size<Pixels>>) {
        unsafe {
            if let Some(size) = size {
                let ns_size =
                    cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![self.ns_window, setMaxSize: ns_size];
            } else {
                // Set to max to remove constraint
                let ns_size = cocoa::foundation::NSSize::new(f64::MAX, f64::MAX);
                let _: () = msg_send![self.ns_window, setMaxSize: ns_size];
            }
        }
    }

    fn scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn raw_window_handle(&self) -> CrossRawWindowHandle {
        unsafe {
            let content_view: id = msg_send![self.ns_window, contentView];
            CrossRawWindowHandle::MacOS {
                ns_view: content_view as *mut std::ffi::c_void,
                ns_window: self.ns_window as *mut std::ffi::c_void,
            }
        }
    }

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        HasWindowHandle::window_handle(self)
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        HasDisplayHandle::display_handle(self)
    }
}

// ============================================================================
// macOS Window Extension Trait Implementation
// ============================================================================

use super::liquid_glass::{LiquidGlassConfig, LiquidGlassMaterial};
use super::window_ext::{
    MacOSCollectionBehavior, MacOSWindowExt as MacOSWindowExtTrait, MacOSWindowLevel,
};
use super::window_tiling::TilingConfiguration;

impl MacOSWindowExtTrait for MacOSWindow {
    fn set_liquid_glass(&mut self, material: LiquidGlassMaterial) {
        // Create default config from material
        let config = LiquidGlassConfig::from_material(material);
        self.set_liquid_glass_config(config);
    }

    fn set_liquid_glass_config(&mut self, config: LiquidGlassConfig) {
        unsafe {
            // Apply vibrancy effect to window content view
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view == nil {
                tracing::warn!("Cannot apply Liquid Glass: content view is nil");
                return;
            }

            // Create NSVisualEffectView
            let effect_view_class = class!(NSVisualEffectView);
            let effect_view: id = msg_send![effect_view_class, alloc];
            let effect_view: id = msg_send![effect_view, init];

            // Set frame to match content view
            let frame: NSRect = msg_send![content_view, frame];
            let _: () = msg_send![effect_view, setFrame: frame];

            // Set material (NSVisualEffectMaterial)
            let material_value: usize = config.material.to_ns_visual_effect_material();
            let _: () = msg_send![effect_view, setMaterial: material_value];

            // Set blending mode (NSVisualEffectBlendingMode)
            let blending_mode: usize = 0; // NSVisualEffectBlendingModeBehindWindow
            let _: () = msg_send![effect_view, setBlendingMode: blending_mode];

            // Set state (NSVisualEffectState)
            let state: usize = 1; // NSVisualEffectStateActive
            let _: () = msg_send![effect_view, setState: state];

            // Enable autoresizing
            let autoresizing_mask: usize = (1 << 1) | (1 << 4); // NSViewWidthSizable | NSViewHeightSizable
            let _: () = msg_send![effect_view, setAutoresizingMask: autoresizing_mask];

            // Set as window content view
            let _: () = msg_send![self.ns_window, setContentView: effect_view];

            // Make window titlebar transparent if requested
            if config.transparent_titlebar {
                let style_mask: cocoa::appkit::NSWindowStyleMask =
                    msg_send![self.ns_window, styleMask];
                let new_style_mask =
                    style_mask | NSWindowStyleMask::NSFullSizeContentViewWindowMask;
                let _: () = msg_send![self.ns_window, setStyleMask: new_style_mask];
                let _: () = msg_send![self.ns_window, setTitlebarAppearsTransparent: YES];
            }

            tracing::debug!("Applied Liquid Glass material: {:?}", config.material);
        }
    }

    fn clear_liquid_glass(&mut self) {
        unsafe {
            // Remove visual effect view and restore normal content view
            let content_view = view::create_content_view(
                NSRect::new(
                    cocoa::foundation::NSPoint::new(0.0, 0.0),
                    cocoa::foundation::NSSize::new(800.0, 600.0),
                ),
                self.scale_factor() as f64,
                Arc::downgrade(&self.handlers),
            );

            let _: () = msg_send![self.ns_window, setContentView: content_view];

            // Restore titlebar appearance
            let _: () = msg_send![self.ns_window, setTitlebarAppearsTransparent: NO];

            tracing::debug!("Cleared Liquid Glass effect");
        }
    }

    fn enable_tiling(&mut self, config: TilingConfiguration) {
        // Store tiling config for later use
        // Note: Actual tiling API is only available in macOS 15+
        // For now, we just log the configuration
        tracing::info!(
            "Window tiling enabled: position={:?}, ratio={}, layout={:?}",
            config.primary_position,
            config.split_ratio,
            config.layout
        );

        // TODO: When macOS 15 APIs are available, implement actual tiling
        // This would use NSWindow's tiling-related properties
    }

    fn disable_tiling(&mut self) {
        tracing::info!("Window tiling disabled");
        // TODO: Implement when macOS 15 APIs are available
    }

    fn is_tiling_enabled(&self) -> bool {
        // TODO: Implement when macOS 15 APIs are available
        false
    }

    fn enable_tabbing(&mut self) {
        unsafe {
            // Enable automatic tabbing (macOS 10.12+)
            let tabbing_mode: isize = 1; // NSWindowTabbingModeAutomatic
            let _: () = msg_send![self.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing enabled");
        }
    }

    fn disable_tabbing(&mut self) {
        unsafe {
            let tabbing_mode: isize = 2; // NSWindowTabbingModeDisallowed
            let _: () = msg_send![self.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing disabled");
        }
    }

    fn add_tab_to_window(&mut self, other_window_id: u64) {
        unsafe {
            // Get the other window from the windows map
            let other_window_ptr = other_window_id as *mut std::ffi::c_void;
            let other_ns_window = other_window_ptr as id;

            if other_ns_window != nil {
                let _: () = msg_send![self.ns_window, addTabbedWindow:other_ns_window ordered:0]; // NSWindowAbove
                tracing::debug!("Added tab to window {:p}", other_ns_window);
            } else {
                tracing::warn!("Cannot add tab: window {:?} not found", other_window_id);
            }
        }
    }

    fn toggle_native_fullscreen(&mut self) {
        unsafe {
            let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
            tracing::debug!("Toggled native fullscreen");
        }
    }

    fn set_window_level(&mut self, level: MacOSWindowLevel) {
        unsafe {
            let level_value = level.to_ns_value();
            let _: () = msg_send![self.ns_window, setLevel: level_value];
            tracing::debug!("Set window level to {:?} ({})", level, level_value);
        }
    }

    fn window_level(&self) -> MacOSWindowLevel {
        unsafe {
            let level_value: isize = msg_send![self.ns_window, level];
            match level_value {
                0 => MacOSWindowLevel::Normal,
                3 => MacOSWindowLevel::Floating,
                8 => MacOSWindowLevel::ModalPanel,
                24 => MacOSWindowLevel::MainMenu,
                25 => MacOSWindowLevel::Status,
                101 => MacOSWindowLevel::PopUpMenu,
                1000 => MacOSWindowLevel::ScreenSaver,
                _ if level_value == isize::MAX - 1 => MacOSWindowLevel::FloatingPanel,
                _ => MacOSWindowLevel::Normal, // Default to normal for unknown values
            }
        }
    }

    fn set_collection_behavior(&mut self, behavior: MacOSCollectionBehavior) {
        unsafe {
            let _: () = msg_send![self.ns_window, setCollectionBehavior: behavior.bits() as usize];
            tracing::debug!("Set collection behavior: {:?}", behavior);
        }
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        unsafe {
            let _: () = msg_send![self.ns_window, setHasShadow: has_shadow as i8];
            tracing::debug!("Set window shadow: {}", has_shadow);
        }
    }

    fn set_alpha(&mut self, alpha: f32) {
        unsafe {
            let clamped_alpha = alpha.clamp(0.0, 1.0);
            let _: () = msg_send![self.ns_window, setAlphaValue: clamped_alpha as f64];
            tracing::debug!("Set window alpha: {}", clamped_alpha);
        }
    }

    fn backing_scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn convert_point_from_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 / scale), Pixels(point.y.0 / scale))
    }

    fn convert_point_to_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 * scale), Pixels(point.y.0 * scale))
    }
}

// ============================================================================
// NSWindowDelegate Implementation
// ============================================================================

use std::sync::Weak;

/// Create a window delegate for lifecycle events
fn create_window_delegate(window: Weak<MacOSWindow>) -> id {
    unsafe {
        // Get or create delegate class
        let class = get_or_create_delegate_class();
        let delegate: id = msg_send![class, alloc];
        let delegate: id = msg_send![delegate, init];

        // Store weak pointer to window
        let window_ptr = Box::into_raw(Box::new(window)) as *mut std::ffi::c_void;
        (*delegate).set_ivar("window_ptr", window_ptr);

        delegate
    }
}

/// Get or create the NSWindowDelegate class
fn get_or_create_delegate_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("FLUIWindowDelegate", superclass).unwrap();

        // Add ivar to store window pointer
        decl.add_ivar::<*mut std::ffi::c_void>("window_ptr");

        // windowDidResize:
        unsafe extern "C" fn window_did_resize(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_resize();
            }
        }

        // windowDidMove:
        unsafe extern "C" fn window_did_move(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_move();
            }
        }

        // windowDidBecomeKey:
        unsafe extern "C" fn window_did_become_key(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_focus_gained();
            }
        }

        // windowDidResignKey:
        unsafe extern "C" fn window_did_resign_key(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_focus_lost();
            }
        }

        // windowShouldClose:
        unsafe extern "C" fn window_should_close(this: &Object, _sel: Sel, _sender: id) -> BOOL {
            if let Some(window) = get_window_from_delegate(this) {
                if window.handle_close_request() {
                    YES
                } else {
                    NO
                }
            } else {
                YES
            }
        }

        // windowWillClose:
        unsafe extern "C" fn window_will_close(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_close();
            }
        }

        // windowDidChangeBackingProperties: (Retina/DPI change)
        unsafe extern "C" fn window_did_change_backing_properties(
            this: &Object,
            _sel: Sel,
            _notification: id,
        ) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_backing_properties_changed();
            }
        }

        // windowDidChangeScreen: (moved to different monitor)
        unsafe extern "C" fn window_did_change_screen(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_screen_changed();
            }
        }

        // Add methods
        unsafe {
            decl.add_method(
                sel!(windowDidResize:),
                window_did_resize as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidBecomeKey:),
                window_did_become_key as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidResignKey:),
                window_did_resign_key as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowShouldClose:),
                window_should_close as extern "C" fn(&Object, Sel, id) -> BOOL,
            );
            decl.add_method(
                sel!(windowWillClose:),
                window_will_close as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidChangeBackingProperties:),
                window_did_change_backing_properties as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidChangeScreen:),
                window_did_change_screen as extern "C" fn(&Object, Sel, id),
            );
        }

        decl.register();
    });

    Class::get("FLUIWindowDelegate").unwrap()
}

/// Get window from delegate
unsafe fn get_window_from_delegate(delegate: &Object) -> Option<Arc<MacOSWindow>> {
    let window_ptr: *mut std::ffi::c_void = *delegate.get_ivar("window_ptr");
    let weak_ptr = window_ptr as *mut Weak<MacOSWindow>;
    if weak_ptr.is_null() {
        return None;
    }
    (*weak_ptr).upgrade()
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// Handle window resize event
    fn handle_resize(&self) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let content_rect: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];

            let new_size = Size::new(
                flui_types::geometry::px(content_rect.size.width as f32),
                flui_types::geometry::px(content_rect.size.height as f32),
            );

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.size = new_size;
            }

            // Notify handlers
            let handlers = self.handlers.lock();
            if let Some(handler) = &handlers.on_resize {
                handler(new_size);
            }

            tracing::debug!(
                "Window resized to {}x{}",
                new_size.width.0,
                new_size.height.0
            );
        }
    }

    /// Handle window move event
    fn handle_move(&self) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];

            let new_origin = Point::new(
                flui_types::geometry::px(frame.origin.x as f32),
                flui_types::geometry::px(frame.origin.y as f32),
            );

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.origin = new_origin;
            }

            tracing::debug!("Window moved to ({}, {})", new_origin.x.0, new_origin.y.0);
        }
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_active {
            handler();
        }
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_inactive {
            handler();
        }
        tracing::debug!("Window lost focus");
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent
    fn handle_close_request(&self) -> bool {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.should_close {
            let should_close = handler();
            tracing::debug!("Window close requested: {}", should_close);
            should_close
        } else {
            true // Allow close by default
        }
    }

    /// Handle window close event
    fn handle_close(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_close {
            handler();
        }
        tracing::debug!("Window closed");
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        unsafe {
            let new_scale: f64 = msg_send![self.ns_window, backingScaleFactor];

            // Update window state
            {
                let mut state = self.state.lock();
                if (state.scale_factor - new_scale).abs() > 0.01 {
                    state.scale_factor = new_scale;
                    tracing::info!("Window scale factor changed to {}", new_scale);
                }
            }

            // Update content view scale factor
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                view::update_view_scale_factor(content_view, new_scale);
            }
        }
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        unsafe {
            let screen: id = msg_send![self.ns_window, screen];
            if screen != nil {
                let scale: f64 = msg_send![screen, backingScaleFactor];
                tracing::debug!("Window moved to screen with scale factor {}", scale);

                // Update scale factor (will trigger backing properties changed)
                self.handle_backing_properties_changed();
            }
        }
    }
}

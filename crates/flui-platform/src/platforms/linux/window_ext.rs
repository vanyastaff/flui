//! Linux-specific window extensions
//!
//! This module provides Linux-specific features that extend the core `Window` trait.
//! These features use Wayland and X11 protocols depending on the active display server.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::Window;
//! use flui_platform::linux::LinuxWindowExt;
//!
//! // Use cross-platform API
//! window.set_title("My App");
//!
//! // Use Linux-specific extensions
//! #[cfg(feature = "wayland")]
//! {
//!     window.set_wayland_app_id("com.example.myapp");
//!     window.request_layer_surface(LayerSurfaceLayer::Overlay);
//! }
//! ```

// ============================================================================
// Linux Window Extension Trait
// ============================================================================

/// Linux-specific window extensions.
///
/// This trait provides access to Linux-specific features that are not part
/// of the cross-platform `Window` trait. The implementation varies based on
/// whether Wayland or X11 is used as the display server.
///
/// # Platform Availability
///
/// - **Wayland Protocols:** Modern Linux with Wayland compositor
/// - **Layer Shell:** wlr-layer-shell protocol (wlroots compositors)
/// - **X11 EWMH:** Extended Window Manager Hints (X11 compositors)
/// - **Client-Side Decorations:** Wayland compositors
/// - **Server-Side Decorations:** X11 and some Wayland compositors
#[cfg(target_os = "linux")]
pub trait LinuxWindowExt {
    /// Get the active display server protocol.
    fn display_server(&self) -> DisplayServer;

    // ========================================================================
    // Wayland-Specific Methods
    // ========================================================================

    /// Set the Wayland app_id.
    ///
    /// The app_id is used by compositors for window grouping, icon matching,
    /// and desktop file association.
    ///
    /// # Platform Requirements
    ///
    /// - Wayland only
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.set_wayland_app_id("com.example.myapp");
    /// ```
    #[cfg(feature = "wayland")]
    fn set_wayland_app_id(&mut self, app_id: &str);

    /// Get the Wayland app_id.
    #[cfg(feature = "wayland")]
    fn wayland_app_id(&self) -> Option<String>;

    /// Request a layer surface.
    ///
    /// Layer surfaces are Wayland surfaces that exist in specific layers
    /// (background, bottom, top, overlay) managed by the compositor.
    ///
    /// This is useful for:
    /// - Desktop widgets
    /// - Status bars / panels
    /// - Wallpapers
    /// - On-screen displays
    ///
    /// # Platform Requirements
    ///
    /// - Wayland with wlr-layer-shell protocol (wlroots-based compositors)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.request_layer_surface(LayerSurfaceConfig {
    ///     layer: LayerSurfaceLayer::Overlay,
    ///     anchor: Anchor::TOP | Anchor::LEFT,
    ///     exclusive_zone: 32,
    ///     keyboard_interactivity: KeyboardInteractivity::OnDemand,
    /// });
    /// ```
    #[cfg(feature = "wayland")]
    fn request_layer_surface(&mut self, config: LayerSurfaceConfig);

    /// Remove layer surface and return to normal toplevel window.
    #[cfg(feature = "wayland")]
    fn remove_layer_surface(&mut self);

    /// Check if window is a layer surface.
    #[cfg(feature = "wayland")]
    fn is_layer_surface(&self) -> bool;

    /// Set client-side decorations mode.
    ///
    /// Controls whether the application draws its own window decorations
    /// or uses compositor/server-side decorations.
    ///
    /// # Platform Requirements
    ///
    /// - Wayland (preferred)
    /// - Some X11 window managers support CSD via GTK/Qt
    fn set_decorations_mode(&mut self, mode: DecorationsMode);

    /// Get current decorations mode.
    fn decorations_mode(&self) -> DecorationsMode;

    // ========================================================================
    // X11-Specific Methods
    // ========================================================================

    /// Set X11 window type hint.
    ///
    /// This uses EWMH _NET_WM_WINDOW_TYPE to inform the window manager
    /// about the window's purpose.
    ///
    /// # Platform Requirements
    ///
    /// - X11 only
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.set_x11_window_type(X11WindowType::Dialog);
    /// ```
    #[cfg(feature = "x11")]
    fn set_x11_window_type(&mut self, window_type: X11WindowType);

    /// Get X11 window type.
    #[cfg(feature = "x11")]
    fn x11_window_type(&self) -> X11WindowType;

    /// Set X11 window state hints.
    ///
    /// Uses EWMH _NET_WM_STATE to set window state flags.
    ///
    /// # Platform Requirements
    ///
    /// - X11 only
    #[cfg(feature = "x11")]
    fn set_x11_state(&mut self, state: X11WindowState);

    /// Get X11 window state.
    #[cfg(feature = "x11")]
    fn x11_state(&self) -> X11WindowState;

    /// Set window as sticky (visible on all workspaces).
    ///
    /// # Platform Requirements
    ///
    /// - X11: Uses _NET_WM_STATE_STICKY
    /// - Wayland: Limited support, compositor-dependent
    fn set_sticky(&mut self, sticky: bool);

    /// Check if window is sticky.
    fn is_sticky(&self) -> bool;

    /// Set window urgency hint.
    ///
    /// Marks the window as requiring attention (typically flashes in taskbar).
    ///
    /// # Platform Requirements
    ///
    /// - X11: Uses _NET_WM_STATE_DEMANDS_ATTENTION
    /// - Wayland: Uses xdg-activation protocol if available
    fn set_urgent(&mut self, urgent: bool);

    /// Check if window urgency hint is set.
    fn is_urgent(&self) -> bool;

    // ========================================================================
    // Desktop Integration
    // ========================================================================

    /// Set window class/resource name.
    ///
    /// Used for desktop file matching and window grouping.
    ///
    /// # Platform
    ///
    /// - X11: Sets WM_CLASS property
    /// - Wayland: Sets app_id
    fn set_class(&mut self, class: &str);

    /// Get window class.
    fn class(&self) -> Option<String>;

    /// Set window role.
    ///
    /// # Platform
    ///
    /// - X11: Sets WM_WINDOW_ROLE
    /// - Wayland: No direct equivalent
    fn set_role(&mut self, role: &str);

    /// Get window role.
    fn role(&self) -> Option<String>;

    /// Request activation from compositor.
    ///
    /// Attempts to bring the window to foreground/focus.
    ///
    /// # Platform
    ///
    /// - X11: Uses _NET_ACTIVE_WINDOW
    /// - Wayland: Uses xdg-activation protocol
    fn request_activation(&mut self);

    // ========================================================================
    // Compositor-Specific Features
    // ========================================================================

    /// Enable/disable compositor shadows.
    ///
    /// # Platform
    ///
    /// - X11: Uses _NET_WM_BYPASS_COMPOSITOR or compositor-specific hints
    /// - Wayland: Compositor-dependent
    fn set_compositor_shadow(&mut self, enable: bool);

    /// Set preferred desktop environment theme.
    ///
    /// # Platform
    ///
    /// - GTK-based: Uses GTK theme settings
    /// - KDE: Uses KDE color schemes
    fn set_theme(&mut self, theme: LinuxTheme);

    /// Get current theme.
    fn theme(&self) -> LinuxTheme;
}

// ============================================================================
// Display Server
// ============================================================================

/// Active display server protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayServer {
    /// Wayland compositor.
    Wayland,

    /// X11 display server.
    X11,

    /// Unknown or not yet detected.
    Unknown,
}

// ============================================================================
// Wayland Layer Surface
// ============================================================================

/// Wayland layer surface configuration.
///
/// Layer surfaces are special surfaces that exist in compositor-managed layers.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSurfaceConfig {
    /// Which layer to place the surface in.
    pub layer: LayerSurfaceLayer,

    /// Which edges to anchor to.
    pub anchor: Anchor,

    /// Exclusive zone size (pixels reserved for this surface).
    pub exclusive_zone: i32,

    /// Keyboard interactivity mode.
    pub keyboard_interactivity: KeyboardInteractivity,

    /// Desired size (None = compositor decides).
    pub size: Option<(u32, u32)>,
}

#[cfg(feature = "wayland")]
impl LayerSurfaceConfig {
    /// Create default overlay configuration.
    pub fn overlay() -> Self {
        Self {
            layer: LayerSurfaceLayer::Overlay,
            anchor: Anchor::empty(),
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            size: None,
        }
    }

    /// Create panel configuration (typically for status bars).
    pub fn panel(position: PanelPosition, height: u32) -> Self {
        let (anchor, exclusive_zone) = match position {
            PanelPosition::Top => (Anchor::TOP | Anchor::LEFT | Anchor::RIGHT, height as i32),
            PanelPosition::Bottom => (Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, height as i32),
            PanelPosition::Left => (Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM, height as i32),
            PanelPosition::Right => (Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM, height as i32),
        };

        Self {
            layer: LayerSurfaceLayer::Top,
            anchor,
            exclusive_zone,
            keyboard_interactivity: KeyboardInteractivity::None,
            size: None,
        }
    }
}

/// Wayland layer surface layer.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerSurfaceLayer {
    /// Background layer (below all windows).
    Background,

    /// Bottom layer (below normal windows, above background).
    Bottom,

    /// Top layer (above normal windows).
    Top,

    /// Overlay layer (above everything).
    Overlay,
}

/// Anchor flags for layer surfaces.
#[cfg(feature = "wayland")]
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Anchor: u32 {
        const TOP = 1 << 0;
        const BOTTOM = 1 << 1;
        const LEFT = 1 << 2;
        const RIGHT = 1 << 3;
    }
}

/// Keyboard interactivity mode for layer surfaces.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardInteractivity {
    /// No keyboard input.
    None,

    /// Keyboard input on demand (user must click).
    OnDemand,

    /// Always receive keyboard input.
    Exclusive,
}

/// Panel position for layer surface panels.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelPosition {
    Top,
    Bottom,
    Left,
    Right,
}

// ============================================================================
// Decorations Mode
// ============================================================================

/// Window decorations mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecorationsMode {
    /// Server-side decorations (drawn by compositor/window manager).
    Server,

    /// Client-side decorations (drawn by application).
    Client,

    /// No decorations.
    None,
}

// ============================================================================
// X11 Window Type
// ============================================================================

/// X11 window type hint (EWMH _NET_WM_WINDOW_TYPE).
#[cfg(feature = "x11")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum X11WindowType {
    /// Normal top-level window.
    Normal,

    /// Dialog window.
    Dialog,

    /// Utility window (e.g., palette, toolbox).
    Utility,

    /// Toolbar window.
    Toolbar,

    /// Menu window.
    Menu,

    /// Splash screen.
    Splash,

    /// Desktop background window.
    Desktop,

    /// Dock/panel window.
    Dock,

    /// Notification window.
    Notification,
}

// ============================================================================
// X11 Window State
// ============================================================================

/// X11 window state flags (EWMH _NET_WM_STATE).
#[cfg(feature = "x11")]
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct X11WindowState: u32 {
        /// Window is modal.
        const MODAL = 1 << 0;

        /// Window is sticky (visible on all workspaces).
        const STICKY = 1 << 1;

        /// Window is maximized vertically.
        const MAXIMIZED_VERT = 1 << 2;

        /// Window is maximized horizontally.
        const MAXIMIZED_HORZ = 1 << 3;

        /// Window is shaded (collapsed to title bar).
        const SHADED = 1 << 4;

        /// Window should skip taskbar.
        const SKIP_TASKBAR = 1 << 5;

        /// Window should skip pager.
        const SKIP_PAGER = 1 << 6;

        /// Window is hidden.
        const HIDDEN = 1 << 7;

        /// Window is fullscreen.
        const FULLSCREEN = 1 << 8;

        /// Window should be above others.
        const ABOVE = 1 << 9;

        /// Window should be below others.
        const BELOW = 1 << 10;

        /// Window demands attention.
        const DEMANDS_ATTENTION = 1 << 11;

        /// Window has focus.
        const FOCUSED = 1 << 12;
    }
}

// ============================================================================
// Linux Theme
// ============================================================================

/// Linux desktop theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinuxTheme {
    /// Light theme.
    Light,

    /// Dark theme.
    Dark,

    /// Follow desktop environment preference.
    System,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_server_variants() {
        assert_ne!(DisplayServer::Wayland, DisplayServer::X11);
        assert_eq!(DisplayServer::Unknown, DisplayServer::Unknown);
    }

    #[cfg(feature = "wayland")]
    #[test]
    fn test_layer_surface_config_overlay() {
        let config = LayerSurfaceConfig::overlay();
        assert_eq!(config.layer, LayerSurfaceLayer::Overlay);
        assert_eq!(config.exclusive_zone, 0);
    }

    #[cfg(feature = "wayland")]
    #[test]
    fn test_layer_surface_config_panel() {
        let config = LayerSurfaceConfig::panel(PanelPosition::Top, 32);
        assert_eq!(config.layer, LayerSurfaceLayer::Top);
        assert_eq!(config.exclusive_zone, 32);
        assert!(config.anchor.contains(Anchor::TOP));
    }

    #[cfg(feature = "wayland")]
    #[test]
    fn test_anchor_flags() {
        let anchor = Anchor::TOP | Anchor::LEFT;
        assert!(anchor.contains(Anchor::TOP));
        assert!(anchor.contains(Anchor::LEFT));
        assert!(!anchor.contains(Anchor::BOTTOM));
    }

    #[cfg(feature = "x11")]
    #[test]
    fn test_x11_state_flags() {
        let state = X11WindowState::MAXIMIZED_VERT | X11WindowState::MAXIMIZED_HORZ;
        assert!(state.contains(X11WindowState::MAXIMIZED_VERT));
        assert!(state.contains(X11WindowState::MAXIMIZED_HORZ));
        assert!(!state.contains(X11WindowState::FULLSCREEN));
    }

    #[test]
    fn test_decorations_mode() {
        assert_ne!(DecorationsMode::Server, DecorationsMode::Client);
        assert_ne!(DecorationsMode::Client, DecorationsMode::None);
    }
}

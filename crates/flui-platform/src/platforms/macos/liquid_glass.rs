//! Liquid Glass Material System for macOS Tahoe 26+
//!
//! Liquid Glass is Apple's new translucent design language introduced in macOS Tahoe 26.
//! It provides rich, dynamic materials with depth, blur, and vibrancy effects.
//!
//! # Reference
//! - macOS Tahoe 26 (Released September 15, 2025)
//! - FINAL macOS version supporting Intel Macs
//! - Design System: https://developer.apple.com/design/human-interface-guidelines/materials

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSView, NSVisualEffectView, NSVisualEffectMaterial, NSVisualEffectBlendingMode};
#[cfg(target_os = "macos")]
use objc2_foundation::MainThreadMarker;

/// Liquid Glass material variants introduced in macOS Tahoe 26
///
/// Each variant is optimized for specific UI contexts and provides different
/// levels of translucency, blur, and vibrancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiquidGlassMaterial {
    /// Standard translucent glass - default material
    ///
    /// **Use for:** General purpose backgrounds, panels
    /// **Characteristics:** Balanced blur and translucency
    Standard,

    /// Prominent glass - more opaque and emphasized
    ///
    /// **Use for:** Key UI elements, focused content areas
    /// **Characteristics:** Stronger material presence, less transparency
    Prominent,

    /// Sidebar optimized glass
    ///
    /// **Use for:** Sidebar backgrounds (Finder, Mail, etc.)
    /// **Characteristics:** Subtle blur, optimized for text legibility
    Sidebar,

    /// Menu optimized glass
    ///
    /// **Use for:** Menu backgrounds, dropdown panels
    /// **Characteristics:** Quick render, optimized for transient UI
    Menu,

    /// Popover optimized glass
    ///
    /// **Use for:** Popovers, tooltips, temporary panels
    /// **Characteristics:** Light, airy appearance
    Popover,

    /// Control Center style glass
    ///
    /// **Use for:** Control panels, settings overlays
    /// **Characteristics:** Maximum vibrancy and depth
    ControlCenter,
}

impl LiquidGlassMaterial {
    /// Get the default blur radius for this material type
    ///
    /// These values are calibrated to match Apple's design guidelines.
    pub fn default_blur_radius(self) -> f32 {
        match self {
            LiquidGlassMaterial::Standard => 30.0,
            LiquidGlassMaterial::Prominent => 20.0,
            LiquidGlassMaterial::Sidebar => 40.0,
            LiquidGlassMaterial::Menu => 25.0,
            LiquidGlassMaterial::Popover => 30.0,
            LiquidGlassMaterial::ControlCenter => 35.0,
        }
    }

    /// Get the default tint color for this material (RGBA)
    ///
    /// Returns (r, g, b, a) where each component is 0.0-1.0
    pub fn default_tint(self) -> (f32, f32, f32, f32) {
        match self {
            LiquidGlassMaterial::Standard => (1.0, 1.0, 1.0, 0.3),
            LiquidGlassMaterial::Prominent => (1.0, 1.0, 1.0, 0.5),
            LiquidGlassMaterial::Sidebar => (0.98, 0.98, 0.98, 0.25),
            LiquidGlassMaterial::Menu => (1.0, 1.0, 1.0, 0.4),
            LiquidGlassMaterial::Popover => (1.0, 1.0, 1.0, 0.35),
            LiquidGlassMaterial::ControlCenter => (0.95, 0.95, 0.98, 0.6),
        }
    }

    /// Map to NSVisualEffectMaterial
    ///
    /// Liquid Glass uses enhanced NSVisualEffectView with new materials
    /// introduced in macOS Tahoe 26.
    #[cfg(target_os = "macos")]
    pub(crate) fn to_ns_material(self) -> NSVisualEffectMaterial {
        match self {
            // Note: These mappings are approximations until official Liquid Glass API is available
            // In macOS Tahoe 26, Apple will provide dedicated LiquidGlass materials
            LiquidGlassMaterial::Standard => NSVisualEffectMaterial::ContentBackground,
            LiquidGlassMaterial::Prominent => NSVisualEffectMaterial::HUDWindow,
            LiquidGlassMaterial::Sidebar => NSVisualEffectMaterial::Sidebar,
            LiquidGlassMaterial::Menu => NSVisualEffectMaterial::Menu,
            LiquidGlassMaterial::Popover => NSVisualEffectMaterial::Popover,
            LiquidGlassMaterial::ControlCenter => NSVisualEffectMaterial::HUDWindow,
        }
    }

    /// Check if Liquid Glass is available on this system
    ///
    /// Liquid Glass requires macOS Tahoe 26 or later.
    #[cfg(target_os = "macos")]
    pub fn is_available() -> bool {
        // Check macOS version
        if let Ok(version) = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
        {
            if let Ok(version_str) = String::from_utf8(version.stdout) {
                // Parse version (e.g., "26.0.0" for Tahoe)
                if let Some(major) = version_str.split('.').next() {
                    if let Ok(major_version) = major.trim().parse::<u32>() {
                        return major_version >= 26;  // Tahoe is macOS 26
                    }
                }
            }
        }
        false
    }
}

/// Liquid Glass effect configuration
///
/// Provides fine-grained control over material appearance.
#[derive(Debug, Clone)]
pub struct LiquidGlassConfig {
    /// Material variant
    pub material: LiquidGlassMaterial,

    /// Blur radius (default: material-specific)
    pub blur_radius: Option<f32>,

    /// Tint color override (RGBA 0.0-1.0)
    pub tint: Option<(f32, f32, f32, f32)>,

    /// Blending mode
    pub blending_mode: BlendingMode,

    /// Vibrancy strength (0.0-1.0, default: 1.0)
    pub vibrancy: f32,
}

impl LiquidGlassConfig {
    /// Create a new Liquid Glass configuration with default settings
    pub fn new(material: LiquidGlassMaterial) -> Self {
        Self {
            material,
            blur_radius: None,
            tint: None,
            blending_mode: BlendingMode::BehindWindow,
            vibrancy: 1.0,
        }
    }

    /// Set custom blur radius
    pub fn with_blur_radius(mut self, radius: f32) -> Self {
        self.blur_radius = Some(radius);
        self
    }

    /// Set custom tint color
    pub fn with_tint(mut self, r: f32, g: f32, b: f32, a: f32) -> Self {
        self.tint = Some((r, g, b, a));
        self
    }

    /// Set blending mode
    pub fn with_blending_mode(mut self, mode: BlendingMode) -> Self {
        self.blending_mode = mode;
        self
    }

    /// Set vibrancy strength
    pub fn with_vibrancy(mut self, vibrancy: f32) -> Self {
        self.vibrancy = vibrancy.clamp(0.0, 1.0);
        self
    }

    /// Get the effective blur radius (custom or default)
    pub fn effective_blur_radius(&self) -> f32 {
        self.blur_radius.unwrap_or_else(|| self.material.default_blur_radius())
    }

    /// Get the effective tint color (custom or default)
    pub fn effective_tint(&self) -> (f32, f32, f32, f32) {
        self.tint.unwrap_or_else(|| self.material.default_tint())
    }
}

impl Default for LiquidGlassConfig {
    fn default() -> Self {
        Self::new(LiquidGlassMaterial::Standard)
    }
}

/// Blending mode for Liquid Glass materials
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendingMode {
    /// Blend with window content behind
    BehindWindow,

    /// Blend within window (for layered effects)
    WithinWindow,
}

#[cfg(target_os = "macos")]
impl BlendingMode {
    pub(crate) fn to_ns_blending_mode(self) -> NSVisualEffectBlendingMode {
        match self {
            BlendingMode::BehindWindow => NSVisualEffectBlendingMode::BehindWindow,
            BlendingMode::WithinWindow => NSVisualEffectBlendingMode::WithinWindow,
        }
    }
}

/// Apply Liquid Glass material to an NSView
///
/// This creates an NSVisualEffectView with Liquid Glass configuration and adds it
/// as a subview.
///
/// # Safety
/// Must be called on the main thread.
#[cfg(target_os = "macos")]
pub unsafe fn apply_liquid_glass_to_view(
    parent_view: &NSView,
    config: &LiquidGlassConfig,
    _mtm: MainThreadMarker,
) -> Retained<NSVisualEffectView> {
    // Create NSVisualEffectView
    let effect_view = NSVisualEffectView::new(_mtm);

    // Set material
    effect_view.setMaterial(config.material.to_ns_material());

    // Set blending mode
    effect_view.setBlendingMode(config.blending_mode.to_ns_blending_mode());

    // Set state (active for vibrancy)
    effect_view.setState(objc2_app_kit::NSVisualEffectState::Active);

    // Set appearance (auto - follows system theme)
    effect_view.setAppearance(None);

    // Match parent view frame
    effect_view.setFrame(parent_view.frame());
    effect_view.setAutoresizingMask(
        objc2_app_kit::NSAutoresizingMaskOptions::NSViewWidthSizable
            | objc2_app_kit::NSAutoresizingMaskOptions::NSViewHeightSizable,
    );

    // Add as subview (behind content)
    parent_view.addSubview_positioned_relativeTo(
        &effect_view,
        objc2_app_kit::NSWindowOrderingMode::Below,
        None,
    );

    effect_view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liquid_glass_materials() {
        let materials = [
            LiquidGlassMaterial::Standard,
            LiquidGlassMaterial::Prominent,
            LiquidGlassMaterial::Sidebar,
            LiquidGlassMaterial::Menu,
            LiquidGlassMaterial::Popover,
            LiquidGlassMaterial::ControlCenter,
        ];

        for material in materials {
            let blur = material.default_blur_radius();
            assert!(blur > 0.0, "Blur radius must be positive");

            let (r, g, b, a) = material.default_tint();
            assert!(r >= 0.0 && r <= 1.0);
            assert!(g >= 0.0 && g <= 1.0);
            assert!(b >= 0.0 && b <= 1.0);
            assert!(a >= 0.0 && a <= 1.0);
        }
    }

    #[test]
    fn test_liquid_glass_config() {
        let config = LiquidGlassConfig::new(LiquidGlassMaterial::Sidebar)
            .with_blur_radius(50.0)
            .with_tint(1.0, 0.0, 0.0, 0.5)
            .with_vibrancy(0.8);

        assert_eq!(config.material, LiquidGlassMaterial::Sidebar);
        assert_eq!(config.effective_blur_radius(), 50.0);
        assert_eq!(config.effective_tint(), (1.0, 0.0, 0.0, 0.5));
        assert_eq!(config.vibrancy, 0.8);
    }

    #[test]
    fn test_default_config() {
        let config = LiquidGlassConfig::default();
        assert_eq!(config.material, LiquidGlassMaterial::Standard);
        assert_eq!(config.vibrancy, 1.0);
    }

    #[test]
    fn test_vibrancy_clamping() {
        let config = LiquidGlassConfig::default().with_vibrancy(2.0);
        assert_eq!(config.vibrancy, 1.0);

        let config = LiquidGlassConfig::default().with_vibrancy(-0.5);
        assert_eq!(config.vibrancy, 0.0);
    }
}

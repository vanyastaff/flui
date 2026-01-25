# macOS Platform Improvements Roadmap (2024-2026)

**Document Version:** 1.1  
**Last Updated:** January 25, 2026  
**Research Date:** January 25, 2026  

## Overview

This document outlines modern macOS features and APIs (2024-2026) that could enhance FLUI platform. Based on research of:
- **macOS Sequoia (15)** - Released 2024
- **macOS Tahoe (26)** - Current version, released September 15, 2025
- **Metal 4** - GPU rendering with MetalFX improvements
- **AppKit improvements** from WWDC 2024-2025

**IMPORTANT:** macOS Tahoe (26) is the **final version supporting Intel Macs**. Future versions require Apple Silicon.

---

## 1. Metal 4 Integration (2025) - GPU Rendering

**Priority:** ⭐⭐⭐⭐⭐ (Critical for performance)  
**Target Crate:** `flui_engine` or new `flui-metal`  
**macOS Requirement:** Apple Silicon (M1+/A14+)

### Features

#### MetalFX Frame Interpolation
- **What:** Generates intermediate frames between rendered frames
- **Benefit:** 2x perceived FPS with minimal input lag
- **Use Case:** Smooth UI animations at 120Hz+ displays
- **API:** MetalFX Frame Interpolation
- **Requirements:** Base frame rate >30fps for best results

#### Ray Tracing Denoiser
- **What:** Cleans up noisy ray-traced rendering with fewer rays
- **Benefit:** Real-time path tracing on devices not designed for it
- **Use Case:** High-quality shadows, reflections in UI effects
- **API:** MetalFX Denoising

#### Dynamic Resolution Upscaling
- **What:** Dynamically adjusts input resolution for complex frames
- **Benefit:** Maintains performance during heavy UI operations
- **Use Case:** Complex animations, large canvases, data visualization
- **API:** MetalFX Temporal Upscaler with dynamic input sizing

#### Unified Command Encoder System
- **What:** Streamlined GPU command submission
- **Benefit:** Lower CPU overhead, better multi-threading
- **Use Case:** Parallel UI rendering, multiple windows

#### Neural Rendering Support
- **What:** AI-accelerated rendering techniques
- **Benefit:** ML-based upscaling, effect generation
- **Use Case:** Advanced visual effects, procedural content

### Implementation Plan

**Phase 1:** Metal 4 Foundation
```rust
// crates/flui-metal/src/metal4.rs
pub struct Metal4Renderer {
    device: MTLDevice,
    command_queue: MTLCommandQueue,
    metalfx_upscaler: MTLFXTemporalScaler,
    frame_interpolator: MTLFXFrameInterpolator,
}
```

**Phase 2:** MetalFX Integration
- Temporal upscaling for UI rendering
- Frame interpolation for animations
- Denoising for ray-traced effects

**Phase 3:** Performance Optimization
- Benchmark vs CPU rendering
- Optimize for battery life
- Profile frame timing

### Resources
- [Metal 4: An overview](https://lowendmac.com/2025/metal-4-an-overview/)
- [Metal 4 features for Mac gaming](https://9to5mac.com/2025/06/18/metal-4-two-new-features-that-will-make-a-difference-for-mac-gaming/)
- [What Is Metal 4](https://www.igeeksblog.com/what-is-metal-4-how-it-improves-mac-gaming/)
- [What's New - Metal](https://developer.apple.com/metal/whats-new/)

---

## 2. Liquid Glass Design System (macOS Tahoe 26)

**Priority:** ⭐⭐⭐⭐⭐ (Critical - New design language)  
**Target Crate:** `flui_painting` or `flui-platform`  
**macOS Requirement:** macOS 26 (Tahoe)+

### Features

#### Liquid Glass Material
- **What:** Translucent material that reflects light and color like real glass
- **Benefit:** Modern, elegant visual design matching system UI
- **Use Case:** Panels, overlays, tooltips, menus, toolbars
- **API:** Updated NSVisualEffectView materials
- **Scope:** Desktop icons, folders, Dock, navigation, menus, Control Center

**Implementation:**
```rust
// crates/flui_painting/src/macos/liquid_glass.rs
pub enum LiquidGlassMaterial {
    Standard,        // Default translucent glass
    Prominent,       // More opaque, emphasized
    Sidebar,         // Optimized for sidebars
    Menu,            // Optimized for menus
    Popover,         // Optimized for popovers
    ControlCenter,   // Optimized for Control Center style
}

impl Material {
    pub fn liquid_glass(variant: LiquidGlassMaterial) -> Self {
        unsafe {
            // NSVisualEffectView with Liquid Glass material
            let effect_view = NSVisualEffectView::alloc();
            msg_send![effect_view, setMaterial: variant.to_ns_material()];
        }
    }
}
```

#### Control Design System Updates
- **What:** Extra large control size for primary actions
- **Benefit:** Better emphasis, improved click targets
- **Use Case:** Primary buttons, important controls
- **API:** AppKit control size APIs

**Implementation:**
```rust
// crates/flui_widgets/src/macos/controls.rs
pub enum ControlSize {
    Mini,
    Small,
    Regular,
    Large,
    ExtraLarge,  // NEW in Tahoe
}

impl Button {
    pub fn with_control_size(size: ControlSize) -> Self {
        // Use NSControl.controlSize
    }
    
    pub fn set_tint_prominence(&mut self, prominence: TintProminence) {
        // NEW: Control shape, material, and tint customization
    }
}
```

#### Enhanced Control Heights
- **What:** Taller mini/small/medium controls for better ergonomics
- **Benefit:** Easier to click, better visual breathing room
- **Use Case:** All UI controls (buttons, sliders, etc.)

#### Slider Neutral Value
- **What:** Anchor slider fill at any point along track
- **Benefit:** Flexible slider visualization (e.g., -100 to +100 with 0 center)
- **Use Case:** Volume controls, adjustment sliders
- **API:** `NSSlider.neutralValue`

**Implementation:**
```rust
// crates/flui_widgets/src/macos/slider.rs
impl Slider {
    pub fn set_neutral_value(&mut self, value: f64) {
        unsafe {
            msg_send![self.ns_slider, setNeutralValue: value];
        }
    }
}
```

#### Menu Icon System
- **What:** Single-column menus with icons for key actions
- **Benefit:** Easier to scan, better visual hierarchy
- **Use Case:** Context menus, menu bars
- **API:** NSMenu with SF Symbols

### Resources
- [Apple introduces Liquid Glass design](https://www.apple.com/newsroom/2025/06/apple-introduces-a-delightful-and-elegant-new-software-design/)
- [Build an AppKit app with the new design - WWDC25](https://developer.apple.com/videos/play/wwdc2025/310/)
- [macOS Tahoe - Wikipedia](https://en.wikipedia.org/wiki/MacOS_Tahoe)

---

## 3. AppKit + SwiftUI Integration (macOS Sequoia 15+)

**Priority:** ⭐⭐⭐⭐ (High - Modern UI patterns)  
**Target Crate:** `flui-platform` + `flui_widgets`  
**macOS Requirement:** macOS 15+

### Features

#### NSHostingMenu
- **What:** Use SwiftUI menus inside AppKit applications
- **Benefit:** Share menu definitions between AppKit and SwiftUI parts
- **Use Case:** Unified menu system across FLUI components
- **API:** `NSHostingMenu`

**Implementation:**
```rust
// crates/flui-platform/src/platforms/macos/menu.rs
pub struct SwiftUIMenuBridge {
    hosting_menu: id, // NSHostingMenu*
}

impl SwiftUIMenuBridge {
    pub fn from_swiftui_view(view: SwiftUIView) -> Self {
        // NSHostingMenu(rootView: view)
    }
}
```

#### SwiftUI Animation for NSView
- **What:** Animate NSViews using SwiftUI Animation types
- **Benefit:** Access to SwiftUI's powerful animation system
- **Use Case:** Modern animations in AppKit-based FLUI windows
- **API:** `NSAnimationContext` with SwiftUI animations

**Implementation:**
```rust
// crates/flui_animation/src/macos/swiftui_bridge.rs
pub fn animate_nsview_with_swiftui(
    view: id,
    animation: SwiftUIAnimation,
    changes: impl FnOnce(),
) {
    unsafe {
        NSAnimationContext::runAnimationGroup_completionHandler(
            |context| {
                context.setSwiftUIAnimation(animation);
                changes();
            },
            None,
        );
    }
}
```

### Resources
- [What's new in AppKit - WWDC24](https://developer.apple.com/videos/play/wwdc2024/10124/)
- [macOS Sequoia 15 Release Notes](https://developer.apple.com/documentation/macos-release-notes/macos-15-release-notes)
- [AppKit updates](https://developer.apple.com/documentation/updates/appkit)

---

## 4. Layered SF Symbols (macOS 14+)

**Priority:** ⭐⭐⭐⭐ (High - Modern iconography)  
**Target Crate:** `flui_painting` or `flui_widgets`  
**macOS Requirement:** macOS 14+

### Features

#### Multi-Layer Symbol Rendering
- **What:** SF Symbols with hierarchical layers (primary, secondary, tertiary)
- **Benefit:** Dynamic color theming, semantic icon styling
- **Use Case:** Toolbar icons, button icons with automatic color adaptation
- **API:** `NSImage.SymbolConfiguration`

**Implementation:**
```rust
// crates/flui_painting/src/macos/sf_symbols.rs
pub struct SFSymbol {
    name: String,
    configuration: SymbolConfiguration,
}

pub struct SymbolConfiguration {
    primary_color: Color,
    secondary_color: Color,
    tertiary_color: Color,
    hierarchical_style: HierarchicalStyle,
}

impl SFSymbol {
    pub fn with_hierarchical_colors(
        name: &str,
        colors: Vec<Color>,
    ) -> Self {
        // NSImage.SymbolConfiguration.preferringHierarchical()
    }
}
```

#### Symbol Configuration Merging
- **What:** Combine multiple symbol configurations
- **Benefit:** Flexible icon styling (size + color + weight)
- **API:** `NSImage.SymbolConfiguration.applying(_:)`

### Resources
- [AppKit Release Notes for macOS 14](https://developer.apple.com/documentation/macos-release-notes/appkit-release-notes-for-macos-14)

---

## 5. Window Tiling API (macOS Sequoia 15+)

**Priority:** ⭐⭐⭐ (Medium - Window management)  
**Target Crate:** `flui-platform`  
**macOS Requirement:** macOS 15+

### Features

#### Automatic Window Tiling
- **What:** Windows snap to screen edges for tiling
- **Benefit:** User-friendly window organization
- **Use Case:** Multi-window FLUI applications
- **API:** System-level (Settings > Desktop & Dock > Windows)

**Note:** This is primarily a user-facing feature. No specific API for developers, but FLUI should respect tiling hints.

#### Stage Manager Integration
- **What:** Group windows by task/project
- **Benefit:** Organized workspace management
- **Use Case:** Multi-document FLUI apps

**Implementation:**
```rust
// crates/flui-platform/src/platforms/macos/window_management.rs
impl MacOSWindow {
    /// Opt-in to Stage Manager grouping
    pub fn set_collection_behavior(&self, behavior: NSWindowCollectionBehavior) {
        unsafe {
            msg_send![self.ns_window, setCollectionBehavior: behavior];
        }
    }
}
```

### Resources
- [Window Tiling in macOS Sequoia](https://appleinsider.com/articles/24/07/01/window-tiling-in-macos-sequoia-is-apples-third-go-at-fixing-a-problem)

---

## 6. Accessibility Improvements (2024-2025)

**Priority:** ⭐⭐⭐ (Medium - Inclusivity)  
**Target Crate:** `flui-platform` + `flui_widgets`  
**macOS Requirement:** macOS 15+

### Features

#### VoiceOver Enhancements
- **What:** New voices, custom volume control, keyboard shortcuts
- **Benefit:** Better screen reader experience
- **Use Case:** Accessible FLUI applications
- **API:** NSAccessibility protocol

**Implementation:**
```rust
// crates/flui-platform/src/platforms/macos/accessibility.rs
pub trait Accessible {
    fn accessibility_label(&self) -> String;
    fn accessibility_value(&self) -> Option<String>;
    fn accessibility_hint(&self) -> Option<String>;
    fn accessibility_role(&self) -> AccessibilityRole;
}

impl MacOSWindow {
    pub fn set_accessibility_element(&self, element: &dyn Accessible) {
        unsafe {
            let label = NSString::from_str(&element.accessibility_label());
            msg_send![self.ns_window, setAccessibilityLabel: label];
        }
    }
}
```

#### Live Recognition
- **What:** ML-based object/text recognition
- **Benefit:** Describe UI elements for VoiceOver users
- **Use Case:** Complex UI, images, charts
- **API:** Vision framework integration

#### Accessibility Nutrition Labels
- **What:** App Store labels for supported accessibility features
- **Benefit:** Users know what to expect before downloading
- **Use Case:** Marketing FLUI-based applications

### SwiftUI Accessibility API
- **What:** Improved accessibility modifiers
- **Benefit:** Easier to build accessible UIs
- **API:** SwiftUI accessibility modifiers (if using SwiftUI bridge)

### Resources
- [Catch up on accessibility in SwiftUI - WWDC24](https://developer.apple.com/videos/play/wwdc2024/10073/)
- [Apple accessibility features preview](https://www.idownloadblog.com/2025/05/13/apple-accessibility-features-preview-ios-19-macos-16-watchos-12-visionos-3/)
- [Accessibility | Apple Developer Documentation](https://developer.apple.com/documentation/accessibility)

---

## 7. Toolbar Enhancements (macOS Sequoia 15+)

**Priority:** ⭐⭐⭐ (Medium - UI/UX)  
**Target Crate:** `flui-platform`  
**macOS Requirement:** macOS 15+

### Features

#### Toolbar Display Mode Customization
- **What:** Allow users to choose toolbar style (with/without labels)
- **Benefit:** User preference for toolbar appearance
- **API:** `NSToolbar.allowsDisplayModeCustomization`

**Implementation:**
```rust
// crates/flui-platform/src/platforms/macos/toolbar.rs
impl MacOSToolbar {
    pub fn allow_display_mode_customization(&mut self, allow: bool) {
        unsafe {
            msg_send![self.toolbar, setAllowsDisplayModeCustomization: allow as BOOL];
        }
    }
}
```

#### Simplified Item Management
- **What:** `NSToolbar.itemIdentifiers` property for easier toolbar updates
- **Benefit:** Automatic minimal additions/removals
- **API:** `NSToolbar.itemIdentifiers`

### Resources
- [What's new in AppKit - WWDC24](https://developer.apple.com/videos/play/wwdc2024/10124/)

---

## 8. AppKit Rendering Improvements (macOS 14+)

**Priority:** ⭐⭐ (Low - Edge cases)  
**Target Crate:** `flui-platform`  
**macOS Requirement:** macOS 14+

### Features

#### NSView.clipsToBounds
- **What:** Explicit control over view clipping
- **Benefit:** Performance optimization, custom rendering
- **Default:** `false` in macOS 14 SDK
- **API:** `NSView.clipsToBounds`

**Implementation:**
```rust
// Already in view.rs
impl FLUIContentView {
    pub fn set_clips_to_bounds(&self, clips: bool) {
        unsafe {
            msg_send![self.view, setClipsToBounds: clips as BOOL];
        }
    }
}
```

#### Menu Reimplementation
- **What:** Menus fully rewritten in AppKit (was Carbon-based)
- **Benefit:** Better performance, modern APIs
- **API:** `NSMenu.selectionBehavior`

#### Improved Logging
- **What:** AppKit "warn once" logs moved to `os_log_error()`
- **Benefit:** Better visibility of issues
- **Category:** "WarnOnce"

### Resources
- [AppKit Release Notes for macOS 14](https://developer.apple.com/documentation/macos-release-notes/appkit-release-notes-for-macos-14)

---

## Implementation Priority Matrix

| Feature | Priority | Effort | Impact | Crate | macOS Version |
|---------|----------|--------|--------|-------|---------------|
| Metal 4 Integration | ⭐⭐⭐⭐⭐ | High | High | flui_engine | 26 (Tahoe)+ |
| Liquid Glass Design | ⭐⭐⭐⭐⭐ | Medium | High | flui_painting | 26 (Tahoe)+ |
| Control Design System | ⭐⭐⭐⭐ | Low | Medium | flui_widgets | 26 (Tahoe)+ |
| NSHostingMenu | ⭐⭐⭐⭐ | Medium | Medium | flui-platform | 15 (Sequoia)+ |
| Layered SF Symbols | ⭐⭐⭐⭐ | Low | Medium | flui_painting | 14+ |
| SwiftUI Animations | ⭐⭐⭐ | Medium | Low | flui_animation | 15 (Sequoia)+ |
| Window Tiling | ⭐⭐⭐ | Low | Low | flui-platform | 15 (Sequoia)+ |
| Accessibility | ⭐⭐⭐ | Medium | Medium | flui-platform | 15 (Sequoia)+ |
| Toolbar Enhancements | ⭐⭐⭐ | Low | Low | flui-platform | 15 (Sequoia)+ |
| clipsToBounds | ⭐⭐ | Very Low | Very Low | flui-platform | 14+ |

---

## Recommended Implementation Order

### Phase 1: Foundation (High Impact, Low Risk)
1. ✅ **Layered SF Symbols** - Easy win, modern iconography (macOS 14+)
2. ✅ **Control Design System** - Simple API additions (macOS Tahoe 26)
3. ✅ **clipsToBounds** - One-line addition (macOS 14+)

### Phase 2: Liquid Glass Design (High Impact, Medium Risk)
4. ✅ **Liquid Glass Materials** - Modern translucent UI (macOS Tahoe 26)
5. ✅ **Enhanced Controls** - Taller, better click targets (macOS Tahoe 26)
6. ✅ **Menu Icon System** - Single-column with SF Symbols (macOS Tahoe 26)

### Phase 3: SwiftUI Integration (High Impact, Medium Risk)
7. ✅ **NSHostingMenu** - Bridge to SwiftUI ecosystem (macOS Sequoia 15+)
8. ✅ **SwiftUI Animations** - Modern animation system (macOS Sequoia 15+)
9. ✅ **Accessibility** - Important for production apps (macOS Sequoia 15+)
10. ✅ **Window Tiling** - Respect system behavior (macOS Sequoia 15+)

### Phase 4: Advanced Rendering (High Impact, High Risk)
11. ✅ **Metal 4 Integration** - Major architectural change (macOS Tahoe 26)
   - Start with basic Metal renderer
   - Add MetalFX upscaling
   - Add frame interpolation
   - Add denoising

---

## Architectural Considerations

### Metal 4 Architecture

**Option A: wgpu Backend** (Recommended)
```
flui_painting (Lyon CPU tessellation)
    ↓
flui_engine (wgpu abstraction)
    ↓
wgpu Metal backend → Metal 4 API
```
- ✅ Cross-platform (Windows DirectX 12, Linux Vulkan)
- ✅ Community-maintained
- ⚠️ May lag behind Metal 4 features

**Option B: Native Metal** (Maximum Performance)
```
flui_painting (Lyon CPU tessellation)
    ↓
flui-metal (Direct Metal 4 API)
    ↓
Metal 4 framework
```
- ✅ Access to all Metal 4 features immediately
- ✅ Maximum performance
- ❌ macOS-only
- ❌ More maintenance

**Recommendation:** Start with **Option A (wgpu)**, add **Option B** as optional feature:
```rust
[features]
default = ["wgpu-backend"]
metal4 = ["native-metal"]  # macOS-only optimizations
```

---

## Testing Requirements

### macOS Version Testing Matrix

| Feature | macOS 14 | macOS 15 (Sequoia) | macOS 26 (Tahoe) | macOS 27+ |
|---------|----------|-------------------|------------------|-----------|
| Metal 4 | ❌ | ⚠️ Limited | ✅ Full | ✅ |
| Liquid Glass Design | ❌ | ❌ | ✅ | ✅ |
| Control Design System | ❌ | ❌ | ✅ | ✅ |
| NSHostingMenu | ❌ | ✅ | ✅ | ✅ |
| SF Symbols Layered | ✅ | ✅ | ✅ | ✅ |
| Window Tiling | ❌ | ✅ | ✅ | ✅ |
| Accessibility (new) | ❌ | ✅ | ✅ Enhanced | ✅ |
| SwiftUI Animations | ❌ | ✅ | ✅ | ✅ |

### Hardware Testing Matrix

| Feature | Intel Mac | M1/M2 | M3/M4/M5 |
|---------|-----------|-------|----------|
| Metal 4 | ⚠️ Tahoe only (final) | ✅ | ✅ |
| Liquid Glass | ⚠️ Tahoe only (final) | ✅ | ✅ |
| MetalFX | ⚠️ Limited | ✅ | ✅ |
| Neural Rendering | ❌ | ⚠️ Limited | ✅ |

**IMPORTANT:** macOS Tahoe (26) is the **final version supporting Intel Macs**. macOS 27+ will require Apple Silicon.

---

## Dependencies

### New Rust Crates Needed

```toml
# For Metal 4
metal = "0.29"           # Metal API bindings
metal-rs = "0.29"        # High-level Metal
objc2-metal = "0.2"      # Objective-C 2.0 Metal

# For SwiftUI bridge
swift-rs = "1.0"         # Swift interop (if needed)

# For SF Symbols
cocoa = "0.26"           # Already have, use for NSImage
core-graphics = "0.24"   # Already have

# For Accessibility
accessibility-sys = "0.1" # NSAccessibility bindings (may need to create)
```

---

## References

### Official Documentation
- [What's New - macOS](https://developer.apple.com/macos/whats-new/)
- [macOS Tahoe 26 Release Notes](https://developer.apple.com/documentation/macos-release-notes/macos-26-release-notes)
- [AppKit updates](https://developer.apple.com/documentation/updates/appkit)
- [Metal Overview](https://developer.apple.com/metal/)
- [Accessibility Documentation](https://developer.apple.com/documentation/accessibility)

### WWDC Sessions
- [Build an AppKit app with the new design - WWDC25](https://developer.apple.com/videos/play/wwdc2025/310/)
- [What's new in AppKit - WWDC24](https://developer.apple.com/videos/play/wwdc2024/10124/)
- [Catch up on accessibility in SwiftUI - WWDC24](https://developer.apple.com/videos/play/wwdc2024/10073/)

### Community Resources
- [macOS Tahoe - Wikipedia](https://en.wikipedia.org/wiki/MacOS_Tahoe)
- [macOS 26 Tahoe Guide - MacMegasite](https://macmegasite.com/2026/01/19/macos-26-tahoe-guide-new-features-in-the-latest-update-and-whats-coming-in-macos-26-3/)
- [macOS Tahoe new features - Macworld](https://www.macworld.com/article/2644146/macos-26-release-beta-features-compatibility.html)
- [Metal 4: An overview - Low End Mac](https://lowendmac.com/2025/metal-4-an-overview/)
- [Apple's Metal 4: The Graphics API Revolution](https://medium.com/@shivashanker7337/apples-metal-4-the-graphics-api-revolution-nobody-saw-coming-a2e272be4d57)
- [Apple introduces Liquid Glass design](https://www.apple.com/newsroom/2025/06/apple-introduces-a-delightful-and-elegant-new-software-design/)

---

**Last Updated:** January 25, 2026  
**Current macOS Version:** macOS 26 (Tahoe) - Released September 15, 2025  
**Next macOS Version:** macOS 27 (expected Fall 2026) - Apple Silicon only  
**Next Review:** After WWDC 2026 (June 2026)

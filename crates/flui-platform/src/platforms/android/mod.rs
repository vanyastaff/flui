//! Android platform implementation (stub)
//!
//! This module provides a stub implementation of the Platform trait for Android.
//! It serves as a placeholder for future native Android integration using:
//!
//! - **Android NDK**: Native Development Kit for C/Rust integration
//! - **JNI**: Java Native Interface for calling Android APIs
//! - **android-activity**: Rust wrapper for NativeActivity
//! - **Vulkan/wgpu**: GPU rendering via Android's Vulkan support
//! - **Android Framework APIs**: WindowManager, InputMethodManager, etc.
//!
//! # Current Status
//!
//! ⚠️ **NOT IMPLEMENTED** - This is a stub that returns `unimplemented!()` for all operations.
//!
//! # Implementation Roadmap
//!
//! ## Core Integration
//!
//! 1. **Activity & Lifecycle**:
//!    - android-activity crate for NativeActivity/GameActivity
//!    - Activity lifecycle events (onCreate, onStart, onResume, onPause, onStop, onDestroy)
//!    - App lifecycle state management
//!    - Configuration changes (rotation, screen size)
//!
//! 2. **Window Management**:
//!    - ANativeWindow for surface access
//!    - WindowManager via JNI for display info
//!    - Soft keyboard control (InputMethodManager)
//!    - Immersive mode / fullscreen support
//!    - Multi-window / split-screen support (Android 7+)
//!
//! 3. **Input System**:
//!    - Touch events (ACTION_DOWN, ACTION_UP, ACTION_MOVE, etc.)
//!    - Multi-touch with pointer IDs
//!    - Gesture detection (tap, long-press, fling, pinch)
//!    - Physical keyboard support
//!    - Hardware buttons (back, home, volume)
//!
//! 4. **Rendering**:
//!    - Vulkan surface via ANativeWindow
//!    - wgpu Android backend
//!    - HDR support on capable devices
//!    - Adaptive refresh rate (90Hz, 120Hz, 144Hz)
//!
//! ## Platform Services
//!
//! 5. **Display & Graphics**:
//!    - Display metrics (density, DPI, resolution)
//!    - Cutout/notch handling (DisplayCutout API)
//!    - Multi-display support (external displays)
//!    - Dark mode detection
//!
//! 6. **Text System**:
//!    - Android fonts (/system/fonts/)
//!    - System font fallback
//!    - Emoji support via system fonts
//!    - Text input via IME (soft keyboard)
//!
//! 7. **System Integration**:
//!    - Clipboard via ClipboardManager
//!    - Haptic feedback (Vibrator)
//!    - Notifications
//!    - Permissions (Runtime permissions API)
//!    - Share intent
//!
//! 8. **Async & Threading**:
//!    - Tokio executor for background tasks
//!    - Main thread via Looper.prepare()
//!    - WorkManager for deferred work
//!
//! ## Android-Specific Features
//!
//! 9. **Mobile Capabilities**:
//!    - Battery status
//!    - Network connectivity (WiFi, Cellular, Metered)
//!    - Screen always-on (WAKE_LOCK)
//!    - Picture-in-Picture (Android 8+)
//!    - Foldable device support (Android 10+)
//!
//! # Usage
//!
//! Currently, attempting to use this platform will panic. For Android development,
//! use the winit-based backend or wait for native implementation.
//!
//! ```rust,ignore
//! #[cfg(target_os = "android")]
//! use flui_platform::AndroidPlatform;
//!
//! // This will panic with "not implemented"
//! let platform = AndroidPlatform::new();
//! ```
//!
//! # Dependencies (Future)
//!
//! When implemented, will require:
//! - `android-activity = "0.6"` - Activity wrapper
//! - `jni = "0.21"` - Java Native Interface
//! - `ndk = "0.9"` - Android NDK bindings
//! - `ndk-context` - Global NDK context

pub mod memory;

// Re-export commonly used types
pub use memory::{
    align_to_page_size, align_to_page_size_u64, get_page_size, is_16kb_page_size, PageAlignedVec,
};

use crate::traits::*;
use anyhow::Result;
use std::sync::Arc;

/// Android platform implementation (stub)
///
/// This is a placeholder for future native Android support. All methods
/// currently return `unimplemented!()`.
///
/// # Future Implementation
///
/// Will use Android NDK + JNI:
/// - `android-activity` for NativeActivity
/// - `ANativeWindow` for surface access
/// - `JNI` for calling Android Framework APIs
/// - `Vulkan` for GPU rendering
///
/// # Android API Levels
///
/// Target: API 21+ (Android 5.0 Lollipop)
/// - API 21: Minimum (96% market share)
/// - API 28: Notch support, DisplayCutout
/// - API 29: Dark mode, gesture navigation
/// - API 30: Variable refresh rate
pub struct AndroidPlatform;

impl AndroidPlatform {
    /// Create a new Android platform instance (stub)
    ///
    /// # Panics
    ///
    /// Always panics with "Android platform not yet implemented"
    pub fn new() -> Result<Self> {
        unimplemented!("Android platform not yet implemented - use winit backend or wait for native NDK implementation")
    }

    /// Initialize from android-activity
    ///
    /// # Panics
    ///
    /// Always panics (stub implementation)
    pub fn from_activity(_activity: ()) -> Result<Self> {
        unimplemented!("Android activity initialization not implemented")
    }
}

impl Platform for AndroidPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Android Tokio executor not implemented")
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Android Looper-based executor not implemented")
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        unimplemented!("Android font system not implemented")
    }

    fn run(&self, _on_finish_launching: Box<dyn FnOnce()>) {
        unimplemented!("Android activity event loop not implemented")
    }

    fn quit(&self) {
        unimplemented!("Android finish() not implemented")
    }

    fn request_frame(&self) {
        unimplemented!("Android frame request not implemented")
    }

    fn active_window(&self) -> Option<WindowId> {
        unimplemented!("Android active window query not implemented")
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        unimplemented!("Android Display enumeration not implemented")
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        unimplemented!("Android primary display query not implemented")
    }

    fn open_window(&self, _options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        unimplemented!("Android window creation not implemented - use NativeActivity surface")
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        unimplemented!("Android ClipboardManager not implemented")
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        unimplemented!("Android capabilities not implemented")
    }

    fn name(&self) -> &'static str {
        "Android (stub)"
    }

    fn on_quit(&self, _callback: Box<dyn FnMut() + Send>) {
        unimplemented!("Android quit callback not implemented")
    }

    fn on_window_event(&self, _callback: Box<dyn FnMut(WindowEvent) + Send>) {
        unimplemented!("Android window event callback not implemented")
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        unimplemented!("Android app path query not implemented")
    }
}

// TODO: Implement these when adding native Android support:
//
// Core:
// - AndroidWindow wrapping ANativeWindow
// - AndroidDisplay wrapping Display class via JNI
// - LooperExecutor using Android's Looper
// - AndroidTextSystem using system fonts
//
// Input:
// - Touch event handling (MotionEvent)
// - Multi-touch tracking
// - Soft keyboard (InputMethodManager)
//
// Lifecycle:
// - Activity lifecycle callbacks
// - onConfigurationChanged (rotation, etc.)
// - onWindowFocusChanged
//
// Services:
// - ClipboardManager via JNI
// - Vibrator for haptics
// - WindowManager for display info
// - NotificationManager
//
// Integration:
// - android-activity for NativeActivity
// - JNI bridge utilities
// - wgpu Vulkan surface creation

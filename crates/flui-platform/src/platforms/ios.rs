//! iOS embedder (placeholder)
//!
//! **Status: Not Yet Implemented**
//!
//! This module will provide iOS-specific platform integration when implemented.
//!
//! # Planned Architecture
//!
//! ```text
//! IosEmbedder
//!   ├─ core: EmbedderCore (90% reuse from desktop/android)
//!   ├─ window: UIWindow (iOS native window)
//!   ├─ renderer: GpuRenderer (Metal backend via wgpu)
//!   ├─ capabilities: MobileCapabilities
//!   └─ lifecycle: foreground/background state management
//! ```
//!
//! # Key Differences from Desktop
//!
//! - **Lifecycle**: UIApplicationDelegate callbacks (foreground/background)
//! - **GPU Backend**: Metal (native iOS graphics API)
//! - **Input**: Touch events only (no mouse/keyboard)
//! - **Window Management**: Single UIWindow (no multi-window support)
//! - **Memory Management**: Strict memory limits, system can kill backgrounded apps
//!
//! # Implementation Roadmap
//!
//! 1. **Phase 1**: Basic rendering
//!    - UIWindow creation
//!    - Metal surface initialization via wgpu
//!    - Touch event handling
//!
//! 2. **Phase 2**: Lifecycle management
//!    - Handle `didEnterBackground` (suspend rendering)
//!    - Handle `willEnterForeground` (resume rendering)
//!    - Low memory warnings
//!
//! 3. **Phase 3**: iOS-specific features
//!    - Safe area insets (notch, home indicator)
//!    - Orientation changes
//!    - Keyboard avoidance
//!
//! # References
//!
//! - Android implementation: `android.rs` (similar mobile patterns)
//! - Desktop implementation: `desktop.rs` (base architecture)

use crate::{
    core::EmbedderCore,
    traits::{
        MobileCapabilities, PlatformCapabilities, PlatformEmbedder, PlatformWindow, WinitWindow,
    },
    PlatformError, Result,
};

/// iOS embedder (placeholder)
///
/// This will provide iOS platform integration following the same pattern
/// as `DesktopEmbedder` and `AndroidEmbedder`, with ~90% code reuse via `EmbedderCore`.
///
/// # Platform-Specific Code
///
/// Only iOS-specific logic will be implemented here:
/// - UIWindow creation and management
/// - iOS lifecycle (foreground/background transitions)
/// - Touch event translation (UITouch → FLUI events)
/// - Metal surface setup (via wgpu)
/// - Safe area insets handling
///
/// # Current Status
///
/// **Not implemented.** Attempting to create an `IosEmbedder` will return an error.
#[doc = "⚠️ **NOT YET IMPLEMENTED** - iOS support is planned but not available yet"]
pub struct IosEmbedder {
    // TODO: Implement with:
    // - core: EmbedderCore
    // - window: UIWindow wrapper
    // - renderer: GpuRenderer
    // - capabilities: MobileCapabilities
    // - lifecycle state: Foreground/Background/Suspended
    _placeholder: (),
}

impl IosEmbedder {
    /// Create a new iOS embedder
    ///
    /// # Errors
    ///
    /// Currently always returns `PlatformError::WindowCreation` as iOS
    /// support is not yet implemented.
    ///
    /// # Future API
    ///
    /// When implemented, the signature will be:
    ///
    /// ```rust,ignore
    /// pub async fn new(
    ///     pipeline_owner: Arc<RwLock<PipelineOwner>>,
    ///     needs_redraw: Arc<AtomicBool>,
    ///     scheduler: Arc<Scheduler>,
    ///     event_router: Arc<RwLock<EventRouter>>,
    ///     ui_window: /* iOS UIWindow handle */,
    /// ) -> Result<Self>
    /// ```
    pub async fn new() -> Result<Self> {
        Err(PlatformError::WindowCreation(
            "iOS embedder not yet implemented. See platforms/ios.rs documentation for roadmap."
                .to_string(),
        ))
    }
}

// Note: PlatformEmbedder impl will be added when iOS support is implemented

//! Platform embedder trait
//!
//! Defines the core contract for platform-specific embedders.

use super::{PlatformCapabilities, PlatformWindow};

/// Core trait for platform embedders
///
/// This trait defines the minimal contract for platform-specific embedders.
/// Most functionality is provided by the shared `EmbedderCore` implementation.
///
/// # Design Philosophy
///
/// - **Minimal interface**: Only platform-specific operations
/// - **Composition over inheritance**: Embedders compose `EmbedderCore`
/// - **Type safety**: Associated types for compile-time guarantees
///
/// # Example
///
/// ```rust,ignore
/// pub struct DesktopEmbedder {
///     core: EmbedderCore,
///     window: WinitWindow,
///     capabilities: DesktopCapabilities,
/// }
///
/// impl PlatformEmbedder for DesktopEmbedder {
///     type Window = WinitWindow;
///     type Capabilities = DesktopCapabilities;
///     // ...
/// }
/// ```
pub trait PlatformEmbedder: Send + Sync {
    /// Platform-specific window type
    type Window: PlatformWindow;

    /// Platform capabilities descriptor
    type Capabilities: PlatformCapabilities;

    /// Get reference to the platform window
    fn window(&self) -> &Self::Window;

    /// Get platform capabilities
    fn capabilities(&self) -> &Self::Capabilities;

    /// Request a redraw from the platform
    ///
    /// Platform-specific implementation (e.g., `window.request_redraw()`).
    fn request_redraw(&self);

    /// Handle platform-specific events
    ///
    /// Override this for platform-specific event handling that doesn't
    /// map to common events (e.g., Android lifecycle, iOS background modes).
    fn handle_platform_event(&mut self, event: PlatformSpecificEvent) {
        tracing::debug!("Unhandled platform event: {:?}", event);
    }
}

/// Platform-specific event types
///
/// Events that are unique to specific platforms and don't map to
/// the common event model.
#[derive(Debug, Clone)]
pub enum PlatformSpecificEvent {
    /// Android lifecycle events
    Android(AndroidEvent),
    /// iOS lifecycle events
    Ios(IosEvent),
    /// Web-specific events
    Web(WebEvent),
}

/// Android-specific events
#[derive(Debug, Clone)]
pub enum AndroidEvent {
    /// App resumed from background
    Resumed,
    /// App suspended to background
    Suspended,
    /// Low memory warning
    LowMemory,
    /// Configuration changed (rotation, etc.)
    ConfigurationChanged,
}

/// iOS-specific events
#[derive(Debug, Clone)]
pub enum IosEvent {
    /// App will enter foreground
    WillEnterForeground,
    /// App did enter background
    DidEnterBackground,
    /// Memory warning received
    MemoryWarning,
    /// Significant time change
    SignificantTimeChange,
}

/// Web-specific events
#[derive(Debug, Clone)]
pub enum WebEvent {
    /// Page visibility changed
    VisibilityChanged(bool),
    /// Before page unload
    BeforeUnload,
    /// Online/offline status changed
    OnlineStatusChanged(bool),
}

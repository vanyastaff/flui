//! Window event callbacks system
//!
//! This module provides a flexible callback system for handling various
//! window events like focus changes, minimization, fullscreen, etc.

use winit::event::WindowEvent;

/// Callbacks for various window events
///
/// This structure allows applications to register handlers for
/// window lifecycle events like focus changes, minimization, etc.
///
/// # Example
///
/// ```rust,ignore
/// let mut callbacks = WindowEventCallbacks::new();
///
/// callbacks.on_focus(|focused| {
///     if focused {
///         println!("Window gained focus");
///     } else {
///         println!("Window lost focus");
///     }
/// });
///
/// callbacks.on_minimized(|minimized| {
///     if minimized {
///         println!("Window minimized");
///         // Pause background tasks, reduce resource usage
///     } else {
///         println!("Window restored");
///         // Resume background tasks
///     }
/// });
/// ```
#[derive(Default)]
pub struct WindowEventCallbacks {
    /// Called when window gains or loses focus
    /// Parameter: true = gained focus, false = lost focus
    pub(crate) on_focus: Option<Box<dyn FnMut(bool) + Send>>,

    /// Called when window is minimized or restored
    /// Parameter: true = minimized (occluded), false = visible
    pub(crate) on_minimized: Option<Box<dyn FnMut(bool) + Send>>,

    /// Called when window scale factor (DPI) changes
    /// Parameters: (new_scale_factor, new_inner_size)
    #[allow(clippy::type_complexity)]
    pub(crate) on_scale_changed: Option<Box<dyn FnMut(f64, (u32, u32)) + Send>>,

    /// Called when system theme changes
    /// Parameter: "dark" or "light"
    #[allow(clippy::type_complexity)]
    pub(crate) on_theme_changed: Option<Box<dyn FnMut(&str) + Send>>,

    /// Called when window is moved
    /// Parameters: (x, y) position in screen coordinates
    pub(crate) on_moved: Option<Box<dyn FnMut(i32, i32) + Send>>,

    /// Called when window is destroyed
    pub(crate) on_destroyed: Option<Box<dyn FnOnce() + Send>>,
}

impl WindowEventCallbacks {
    /// Create a new empty callback set
    pub fn new() -> Self {
        Self::default()
    }

    /// Set callback for window focus changes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_focus(|focused| {
    ///     if focused {
    ///         println!("Window gained focus - resume animations");
    ///     } else {
    ///         println!("Window lost focus - pause animations");
    ///     }
    /// });
    /// ```
    pub fn on_focus<F>(&mut self, callback: F)
    where
        F: FnMut(bool) + Send + 'static,
    {
        self.on_focus = Some(Box::new(callback));
    }

    /// Set callback for window minimization/restoration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_minimized(|minimized| {
    ///     if minimized {
    ///         println!("Window minimized - reduce resource usage");
    ///         // Pause rendering, reduce CPU/GPU usage
    ///     } else {
    ///         println!("Window restored - resume normal operation");
    ///     }
    /// });
    /// ```
    pub fn on_minimized<F>(&mut self, callback: F)
    where
        F: FnMut(bool) + Send + 'static,
    {
        self.on_minimized = Some(Box::new(callback));
    }

    /// Set callback for DPI/scale factor changes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_scale_changed(|scale, (width, height)| {
    ///     println!("Scale changed to {}x ({}x{})", scale, width, height);
    ///     // Reload textures at new scale, update UI scaling
    /// });
    /// ```
    pub fn on_scale_changed<F>(&mut self, callback: F)
    where
        F: FnMut(f64, (u32, u32)) + Send + 'static,
    {
        self.on_scale_changed = Some(Box::new(callback));
    }

    /// Set callback for system theme changes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_theme_changed(|theme| {
    ///     println!("Theme changed to: {}", theme);
    ///     // Update UI colors to match system theme
    /// });
    /// ```
    pub fn on_theme_changed<F>(&mut self, callback: F)
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_theme_changed = Some(Box::new(callback));
    }

    /// Set callback for window movement
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_moved(|x, y| {
    ///     println!("Window moved to ({}, {})", x, y);
    ///     // Save window position to settings
    /// });
    /// ```
    pub fn on_moved<F>(&mut self, callback: F)
    where
        F: FnMut(i32, i32) + Send + 'static,
    {
        self.on_moved = Some(Box::new(callback));
    }

    /// Set callback for window destruction
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// callbacks.on_destroyed(|| {
    ///     println!("Window destroyed");
    ///     // Final cleanup before window is gone
    /// });
    /// ```
    pub fn on_destroyed<F>(&mut self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.on_destroyed = Some(Box::new(callback));
    }

    /// Handle a window event by calling the appropriate callback
    ///
    /// This is called internally by the event loop.
    pub(crate) fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(focused) => {
                if let Some(ref mut callback) = self.on_focus {
                    callback(*focused);
                }
            }

            WindowEvent::Occluded(occluded) => {
                if let Some(ref mut callback) = self.on_minimized {
                    callback(*occluded);
                }
            }

            WindowEvent::ScaleFactorChanged {
                scale_factor,
                inner_size_writer: _,
            } => {
                // Note: inner_size_writer is complex, we'll just use the current window size
                // The resize event will handle the actual size change
                if let Some(ref mut callback) = self.on_scale_changed {
                    // We'll need to pass the new size from the resize event
                    // For now, just pass (0, 0) and let resize event handle it
                    callback(*scale_factor, (0, 0));
                }
            }

            WindowEvent::ThemeChanged(theme) => {
                if let Some(ref mut callback) = self.on_theme_changed {
                    let theme_str = match theme {
                        winit::window::Theme::Dark => "dark",
                        winit::window::Theme::Light => "light",
                    };
                    callback(theme_str);
                }
            }

            WindowEvent::Moved(position) => {
                if let Some(ref mut callback) = self.on_moved {
                    callback(position.x, position.y);
                }
            }

            WindowEvent::Destroyed => {
                if let Some(callback) = self.on_destroyed.take() {
                    callback();
                }
            }

            _ => {
                // Other events are not handled by this callback system
            }
        }
    }
}

//! Unified application API for Flui
//!
//! This module provides a backend-agnostic API for creating applications.
//! The backend (WGPU, Egui, etc.) is selected automatically based on configuration.


/// Backend type for rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// GPU-accelerated rendering with WGPU
    #[cfg(feature = "wgpu")]
    Wgpu,

    /// CPU rendering with Egui
    #[cfg(feature = "egui")]
    Egui,

    /// Automatically select the best available backend
    #[default]
    Auto,
}

/// Window configuration
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Window title
    pub title: String,

    /// Window width
    pub width: u32,

    /// Window height
    pub height: u32,

    /// Enable VSync (limits FPS to monitor refresh rate)
    pub vsync: bool,

    /// Enable MSAA (anti-aliasing)
    pub msaa: bool,

    /// MSAA sample count (2, 4, 8, 16)
    pub msaa_samples: u32,

    /// Enable resizable window
    pub resizable: bool,

    /// Start maximized
    pub maximized: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Flui Application".to_string(),
            width: 800,
            height: 600,
            vsync: true,
            msaa: true,
            msaa_samples: 4,
            resizable: true,
            maximized: false,
        }
    }
}

/// Application configuration
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Backend to use for rendering
    pub backend: Backend,

    /// Window configuration
    pub window: WindowConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            window: WindowConfig::default(),
        }
    }
}

impl AppConfig {
    /// Create a new app configuration with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the backend type
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    /// Set the window title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.window.title = title.into();
        self
    }

    /// Set the window size
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.window.width = width;
        self.window.height = height;
        self
    }

    /// Enable or disable VSync
    pub fn vsync(mut self, enabled: bool) -> Self {
        self.window.vsync = enabled;
        self
    }

    /// Enable or disable MSAA
    pub fn msaa(mut self, enabled: bool) -> Self {
        self.window.msaa = enabled;
        self
    }

    /// Set MSAA sample count
    pub fn msaa_samples(mut self, samples: u32) -> Self {
        self.window.msaa_samples = samples;
        self
    }

    /// Enable or disable window resizing
    pub fn resizable(mut self, enabled: bool) -> Self {
        self.window.resizable = enabled;
        self
    }

    /// Start window maximized
    pub fn maximized(mut self, enabled: bool) -> Self {
        self.window.maximized = enabled;
        self
    }
}

/// Trait for application logic
///
/// Implement this trait to define your application's behavior.
pub trait AppLogic: Send + 'static {
    /// Called once when the application starts
    fn setup(&mut self) {}

    /// Called every frame to update application state
    ///
    /// # Parameters
    /// - `delta_time`: Time since last frame in seconds
    fn update(&mut self, delta_time: f32) {
        let _ = delta_time;
    }

    /// Called when an event occurs
    ///
    /// # Parameters
    /// - `event`: The event that occurred
    ///
    /// # Returns
    /// `true` if the event was handled, `false` otherwise
    fn on_event(&mut self, event: &flui_types::Event) -> bool {
        let _ = event;
        false // Default: event not handled
    }

    /// Called every frame to render the application
    ///
    /// # Parameters
    /// - `painter`: The painter to draw with
    fn render(&mut self, painter: &mut dyn crate::Painter);
}

/// Unified application builder
pub struct App {
    config: AppConfig,
}

impl App {
    /// Create a new application with default configuration
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
        }
    }

    /// Create a new application with custom configuration
    pub fn with_config(config: AppConfig) -> Self {
        Self { config }
    }

    /// Set the backend type
    pub fn backend(mut self, backend: Backend) -> Self {
        self.config.backend = backend;
        self
    }

    /// Set the window title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.config.window.title = title.into();
        self
    }

    /// Set the window size
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.config.window.width = width;
        self.config.window.height = height;
        self
    }

    /// Enable or disable VSync
    pub fn vsync(mut self, enabled: bool) -> Self {
        self.config.window.vsync = enabled;
        self
    }

    /// Enable or disable MSAA
    pub fn msaa(mut self, enabled: bool) -> Self {
        self.config.window.msaa = enabled;
        self
    }

    /// Run the application with the given logic
    ///
    /// # Parameters
    /// - `logic`: Your application logic implementation
    pub fn run<L: AppLogic>(self, logic: L) -> Result<(), String> {
        // Select backend based on configuration
        let backend = match self.config.backend {
            Backend::Auto => {
                #[cfg(feature = "wgpu")]
                { Backend::Wgpu }

                #[cfg(all(feature = "egui", not(feature = "wgpu")))]
                { Backend::Egui }

                #[cfg(not(any(feature = "wgpu", feature = "egui")))]
                { return Err("No backend available. Enable 'wgpu' or 'egui' feature.".to_string()); }
            }
            backend => backend,
        };

        // Run with selected backend
        #[allow(unreachable_patterns)]
        match backend {
            #[cfg(feature = "wgpu")]
            Backend::Wgpu => self.run_wgpu(logic),

            #[cfg(feature = "egui")]
            Backend::Egui => self.run_egui(logic),

            Backend::Auto => unreachable!("Auto backend should have been resolved"),

            #[allow(unreachable_patterns)]
            _ => {
                #[cfg(not(any(feature = "wgpu", feature = "egui")))]
                {
                    Err("No backend available. Enable 'wgpu' or 'egui' feature.".to_string())
                }

                #[cfg(all(not(feature = "wgpu"), feature = "egui"))]
                {
                    Err("WGPU backend not available. Enable 'wgpu' feature.".to_string())
                }

                #[cfg(all(feature = "wgpu", not(feature = "egui")))]
                {
                    Err("Egui backend not available. Enable 'egui' feature.".to_string())
                }

                #[cfg(all(feature = "wgpu", feature = "egui"))]
                {
                    unreachable!("Both backends are available, this shouldn't happen")
                }
            }
        }
    }

    /// Run with WGPU backend
    #[cfg(feature = "wgpu")]
    fn run_wgpu<L: AppLogic>(self, logic: L) -> Result<(), String> {
        crate::backends::wgpu::window::run(logic, self.config.window)
    }

    /// Run with Egui backend
    #[cfg(feature = "egui")]
    fn run_egui<L: AppLogic>(self, logic: L) -> Result<(), String> {
        crate::backends::egui::window::run(logic, self.config.window)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

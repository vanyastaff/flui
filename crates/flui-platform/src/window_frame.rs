//! What frames a window: the system's title bar and borders, the application's own title
//! bar inside a frame the system keeps, or nothing at all.

use crate::WindowCapabilities;

/// What frames a window ([`WindowOptions::frame`](crate::WindowOptions::frame),
/// [`WindowCommand::SetFrame`](crate::WindowCommand::SetFrame),
/// [`WindowState::frame`](crate::WindowState::frame)).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub enum WindowFrame {
    /// The system's title bar and borders.
    #[default]
    System,
    /// The application draws the title bar inside its content, and the system keeps the rest
    /// of what a frame does: the resize borders, the shadow and corners, snapping and the
    /// window menu. The application marks where its bar moves the window and where its
    /// caption buttons are with input regions. A window takes it where its
    /// [`WindowCapabilities::app_title_bar`](crate::WindowCapabilities::app_title_bar) is
    /// available.
    AppTitleBar(AppTitleBar),
    /// No frame: no title bar and no system resize borders.
    None,
}

impl WindowFrame {
    /// Whether the system draws the window's title bar.
    #[must_use]
    pub const fn has_system_title_bar(self) -> bool {
        matches!(self, Self::System)
    }

    /// The frame a window asked to open with this one gets under `capabilities`:
    /// [`WindowFrame::System`] in place of a [`WindowFrame::AppTitleBar`] the window
    /// cannot take, so a window never opens without a title bar to move it by.
    #[must_use]
    pub const fn at_open(self, capabilities: &WindowCapabilities) -> Self {
        match self {
            Self::AppTitleBar(_) if !capabilities.app_title_bar.is_available() => Self::System,
            Self::System | Self::AppTitleBar(_) | Self::None => self,
        }
    }
}

/// How a window whose application draws the title bar keeps the system's own controls
/// ([`WindowFrame::AppTitleBar`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct AppTitleBar {
    system_controls: SystemControls,
}

impl AppTitleBar {
    /// A title bar of the application's, with the system's controls where the system puts
    /// them.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            system_controls: SystemControls::Shown,
        }
    }

    /// Keep or hide the system's own controls, or move them.
    #[must_use]
    pub const fn with_system_controls(mut self, controls: SystemControls) -> Self {
        self.system_controls = controls;
        self
    }

    /// The system's controls.
    #[must_use]
    pub const fn system_controls(self) -> SystemControls {
        self.system_controls
    }
}

/// The controls a system draws over a title bar the application draws: macOS's close,
/// minimize and zoom buttons. A system that draws none over such a bar (Windows, Linux)
/// ignores this, and the application draws its caption buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub enum SystemControls {
    /// Where the system puts them.
    #[default]
    Shown,
    /// Moved by an offset from where the system puts them, to sit in a taller bar.
    Offset(ControlsOffset),
    /// Not drawn: the application draws its own.
    Hidden,
}

/// How far the system's controls move from where the system puts them, in logical pixels:
/// toward the trailing edge and down.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ControlsOffset {
    along: f64,
    down: f64,
}

/// A [`ControlsOffset`] whose distances are not finite and non-negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a controls offset is finite and non-negative")]
pub struct InvalidControlsOffset;

impl ControlsOffset {
    /// The offset `along` the bar toward its trailing edge and `down` from its top.
    ///
    /// # Errors
    ///
    /// [`InvalidControlsOffset`] when either distance is negative or not finite.
    pub fn try_new(along: f64, down: f64) -> Result<Self, InvalidControlsOffset> {
        let valid = |value: f64| value.is_finite() && value >= 0.0;
        if valid(along) && valid(down) {
            Ok(Self { along, down })
        } else {
            Err(InvalidControlsOffset)
        }
    }

    /// The distance along the bar, toward its trailing edge.
    #[must_use]
    pub const fn along(self) -> f64 {
        self.along
    }

    /// The distance down from the bar's top.
    #[must_use]
    pub const fn down(self) -> f64 {
        self.down
    }
}

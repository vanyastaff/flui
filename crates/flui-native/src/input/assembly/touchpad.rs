//! A precision touchpad's viewport as plain data (ADR-0217 §4): what a
//! viewport reports (its status and its content transform), and where and
//! when a report happened. The Win32 input copies each DirectManipulation
//! callback into a [`ViewportReading`] and decodes it with its touchpad
//! assembler; a native recording journals the readings, and Linux CI replays
//! them through the same decoder.

use flui_platform::keyboard::Modifiers;

use super::ReadingTime;

/// `DIRECTMANIPULATION_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ViewportStatus {
    /// Being built.
    Building,
    /// Enabled and waiting for a contact.
    Enabled,
    /// Disabled; no input reaches it.
    Disabled,
    /// Fingers are manipulating it.
    Running,
    /// Coasting after the fingers lifted.
    Inertia,
    /// Stopped.
    Ready,
    /// Suspended by the system.
    Suspended,
}

impl ViewportStatus {
    /// The status a native `DIRECTMANIPULATION_STATUS` number names.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Option<Self> {
        Some(match raw {
            0 => Self::Building,
            1 => Self::Enabled,
            2 => Self::Disabled,
            3 => Self::Running,
            4 => Self::Inertia,
            5 => Self::Ready,
            6 => Self::Suspended,
            _ => return None,
        })
    }

    /// Whether the viewport still moves, so its owner must keep updating it.
    #[must_use]
    pub const fn is_animating(self) -> bool {
        matches!(self, Self::Running | Self::Inertia)
    }
}

/// A viewport's content transform: a uniform scale and an offset in device
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentTransform {
    scale: f64,
    x: f64,
    y: f64,
}

impl ContentTransform {
    /// No scale and no offset: the content at the origin.
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        x: 0.0,
        y: 0.0,
    };

    /// A transform; `None` when a component is not finite or `scale` is not
    /// positive.
    #[must_use]
    pub fn try_new(scale: f64, x: f64, y: f64) -> Option<Self> {
        (scale.is_finite() && scale > 0.0 && x.is_finite() && y.is_finite()).then_some(Self {
            scale,
            x,
            y,
        })
    }

    /// The transform of the six-number matrix `GetContentTransform` fills
    /// (`[scale, 0, 0, scale, x, y]`); `None` when it is shorter or not a
    /// finite positive scale with a finite offset.
    #[must_use]
    pub fn from_matrix(matrix: &[f32]) -> Option<Self> {
        let [scale, _, _, _, x, y] = *matrix.first_chunk::<6>()?;
        Self::try_new(f64::from(scale), f64::from(x), f64::from(y))
    }

    /// The uniform scale.
    #[must_use]
    pub const fn scale(&self) -> f64 {
        self.scale
    }

    /// The horizontal offset, in device pixels.
    #[must_use]
    pub const fn x(&self) -> f64 {
        self.x
    }

    /// The vertical offset, in device pixels.
    #[must_use]
    pub const fn y(&self) -> f64 {
        self.y
    }
}

/// One callback of a viewport's event handler, as plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ViewportReading {
    /// `OnViewportStatusChanged`: the viewport's new status.
    Status(ViewportStatus),
    /// `OnContentUpdated`: the content's transform.
    Content(ContentTransform),
}

/// Where and when a viewport reading happened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchpadFrame {
    /// The reading's clocks; the performance counter stamps it when present.
    pub time: ReadingTime,
    /// The cursor, in device pixels of the window's client area: a touchpad
    /// manipulates what is under the cursor.
    pub position: (f64, f64),
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

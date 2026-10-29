//! What a backend's input path can deliver in this session, for the capabilities that have no
//! handle of their own (ADR-0213).
//!
//! Unstable: the SDK and the facade re-export this module's types only behind their additive
//! `unstable-input` feature, until every backend passes its conformance rows (ADR-0213 §5).
//! This crate always builds them, so Cargo's feature unification cannot change them.

mod kind_set;

pub use kind_set::PointerKindSet;

use crate::Availability;

/// The input capabilities of one surface's backend that no handle answers for: pen input,
/// precise (phased) scrolling, trackpad pinch and rotate, input regions and outgoing drags,
/// with the kinds of pointer device attached now.
///
/// Haptics and IME answer through their own handles
/// ([`PlatformWindow::haptics`](crate::PlatformWindow::haptics),
/// [`PlatformWindow::text_input`](crate::PlatformWindow::text_input)), so nothing is reported
/// twice. A window receives its first value as the first
/// [`PlatformInput::Capabilities`](crate::PlatformInput::Capabilities) after its input
/// callback is installed, and every change after that as another one, in order with the input
/// around it.
///
/// The fields are private so that adding a capability is not a breaking change; a backend
/// builds a value with [`InputCapabilities::builder`]. A struct literal does not compile
/// outside this crate:
///
/// ```compile_fail,E0451
/// use flui_platform::Availability;
/// use flui_platform::input_capabilities::InputCapabilities;
/// use flui_platform::input_capabilities::PointerKindSet;
///
/// let forged = InputCapabilities {
///     pen: Availability::Native,
///     precise_scroll: Availability::Native,
///     trackpad_gestures: Availability::Native,
///     input_regions: Availability::Native,
///     outgoing_drag: Availability::Native,
///     present: PointerKindSet::PEN,
/// };
/// ```
///
/// The builder is the way in:
///
/// ```
/// use flui_platform::Availability;
/// use flui_platform::input_capabilities::InputCapabilities;
/// use flui_platform::input_capabilities::PointerKindSet;
///
/// let wayland = InputCapabilities::builder()
///     .pen(Availability::Native)
///     .precise_scroll(Availability::Native)
///     .present(PointerKindSet::MOUSE)
///     .build();
/// assert_eq!(wayland.pen(), Availability::Native);
/// assert!(!wayland.present().contains(PointerKindSet::PEN)); // implemented, none attached
/// assert_eq!(wayland.outgoing_drag(), Availability::Unavailable);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InputCapabilities {
    pen: Availability,
    precise_scroll: Availability,
    trackpad_gestures: Availability,
    input_regions: Availability,
    outgoing_drag: Availability,
    present: PointerKindSet,
}

impl InputCapabilities {
    /// Nothing available and no device attached: what a backend reports for a surface with no
    /// input path.
    pub const UNAVAILABLE: Self = Self {
        pen: Availability::Unavailable,
        precise_scroll: Availability::Unavailable,
        trackpad_gestures: Availability::Unavailable,
        input_regions: Availability::Unavailable,
        outgoing_drag: Availability::Unavailable,
        present: PointerKindSet::empty(),
    };

    /// A builder starting from [`UNAVAILABLE`](Self::UNAVAILABLE).
    pub const fn builder() -> InputCapabilitiesBuilder {
        InputCapabilitiesBuilder(Self::UNAVAILABLE)
    }

    /// A builder starting from this value, so a change to one capability
    /// keeps every other.
    pub const fn to_builder(self) -> InputCapabilitiesBuilder {
        InputCapabilitiesBuilder(self)
    }

    /// Pen input: pressure, angles, hover and the eraser.
    #[must_use]
    pub const fn pen(&self) -> Availability {
        self.pen
    }

    /// Precise scrolling with gesture and momentum phases
    /// ([`ScrollPhase`](crate::pointer::ScrollPhase)), not only wheel detents.
    #[must_use]
    pub const fn precise_scroll(&self) -> Availability {
        self.precise_scroll
    }

    /// Trackpad pinch and rotate, delivered as [`PanZoom`](crate::pointer::PointerEvent::PanZoom).
    #[must_use]
    pub const fn trackpad_gestures(&self) -> Availability {
        self.trackpad_gestures
    }

    /// Input regions the OS honours, such as system-gesture exclusion.
    #[must_use]
    pub const fn input_regions(&self) -> Availability {
        self.input_regions
    }

    /// Starting a drag that leaves the application for the OS. This is the one authoritative
    /// answer; `DragError::Unsupported` (ADR-0211) is the matching error. It is the backend's
    /// answer, not every contact's: where the OS session follows the mouse alone (Win32's
    /// `DoDragDrop`), a pen or touch drag is still refused `Unsupported` at its start and stays
    /// in its window.
    #[must_use]
    pub const fn outgoing_drag(&self) -> Availability {
        self.outgoing_drag
    }

    /// The kinds of pointer device attached now.
    #[must_use]
    pub const fn present(&self) -> PointerKindSet {
        self.present
    }

    /// This value with `present` attached devices, its availabilities unchanged: how a backend
    /// reports a device arriving or leaving.
    #[must_use]
    pub const fn with_present(self, present: PointerKindSet) -> Self {
        Self { present, ..self }
    }
}

impl Default for InputCapabilities {
    fn default() -> Self {
        Self::UNAVAILABLE
    }
}

/// Builds an [`InputCapabilities`]; every capability left unset is
/// [`Availability::Unavailable`] and no device is present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct InputCapabilitiesBuilder(InputCapabilities);

impl InputCapabilitiesBuilder {
    /// Pen input.
    pub const fn pen(self, availability: Availability) -> Self {
        Self(InputCapabilities {
            pen: availability,
            ..self.0
        })
    }

    /// Precise phased scrolling.
    pub const fn precise_scroll(self, availability: Availability) -> Self {
        Self(InputCapabilities {
            precise_scroll: availability,
            ..self.0
        })
    }

    /// Trackpad pinch and rotate.
    pub const fn trackpad_gestures(self, availability: Availability) -> Self {
        Self(InputCapabilities {
            trackpad_gestures: availability,
            ..self.0
        })
    }

    /// Input regions.
    pub const fn input_regions(self, availability: Availability) -> Self {
        Self(InputCapabilities {
            input_regions: availability,
            ..self.0
        })
    }

    /// Outgoing drags to the OS.
    pub const fn outgoing_drag(self, availability: Availability) -> Self {
        Self(InputCapabilities {
            outgoing_drag: availability,
            ..self.0
        })
    }

    /// The device kinds attached now.
    pub const fn present(self, present: PointerKindSet) -> Self {
        Self(InputCapabilities { present, ..self.0 })
    }

    /// The value built.
    #[must_use]
    pub const fn build(self) -> InputCapabilities {
        self.0
    }
}

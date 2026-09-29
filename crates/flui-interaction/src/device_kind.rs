//! The kind of device behind a pointer.

/// The kind of pointer device
///
/// Similar to Flutter's `PointerDeviceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum PointerDeviceKind {
    /// A touch-based pointer device (finger on touchscreen)
    #[default]
    Touch,

    /// A mouse pointer device
    Mouse,

    /// A stylus pointer device
    Stylus,

    /// An inverted stylus (eraser end)
    InvertedStylus,

    /// A trackpad pointer device
    Trackpad,

    /// An unknown pointer device
    Unknown,
}

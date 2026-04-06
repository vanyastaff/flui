//! Frame submission and GPU synchronization.

/// A scissor rectangle in physical pixel coordinates for GPU clipping.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScissorRect {
    /// Left edge in physical pixels.
    pub x: u32,
    /// Top edge in physical pixels.
    pub y: u32,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

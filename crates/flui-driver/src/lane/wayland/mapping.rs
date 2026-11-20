//! What the Wayland lane sends through libei, as pure functions: where a device point lands in
//! the compositor's logical space and its regions, the evdev code of a button, a scroll's logical
//! distance, the isolation modes the lane runs in, and what a portal's answer means.
//!
//! EI places an absolute pointer in the compositor's logical pixels, inside one of the regions
//! the device announced; the driver maps in device pixels, the window's logical points times its
//! scale. A point is divided by the scale and must fall inside a region: a point outside every
//! region is refused, never clamped to the nearest edge.

use flui_protocol::input::PointerButton;
use flui_protocol::session::Isolation;

use crate::coordinates::DevicePoint;
use crate::error::Error;

/// The lane's name, which its refusals carry.
pub(crate) const LANE: &str = "wayland";

/// A region an EI device covers, in the compositor's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EiRegion {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Where the device point `point` of a window at `scale` lands in the compositor's logical space,
/// for an absolute pointer covering `regions`; `None` outside every region, or for a scale that
/// is not finite and positive.
#[must_use]
pub fn ei_point(point: DevicePoint, scale: f64, regions: &[EiRegion]) -> Option<(f32, f32)> {
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let x = f64::from(point.x) / scale;
    let y = f64::from(point.y) / scale;
    let inside = regions.iter().any(|region| {
        x >= f64::from(region.x)
            && y >= f64::from(region.y)
            && x < f64::from(region.x) + f64::from(region.width)
            && y < f64::from(region.y) + f64::from(region.height)
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "inside a region whose edges are u32: within f32's range, to its precision"
    )]
    inside.then_some((x as f32, y as f32))
}

/// The evdev code (`linux/input-event-codes.h`) of `button`; `None` for a button EI has no code
/// for.
#[must_use]
pub fn button_code(button: PointerButton) -> Option<u32> {
    Some(match button {
        PointerButton::Primary => 0x110,   // BTN_LEFT
        PointerButton::Secondary => 0x111, // BTN_RIGHT
        PointerButton::Middle => 0x112,    // BTN_MIDDLE
        PointerButton::Back => 0x113,      // BTN_SIDE
        PointerButton::Forward => 0x114,   // BTN_EXTRA
        _ => return None,
    })
}

/// A scroll of `delta` device pixels at `scale`, in the logical pixels EI scrolls by.
#[must_use]
pub fn scroll_distance(delta: i32, scale: f64) -> f32 {
    if !(scale.is_finite() && scale > 0.0) {
        return 0.0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "an i32 over a positive scale fits f32's range, to its precision"
    )]
    let distance = (f64::from(delta) / scale) as f32;
    distance
}

/// Whether the lane may run in `isolation`: a nested or headless compositor of the run's own
/// (`Seat`), where it sends input, or the user's own session (`Shared`), where it only reads,
/// since it has no guard and `lane::admit` refuses its input there.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for any other mode.
pub fn admit_isolation(isolation: Isolation) -> Result<(), Error> {
    match isolation {
        Isolation::Seat | Isolation::Shared => Ok(()),
        _ => Err(Error::invalid_argument(format!(
            "{} is no isolation mode of a Wayland compositor",
            isolation.name()
        ))),
    }
}

/// What a portal's `Response` with code `response` to its step `step` means: `Ok` for success;
/// a lane the host cannot run (cannot verify) when the user cancelled the dialog (1) or the
/// portal ended the interaction otherwise (2 and anything else).
///
/// # Errors
///
/// [`Error::Lane`] with `unavailable` for every code but 0.
pub fn portal_response(response: u32, step: &str) -> Result<(), Error> {
    let message = match response {
        0 => return Ok(()),
        1 => format!("the RemoteDesktop portal's {step} was refused in its dialog"),
        other => format!("the RemoteDesktop portal ended its {step} (response {other})"),
    };
    Err(Error::Lane {
        lane: LANE.to_owned(),
        unavailable: true,
        message,
    })
}

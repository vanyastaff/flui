//! Where a window was, remembered across runs and fitted to the displays of the
//! next run (ADR-0237 decision 6).
//!
//! A [`WindowPlacement`] names its display by [`DisplayDescription::persistent_key`](crate::DisplayDescription::persistent_key),
//! never by a [`DisplayId`] (an id means nothing in another run), and measures
//! the window from the origin of the display's usable area in that display's
//! logical pixels, so a mixed-scale desktop does not skew it and a window
//! reopens below a panel along the display's top, not under it.
//! [`resolve`](WindowPlacement::resolve) is a pure function of the placement,
//! the current displays and what the window can do; whatever it returns lies
//! wholly on a live display, as a [`WindowPosition`] on that display: the
//! backend turns it into pixels through the display's own scale.
//!
//! # Encoding
//!
//! [`to_bytes`](WindowPlacement::to_bytes) writes text, one `name=value` per
//! line between a version line and an `end` line. `display` and `offset` are
//! absent when the operating system reported neither:
//!
//! ```text
//! flui-window-placement/1
//! display=DP-1
//! offset=120,80
//! size=800,600
//! mode=normal
//! scale=1.5
//! maximized_before_minimize=0
//! end
//! ```
//!
//! A reader of another version, and bytes cut short before `end`, are refused
//! by [`from_bytes`](WindowPlacement::from_bytes); the caller opens the window
//! from its configuration instead.

use std::fmt::Write as _;

use flui_foundation::geometry::{DevicePixelRatio, Point, Size};

use crate::{
    DisplayId, DisplaySnapshot, InvalidGeometry, WindowCapabilities, WindowCommand, WindowExtent,
    WindowMode, WindowPosition, WindowState,
};

const VERSION_LINE: &str = "flui-window-placement/1";
const END_LINE: &str = "end";

/// A window's remembered position, size and mode.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowPlacement {
    /// The display's [`persistent_key`](crate::DisplayDescription::persistent_key), or `None`
    /// when the window was on a display with none: the next run then uses its
    /// primary display.
    pub display: Option<String>,
    /// The outer top-left of the window's normal bounds, measured from the
    /// origin of the display's usable area in that display's logical pixels;
    /// `None` when the operating system reported no position, and the window is
    /// then not positioned when it is restored.
    pub offset: Option<PlacementOffset>,
    /// The size of the window's content area in normal mode, in logical
    /// pixels; one that encloses no area is never written or read, and
    /// [`resolve`](Self::resolve) refuses one.
    pub size: WindowExtent,
    /// The mode the window was in.
    pub mode: WindowMode,
    /// The display's scale factor when this was captured.
    pub scale: DevicePixelRatio,
    /// Whether a window that was minimized was maximized before: it comes back
    /// maximized, not normal.
    pub maximized_before_minimize: bool,
}

/// Where a remembered window's outer top-left is, from the origin of its
/// display's usable area, in that display's logical pixels: finite by
/// construction, so a placement never holds a place that could not be
/// restored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementOffset(Point<f64>);

// Both coordinates are finite, never NaN, so equality is total.
impl Eq for PlacementOffset {}

impl PlacementOffset {
    /// The offset `at` from the usable area's origin.
    ///
    /// # Errors
    /// [`InvalidGeometry::Position`] when a coordinate is NaN or infinite.
    pub fn try_new(at: Point<f64>) -> Result<Self, InvalidGeometry> {
        if at.x.is_finite() && at.y.is_finite() {
            Ok(Self(at))
        } else {
            Err(InvalidGeometry::Position { x: at.x, y: at.y })
        }
    }

    /// The offset, in logical pixels of its display.
    #[must_use]
    pub const fn get(self) -> Point<f64> {
        self.0
    }
}

/// What [`WindowPlacement::resolve`] decided for the window of the next run.
///
/// The geometry is the window's normal bounds. A window that opens maximized or
/// fullscreen returns to them, so a backend gives them to the window system as
/// the rectangle to restore.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedPlacement {
    /// The display the window opens on, whose scale its frame is measured at;
    /// the display [`position`](Self::position) names, when there is one.
    pub display: DisplayId,
    /// The window's outer top-left on [`display`](Self::display), finite.
    at: Option<Point<f64>>,
    /// The content size in that display's logical pixels, enclosing an area.
    /// The window's outer size, with its frame, fits the display's usable
    /// area.
    pub size: WindowExtent,
    /// The mode to open in; never [`WindowMode::Minimized`].
    pub mode: WindowMode,
}

impl ResolvedPlacement {
    /// The window's outer top-left on [`display`](Self::display), inside its
    /// usable area; `None` when the window cannot be placed
    /// ([`places_at_open`](crate::WindowCapabilities::places_at_open) is
    /// `false`) or the placement carried no position.
    #[must_use]
    pub fn position(&self) -> Option<WindowPosition> {
        self.at.map(|at| {
            WindowPosition::try_new(self.display, at)
                .expect("BUG: a resolved position is built from finite numbers")
        })
    }
}

/// Why bytes are not a [`WindowPlacement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlacementError {
    /// The bytes are cut short: a line or the closing `end` is missing.
    #[error("the saved window placement is cut short")]
    Truncated,
    /// The bytes are written in a version of the format this build does not read.
    #[error("the saved window placement is in another version of the format")]
    UnsupportedVersion,
    /// A field is not what the format allows.
    #[error("the saved window placement has a bad `{field}`")]
    Malformed {
        /// The name of the field.
        field: &'static str,
    },
}

impl WindowPlacement {
    /// A placement of a window in `mode`, not known to have been maximized.
    #[must_use]
    pub const fn new(
        display: Option<String>,
        offset: Option<PlacementOffset>,
        size: WindowExtent,
        mode: WindowMode,
        scale: DevicePixelRatio,
    ) -> Self {
        Self {
            display,
            offset,
            size,
            mode,
            scale,
            maximized_before_minimize: false,
        }
    }

    /// Say whether a minimized window was maximized before it was minimized.
    #[must_use]
    pub const fn with_maximized_before_minimize(mut self, maximized: bool) -> Self {
        self.maximized_before_minimize = maximized;
        self
    }

    /// Where `state`'s window is, as a placement.
    ///
    /// The bounds are the window's normal ones: a window whose snapshot names
    /// the rectangle it returns to ([`WindowState::normal_geometry`]: a
    /// maximized, fullscreen or minimized window, or one a mode change is
    /// carrying out in steps) is remembered by it, any other by its outer
    /// rectangle. The size is the content area's, so opening a window with it
    /// does not grow the window by its frame; for a window remembered by the
    /// rectangle it returns to, the frame is taken from the current one.
    ///
    /// The display is the one the normal bounds' position names, or, for a
    /// window whose window system reports no position, the display the window
    /// is on; the offset is the position's, moved from that display's corner
    /// to its usable area's.
    ///
    /// `None` when the window has no usable size, when its position names a
    /// display `displays` does not hold (a monitor the snapshot has not caught
    /// up with), or when it has no position though its host
    /// [observes positions](crate::WindowCapabilities::observes_position) (a
    /// sample that could not place it): a placement is never remembered from
    /// a value that could not be restored, and the one remembered before
    /// stays.
    #[must_use]
    pub fn from_state(
        state: &WindowState,
        displays: &DisplaySnapshot,
        maximized_before_minimize: bool,
    ) -> Option<Self> {
        // The rectangle the window returns to, whenever the snapshot names
        // one: a snapshot of a mode change on its way names it in normal mode
        // too, while its outer rectangle is a step of the change's.
        let (normal, own) = match state.normal_geometry {
            Some(normal) => (normal, false),
            None => (state.outer, true),
        };
        // A rectangle with no area is no place a window was: a window whose
        // window system has reported none yet has nothing to remember.
        let normal_size = normal.extent().area().ok()?;
        let size = if own {
            state.content_size
        } else {
            let frame_width = (state.outer.size().width - state.content_size.width()).max(0.0);
            let frame_height = (state.outer.size().height - state.content_size.height()).max(0.0);
            // Each side is a finite extent less a finite frame, held at one.
            WindowExtent::try_new(Size::new(
                (normal_size.width() - frame_width).max(1.0),
                (normal_size.height() - frame_height).max(1.0),
            ))
            .ok()?
        };
        let size = size.area().ok()?;
        let (display, offset) = match normal.position() {
            Some(position) => {
                let display = displays.get(position.display())?;
                let usable = display.bounds.usable();
                let at = position.at();
                // A finite position less a finite corner can still round past
                // the largest number; such a place is not remembered.
                let offset =
                    PlacementOffset::try_new(Point::new(at.x - usable.min.x, at.y - usable.min.y))
                        .ok()?;
                (Some(display), Some(offset))
            }
            // A host that reports where its windows are and reported nowhere
            // has lost the place, not given it up: an empty offset would
            // overwrite the place remembered before.
            None if state.capabilities.observes_position().is_available() => return None,
            None => (state.display.and_then(|id| displays.get(id)), None),
        };
        let (key, scale) = display.map_or((None, DevicePixelRatio::ONE), |display| {
            (display.persistent_key.clone(), display.scale_factor)
        });
        Some(
            Self::new(key, offset, size, state.mode, scale)
                .with_maximized_before_minimize(maximized_before_minimize),
        )
    }

    /// The placement as bytes, in the format described in the module docs.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = String::new();
        text.push_str(VERSION_LINE);
        text.push('\n');
        if let Some(display) = &self.display {
            text.push_str("display=");
            escape_into(&mut text, display);
            text.push('\n');
        }
        if let Some(offset) = self.offset {
            let at = offset.get();
            let _ = writeln!(text, "offset={},{}", at.x, at.y);
        }
        let _ = writeln!(
            text,
            "size={},{}\nmode={}\nscale={}\nmaximized_before_minimize={}\n{END_LINE}",
            self.size.width(),
            self.size.height(),
            mode_name(self.mode),
            self.scale.get(),
            u8::from(self.maximized_before_minimize),
        );
        text.into_bytes()
    }

    /// Read a placement written by [`to_bytes`](Self::to_bytes).
    ///
    /// # Errors
    /// [`PlacementError::Truncated`] for bytes cut short,
    /// [`PlacementError::UnsupportedVersion`] for another version of the format
    /// (or bytes that are not a placement at all), and
    /// [`PlacementError::Malformed`] for a field the format does not allow: a
    /// number that is not finite, a size that encloses no area, a scale no
    /// display has ([`DevicePixelRatio`]'s range), an unknown mode, a field
    /// given twice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PlacementError> {
        let text = std::str::from_utf8(bytes).map_err(|_| PlacementError::UnsupportedVersion)?;
        let mut lines = text.lines();
        let version = lines.next().ok_or(PlacementError::Truncated)?;
        if version != VERSION_LINE {
            return Err(PlacementError::UnsupportedVersion);
        }
        if !text.ends_with('\n') {
            return Err(PlacementError::Truncated);
        }
        let mut fields = Fields::default();
        let mut ended = false;
        for line in lines {
            if ended {
                return Err(PlacementError::Malformed { field: "end" });
            }
            if line == END_LINE {
                ended = true;
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .ok_or(PlacementError::Malformed { field: "line" })?;
            match name {
                "display" => fields.display.give("display", unescape(value)?)?,
                "offset" => fields.offset.give("offset", pair(value, "offset")?)?,
                "size" => fields.size.give("size", pair(value, "size")?)?,
                "mode" => fields.mode.give("mode", parse_mode(value)?)?,
                "scale" => fields.scale.give("scale", number(value, "scale")?)?,
                "maximized_before_minimize" => fields.maximized.give(
                    "maximized_before_minimize",
                    match value {
                        "0" => false,
                        "1" => true,
                        _ => {
                            return Err(PlacementError::Malformed {
                                field: "maximized_before_minimize",
                            });
                        }
                    },
                )?,
                _ => return Err(PlacementError::Malformed { field: "line" }),
            }
        }
        if !ended {
            return Err(PlacementError::Truncated);
        }
        let size = fields.size.0.ok_or(PlacementError::Truncated)?;
        let scale = fields.scale.0.ok_or(PlacementError::Truncated)?;
        let size = WindowExtent::try_new(Size::new(size.0, size.1))
            .and_then(WindowExtent::area)
            .map_err(|_| PlacementError::Malformed { field: "size" })?;
        let scale =
            DevicePixelRatio::try_new(scale).ok_or(PlacementError::Malformed { field: "scale" })?;
        let offset = fields
            .offset
            .0
            .map(|at| PlacementOffset::try_new(Point::new(at.0, at.1)))
            .transpose()
            .map_err(|_| PlacementError::Malformed { field: "offset" })?;
        Ok(Self::new(
            fields.display.0,
            offset,
            size,
            fields.mode.0.ok_or(PlacementError::Truncated)?,
            scale,
        )
        .with_maximized_before_minimize(fields.maximized.0.ok_or(PlacementError::Truncated)?))
    }

    /// Fit the placement to the displays of this run and to what the window can
    /// do.
    ///
    /// `frame` is how much larger the window's outer size is than its content
    /// size (title bar and borders), in logical pixels: the backend knows it
    /// before a window exists, and headless backends pass zero. The fit is of
    /// the outer size, so a window never hangs past the work area by its frame.
    ///
    /// - The display is the one with this placement's
    ///   [`persistent_key`](crate::DisplayDescription::persistent_key); with no such display,
    ///   or no key, the primary display, else the first. A display whose
    ///   usable area is less than a logical pixel either way is not a
    ///   candidate. On a display other than the remembered one the window
    ///   keeps its physical size, so the logical size is rescaled by the two
    ///   scale factors.
    /// - The outer size is cut to the display's usable area, and the window is
    ///   shifted so that it lies wholly on it: its position is inside the
    ///   usable area, measured from the display's corner as every
    ///   [`WindowPosition`] is.
    /// - A minimized window opens maximized if it was maximized before, else
    ///   normal. A mode the window cannot enter (`capabilities`) becomes
    ///   normal.
    /// - Without [`places_at_open`](crate::WindowCapabilities::places_at_open) the
    ///   position is not applied: size and mode still are.
    ///
    /// `None` when the placement describes no window (a size with no area, a
    /// frame that is not finite) or no display can hold one: the
    /// window then opens from its configuration.
    #[must_use]
    pub fn resolve(
        &self,
        displays: &DisplaySnapshot,
        capabilities: &WindowCapabilities,
        frame: Size<f64>,
    ) -> Option<ResolvedPlacement> {
        let size = self.size.area().ok()?.get();
        if !frame.width.is_finite() || !frame.height.is_finite() {
            return None;
        }
        let frame = Size::new(frame.width.max(0.0), frame.height.max(0.0));
        let wanted = match self.mode {
            WindowMode::Minimized if self.maximized_before_minimize => WindowMode::Maximized,
            WindowMode::Minimized => WindowMode::Normal,
            mode @ (WindowMode::Normal | WindowMode::Maximized | WindowMode::Fullscreen) => mode,
        };
        let mode = if capabilities.permits(&WindowCommand::SetMode(wanted)) {
            wanted
        } else {
            WindowMode::Normal
        };
        let candidates = || {
            displays.displays().iter().filter(|display| {
                let usable = display.bounds.usable();
                usable.width() >= 1.0 && usable.height() >= 1.0
            })
        };
        let keyed = self.display.as_deref().and_then(|key| {
            candidates().find(|display| display.persistent_key.as_deref() == Some(key))
        });
        let remembered = keyed.is_some();
        let display = keyed
            .or_else(|| candidates().find(|display| display.is_primary))
            .or_else(|| candidates().next())?;
        let area = display.bounds.usable();
        // Two ratios between 1/32 and 32, so the quotient is at most 1024. A
        // large extent times it can still overflow to infinity; the `min`
        // with the usable area below is what bounds the outer size, and every
        // side that leaves this function is finite.
        let carry = if remembered {
            1.0
        } else {
            self.scale.get() / display.scale_factor.get()
        };
        let outer = Size::new(
            (size.width * carry + frame.width).min(area.width()),
            (size.height * carry + frame.height).min(area.height()),
        );
        let size = WindowExtent::try_new(Size::new(
            (outer.width - frame.width).max(1.0),
            (outer.height - frame.height).max(1.0),
        ))
        .ok()?;
        let at = if capabilities.places_at_open().is_available() {
            // Each coordinate is a finite offset held between zero and a
            // finite room, plus a finite corner: a point of the usable area.
            let place = |offset: f64, room: f64, corner: f64| corner + offset.min(room).max(0.0);
            self.offset.map(PlacementOffset::get).map(|at| {
                Point::new(
                    place(at.x, area.width() - outer.width, area.min.x),
                    place(at.y, area.height() - outer.height, area.min.y),
                )
            })
        } else {
            None
        };
        Some(ResolvedPlacement {
            display: display.id,
            at,
            size,
            mode,
        })
    }
}

/// Each field of the format at most once.
#[derive(Default)]
struct Fields {
    display: Once<String>,
    offset: Once<(f64, f64)>,
    size: Once<(f64, f64)>,
    mode: Once<WindowMode>,
    scale: Once<f64>,
    maximized: Once<bool>,
}

struct Once<T>(Option<T>);

impl<T> Default for Once<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Once<T> {
    fn give(&mut self, field: &'static str, value: T) -> Result<(), PlacementError> {
        if self.0.replace(value).is_some() {
            return Err(PlacementError::Malformed { field });
        }
        Ok(())
    }
}

fn mode_name(mode: WindowMode) -> &'static str {
    match mode {
        WindowMode::Minimized => "minimized",
        WindowMode::Maximized => "maximized",
        WindowMode::Fullscreen => "fullscreen",
        WindowMode::Normal => "normal",
    }
}

fn parse_mode(value: &str) -> Result<WindowMode, PlacementError> {
    match value {
        "normal" => Ok(WindowMode::Normal),
        "minimized" => Ok(WindowMode::Minimized),
        "maximized" => Ok(WindowMode::Maximized),
        "fullscreen" => Ok(WindowMode::Fullscreen),
        _ => Err(PlacementError::Malformed { field: "mode" }),
    }
}

fn number(value: &str, field: &'static str) -> Result<f64, PlacementError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or(PlacementError::Malformed { field })
}

fn pair(value: &str, field: &'static str) -> Result<(f64, f64), PlacementError> {
    let (first, second) = value
        .split_once(',')
        .ok_or(PlacementError::Malformed { field })?;
    Ok((number(first, field)?, number(second, field)?))
}

/// Write `text` with the three bytes the format reserves escaped: `%`, and the
/// two line breaks.
fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '%' => out.push_str("%25"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            other => out.push(other),
        }
    }
}

fn unescape(value: &str) -> Result<String, PlacementError> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let code = rest
            .get(at + 1..at + 3)
            .ok_or(PlacementError::Malformed { field: "display" })?;
        out.push(match code {
            "25" => '%',
            "0A" => '\n',
            "0D" => '\r',
            _ => return Err(PlacementError::Malformed { field: "display" }),
        });
        rest = &rest[at + 3..];
    }
    out.push_str(rest);
    Ok(out)
}

//! Where a window was, remembered across runs and fitted to the displays of the
//! next run (ADR-0237 decision 6).
//!
//! A [`WindowPlacement`] names its display by [`DisplayInfo::persistent_key`],
//! never by a [`DisplayId`] (an id means nothing in another run), and measures
//! the window from the origin of the display's usable area in that display's
//! logical pixels, so a mixed-scale desktop does not skew it.
//! [`resolve`](WindowPlacement::resolve) is a pure function of the placement,
//! the current displays and what the window can do; whatever it returns lies
//! wholly on a live display and is still expressed relative to that display:
//! the backend turns it into pixels through the display's own scale.
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

use flui_foundation::geometry::{Point, Size};

use crate::{
    DisplayId, DisplayInfo, DisplaySnapshot, WindowCapabilities, WindowCommand, WindowMode,
    WindowState,
};

const VERSION_LINE: &str = "flui-window-placement/1";
const END_LINE: &str = "end";

/// A window's remembered position, size and mode.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowPlacement {
    /// The display's [`persistent_key`](DisplayInfo::persistent_key), or `None`
    /// when the window was on a display with none: the next run then uses its
    /// primary display.
    pub display: Option<String>,
    /// The outer top-left of the window's normal bounds, measured from the
    /// origin of the display's usable area in that display's logical pixels;
    /// `None` when the operating system reported no position, and the window is
    /// then not positioned when it is restored.
    pub offset: Option<Point<f64>>,
    /// The size of the window's content area in normal mode, in logical pixels.
    pub size: Size<f64>,
    /// The mode the window was in.
    pub mode: WindowMode,
    /// The display's scale factor when this was captured.
    pub scale: f64,
    /// Whether a window that was minimized was maximized before: it comes back
    /// maximized, not normal.
    pub maximized_before_minimize: bool,
}

/// What [`WindowPlacement::resolve`] decided for the window of the next run.
///
/// The geometry is the window's normal bounds. A window that opens maximized or
/// fullscreen returns to them, so a backend gives them to the window system as
/// the rectangle to restore.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedPlacement {
    /// The display the window opens on.
    pub display: DisplayId,
    /// The window's outer top-left, from the origin of that display's usable
    /// area in that display's logical pixels; `None` when the window cannot be
    /// placed ([`places_at_open`](WindowCapabilities::places_at_open) is
    /// `false`) or the placement carried no position.
    pub offset_in_display: Option<Point<f64>>,
    /// The content size in that display's logical pixels. The window's outer
    /// size, with its frame, fits the display's usable area.
    pub size: Size<f64>,
    /// The mode to open in; never [`WindowMode::Minimized`].
    pub mode: WindowMode,
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
        offset: Option<Point<f64>>,
        size: Size<f64>,
        mode: WindowMode,
        scale: f64,
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
    /// The bounds are the window's normal ones: a maximized, fullscreen or
    /// minimized window is remembered by the rectangle it returns to
    /// ([`WindowState::normal_geometry`]). The size is the content area's, so
    /// opening a window with it does not grow the window by its frame; for a
    /// window not in normal mode the frame is taken from the current one.
    ///
    /// `None` when the window has no usable size, or a number the state reports
    /// is not finite (its display's scale, or the position): a placement is
    /// never remembered from a value that could not be restored.
    #[must_use]
    pub fn from_state(
        state: &WindowState,
        displays: &DisplaySnapshot,
        maximized_before_minimize: bool,
    ) -> Option<Self> {
        let normal = if state.mode == WindowMode::Normal {
            state.outer
        } else {
            state.normal_geometry.unwrap_or(state.outer)
        };
        let frame_width = (state.outer.size.width - state.content_size.width).max(0.0);
        let frame_height = (state.outer.size.height - state.content_size.height).max(0.0);
        let size = if state.mode == WindowMode::Normal {
            state.content_size
        } else {
            Size::new(
                (normal.size.width - frame_width).max(1.0),
                (normal.size.height - frame_height).max(1.0),
            )
        };
        if !positive_finite(size.width) || !positive_finite(size.height) {
            return None;
        }
        let display = state
            .display
            .and_then(|id| displays.get(id))
            .or_else(|| displays.primary())
            .and_then(|display| usable(display).map(|usable| (display, usable)));
        let (key, scale, offset) = match display {
            Some((display, usable)) => (
                display.persistent_key.clone(),
                display.scale_factor,
                normal
                    .position
                    .map(|at| Point::new(at.x - usable.origin.x, at.y - usable.origin.y)),
            ),
            None => (None, 1.0, normal.position),
        };
        if offset.is_some_and(|at| !at.x.is_finite() || !at.y.is_finite()) {
            return None;
        }
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
            let _ = writeln!(text, "offset={},{}", offset.x, offset.y);
        }
        let _ = writeln!(
            text,
            "size={},{}\nmode={}\nscale={}\nmaximized_before_minimize={}\n{END_LINE}",
            self.size.width,
            self.size.height,
            mode_name(self.mode),
            self.scale,
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
    /// number that is not finite, a size or scale that is not positive, an
    /// unknown mode, a field given twice.
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
        if !positive_finite(size.0) || !positive_finite(size.1) {
            return Err(PlacementError::Malformed { field: "size" });
        }
        if !positive_finite(scale) {
            return Err(PlacementError::Malformed { field: "scale" });
        }
        Ok(Self::new(
            fields.display.0,
            fields.offset.0.map(|at| Point::new(at.0, at.1)),
            Size::new(size.0, size.1),
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
    ///   [`persistent_key`](DisplayInfo::persistent_key); with no such display,
    ///   or no key, the primary display, else the first. A display with a scale
    ///   that is not finite and positive, or with no usable area, is not a
    ///   candidate. On a display other than the remembered one the window keeps
    ///   its physical size, so the logical size is rescaled by the two scale
    ///   factors.
    /// - The outer size is cut to the display's usable area, and the window is
    ///   shifted so that it lies wholly on it.
    /// - A minimized window opens maximized if it was maximized before, else
    ///   normal. A mode the window cannot enter (`capabilities`) becomes
    ///   normal.
    /// - Without [`places_at_open`](WindowCapabilities::places_at_open) the
    ///   position is not applied: size and mode still are.
    ///
    /// `None` when the placement describes no window (a size or scale that is
    /// not finite and positive) or no display can hold one: the window then
    /// opens from its configuration.
    #[must_use]
    pub fn resolve(
        &self,
        displays: &DisplaySnapshot,
        capabilities: &WindowCapabilities,
        frame: Size<f64>,
    ) -> Option<ResolvedPlacement> {
        if !positive_finite(self.size.width)
            || !positive_finite(self.size.height)
            || !positive_finite(self.scale)
            || !frame.width.is_finite()
            || !frame.height.is_finite()
            || self
                .offset
                .is_some_and(|at| !at.x.is_finite() || !at.y.is_finite())
        {
            return None;
        }
        let frame = Size::new(frame.width.max(0.0), frame.height.max(0.0));
        let wanted = match self.mode {
            WindowMode::Minimized if self.maximized_before_minimize => WindowMode::Maximized,
            WindowMode::Minimized => WindowMode::Normal,
            mode => mode,
        };
        let mode = if capabilities.permits(&WindowCommand::SetMode(wanted)) {
            wanted
        } else {
            WindowMode::Normal
        };
        let candidates = || {
            displays
                .displays()
                .iter()
                .filter_map(|display| usable(display).map(|usable| (display, usable)))
        };
        let keyed = self.display.as_deref().and_then(|key| {
            candidates().find(|(display, _)| display.persistent_key.as_deref() == Some(key))
        });
        let remembered = keyed.is_some();
        let (display, area) = keyed
            .or_else(|| candidates().find(|(display, _)| display.is_primary))
            .or_else(|| candidates().next())?;
        let carry = if remembered {
            1.0
        } else {
            self.scale / display.scale_factor
        };
        let outer = Size::new(
            (self.size.width * carry + frame.width).min(area.size.width),
            (self.size.height * carry + frame.height).min(area.size.height),
        );
        let size = Size::new(
            (outer.width - frame.width).max(1.0),
            (outer.height - frame.height).max(1.0),
        );
        let offset_in_display = if capabilities.places_at_open.is_available() {
            self.offset.map(|at| {
                Point::new(
                    at.x.min(area.size.width - outer.width).max(0.0),
                    at.y.min(area.size.height - outer.height).max(0.0),
                )
            })
        } else {
            None
        };
        let resolved = ResolvedPlacement {
            display: display.id,
            offset_in_display,
            size,
            mode,
        };
        let sound = positive_finite(resolved.size.width)
            && positive_finite(resolved.size.height)
            && resolved
                .offset_in_display
                .is_none_or(|at| at.x.is_finite() && at.y.is_finite());
        sound.then_some(resolved)
    }
}

/// A display's usable area in its own logical pixels: where it starts in the
/// global logical space and how large it is. `None` for a display whose scale
/// is not finite and positive or whose usable area is empty.
struct Usable {
    origin: Point<f64>,
    size: Size<f64>,
}

fn usable(display: &DisplayInfo) -> Option<Usable> {
    let scale = display.scale_factor;
    if !positive_finite(scale) {
        return None;
    }
    let bounds = display.usable_bounds;
    let size = Size::new(
        f64::from(bounds.size.width) / scale,
        f64::from(bounds.size.height) / scale,
    );
    (size.width >= 1.0 && size.height >= 1.0).then(|| Usable {
        origin: Point::new(
            f64::from(bounds.origin.x) / scale,
            f64::from(bounds.origin.y) / scale,
        ),
        size,
    })
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
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
        // `Normal`, and a mode added later that this format does not know: it
        // is remembered as the one every window has.
        _ => "normal",
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

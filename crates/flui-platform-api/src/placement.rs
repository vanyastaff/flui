//! Where a window was, remembered across runs and fitted to the displays of the
//! next run (ADR-0237 decision 6).
//!
//! A [`WindowPlacement`] names its display by [`DisplayInfo::persistent_key`],
//! never by a [`DisplayId`](crate::DisplayId) (an id means nothing in another
//! run), and measures the window from the display's usable area in that
//! display's logical pixels, so a mixed-scale desktop does not skew it.
//! [`resolve`](WindowPlacement::resolve) is a pure function of the placement,
//! the current displays and what the window can do; whatever it returns lies
//! wholly on a live display.
//!
//! # Encoding
//!
//! [`encode`](WindowPlacement::encode) writes text, one `name=value` per line
//! between a version line and an `end` line:
//!
//! ```text
//! flui-window-placement/1
//! display=DP-1
//! offset=120,80
//! size=800,600
//! mode=normal
//! scale=1.5
//! maximized=0
//! end
//! ```
//!
//! A reader of another version, and bytes cut short before `end`, are refused
//! by [`decode`](WindowPlacement::decode); the caller opens the window from its
//! configuration instead.

use std::fmt::Write as _;

use flui_foundation::geometry::{Point, Size};

use crate::{
    DisplayInfo, DisplaySnapshot, WindowCapabilities, WindowCommand, WindowMode, WindowState,
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
    /// The top-left of the window's normal bounds, measured from the origin of
    /// the display's usable area in that display's logical pixels.
    pub offset: Point<f64>,
    /// The size of the window's content area in normal mode, in logical pixels.
    pub size: Size<f64>,
    /// The mode the window was in.
    pub mode: WindowMode,
    /// The display's scale factor when this was captured.
    pub scale: f64,
    /// Whether a window that was minimized was maximized before: it comes back
    /// maximized, not normal.
    pub restores_maximized: bool,
}

/// What [`WindowPlacement::resolve`] decided for the window of the next run.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedPlacement {
    /// The display the window opens on; `None` when the platform reports no
    /// displays.
    pub display: Option<crate::DisplayId>,
    /// The global position of the window's outer top-left, in logical pixels;
    /// `None` when the window cannot be placed (`restores_position` is `false`)
    /// or there is no display to place it on.
    pub position: Option<Point<f64>>,
    /// The content size, no larger than the display's usable area.
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
        offset: Point<f64>,
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
            restores_maximized: false,
        }
    }

    /// Say whether a minimized window was maximized before it was minimized.
    #[must_use]
    pub const fn with_restores_maximized(mut self, restores_maximized: bool) -> Self {
        self.restores_maximized = restores_maximized;
        self
    }

    /// Where `state`'s window is, as a placement.
    ///
    /// The bounds are the window's normal ones: a maximized, fullscreen or
    /// minimized window is remembered by the rectangle it returns to
    /// ([`WindowState::normal_geometry`]). The size is the content area's, so
    /// opening a window with it does not grow the window by its frame; for a
    /// window not in normal mode the frame is taken from the current one.
    /// `None` when the window has no usable size.
    #[must_use]
    pub fn capture(
        state: &WindowState,
        displays: &DisplaySnapshot,
        restores_maximized: bool,
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
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return None;
        }
        let display = state
            .display
            .and_then(|id| displays.get(id))
            .or_else(|| displays.primary());
        let (key, scale, origin) = display.map_or((None, 1.0, Point::new(0.0, 0.0)), |display| {
            (
                display.persistent_key.clone(),
                display.scale_factor,
                usable_origin(display),
            )
        });
        let offset = normal.position.map_or_else(
            || Point::new(0.0, 0.0),
            |at| Point::new(at.x - origin.x, at.y - origin.y),
        );
        Some(
            Self::new(key, offset, size, state.mode, scale)
                .with_restores_maximized(restores_maximized),
        )
    }

    /// The placement as bytes, in the format described in the module docs.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut text = String::new();
        text.push_str(VERSION_LINE);
        text.push('\n');
        if let Some(display) = &self.display {
            text.push_str("display=");
            escape_into(&mut text, display);
            text.push('\n');
        }
        let _ = writeln!(
            text,
            "offset={},{}\nsize={},{}\nmode={}\nscale={}\nmaximized={}\n{END_LINE}",
            self.offset.x,
            self.offset.y,
            self.size.width,
            self.size.height,
            mode_name(self.mode),
            self.scale,
            u8::from(self.restores_maximized),
        );
        text.into_bytes()
    }

    /// Read a placement written by [`encode`](Self::encode).
    ///
    /// # Errors
    /// [`PlacementError::Truncated`] for bytes cut short,
    /// [`PlacementError::UnsupportedVersion`] for another version of the format
    /// (or bytes that are not a placement at all), and
    /// [`PlacementError::Malformed`] for a field the format does not allow: a
    /// number that is not finite, a size or scale that is not positive, an
    /// unknown mode.
    pub fn decode(bytes: &[u8]) -> Result<Self, PlacementError> {
        let text = std::str::from_utf8(bytes).map_err(|_| PlacementError::UnsupportedVersion)?;
        let mut lines = text.lines();
        let version = lines.next().ok_or(PlacementError::Truncated)?;
        if version != VERSION_LINE {
            return Err(PlacementError::UnsupportedVersion);
        }
        if !text.ends_with('\n') {
            return Err(PlacementError::Truncated);
        }
        let mut display = None;
        let mut offset = None;
        let mut size = None;
        let mut mode = None;
        let mut scale = None;
        let mut maximized = None;
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
                "display" => display = Some(unescape(value)?),
                "offset" => offset = Some(pair(value, "offset")?),
                "size" => size = Some(pair(value, "size")?),
                "mode" => mode = Some(parse_mode(value)?),
                "scale" => scale = Some(number(value, "scale")?),
                "maximized" => {
                    maximized = Some(match value {
                        "0" => false,
                        "1" => true,
                        _ => return Err(PlacementError::Malformed { field: "maximized" }),
                    });
                }
                _ => return Err(PlacementError::Malformed { field: "line" }),
            }
        }
        if !ended {
            return Err(PlacementError::Truncated);
        }
        let (offset, size, mode, scale, maximized) = (
            offset.ok_or(PlacementError::Truncated)?,
            size.ok_or(PlacementError::Truncated)?,
            mode.ok_or(PlacementError::Truncated)?,
            scale.ok_or(PlacementError::Truncated)?,
            maximized.ok_or(PlacementError::Truncated)?,
        );
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return Err(PlacementError::Malformed { field: "size" });
        }
        if scale <= 0.0 {
            return Err(PlacementError::Malformed { field: "scale" });
        }
        Ok(Self::new(
            display,
            Point::new(offset.0, offset.1),
            Size::new(size.0, size.1),
            mode,
            scale,
        )
        .with_restores_maximized(maximized))
    }

    /// Fit the placement to the displays of this run and to what the window can
    /// do.
    ///
    /// - The display is the one with this placement's
    ///   [`persistent_key`](DisplayInfo::persistent_key); with no such display,
    ///   or no key, the primary display, else the first. On a different
    ///   display the window keeps its physical size, so the logical size is
    ///   rescaled by the two scale factors.
    /// - The size is cut to the display's usable area, and the window is
    ///   shifted so that it lies wholly on it.
    /// - A minimized window opens maximized if it was maximized before, else
    ///   normal. A mode the window cannot enter (`capabilities`) becomes
    ///   normal.
    /// - Without `restores_position` the position is not applied: size and mode
    ///   still are.
    #[must_use]
    pub fn resolve(
        &self,
        displays: &DisplaySnapshot,
        capabilities: &WindowCapabilities,
    ) -> ResolvedPlacement {
        let wanted = match self.mode {
            WindowMode::Minimized if self.restores_maximized => WindowMode::Maximized,
            WindowMode::Minimized => WindowMode::Normal,
            mode => mode,
        };
        let mode = if capabilities.permits(&WindowCommand::SetMode(wanted)) {
            wanted
        } else {
            WindowMode::Normal
        };
        let keyed = self.display.as_deref().and_then(|key| {
            displays
                .displays()
                .iter()
                .find(|display| display.persistent_key.as_deref() == Some(key))
        });
        let Some(display) = keyed
            .or_else(|| displays.primary())
            .or_else(|| displays.displays().first())
        else {
            return ResolvedPlacement {
                display: None,
                position: None,
                size: self.size,
                mode,
            };
        };
        let origin = usable_origin(display);
        let area = Size::new(
            f64::from(display.usable_bounds.size.width) / display.scale_factor,
            f64::from(display.usable_bounds.size.height) / display.scale_factor,
        );
        let carry = if keyed.is_some() {
            1.0
        } else {
            self.scale / display.scale_factor
        };
        let size = Size::new(
            (self.size.width * carry).min(area.width),
            (self.size.height * carry).min(area.height),
        );
        let position = capabilities.restores_position.then(|| {
            Point::new(
                (origin.x + self.offset.x)
                    .min(origin.x + area.width - size.width)
                    .max(origin.x),
                (origin.y + self.offset.y)
                    .min(origin.y + area.height - size.height)
                    .max(origin.y),
            )
        });
        ResolvedPlacement {
            display: Some(display.id),
            position,
            size,
            mode,
        }
    }
}

/// The origin of a display's usable area in that display's logical pixels.
fn usable_origin(display: &DisplayInfo) -> Point<f64> {
    Point::new(
        f64::from(display.usable_bounds.origin.x) / display.scale_factor,
        f64::from(display.usable_bounds.origin.y) / display.scale_factor,
    )
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

//! The frame a Wayland window draws itself where the compositor draws none
//! ([`decoration`](super::decoration)): a title bar with the window's title
//! and its buttons, and the margins its edges are resized from.
//!
//! The frame lies on surfaces of its own around the window's content, each a
//! subsurface of the content's surface: the title bar above it, and four
//! transparent strips outside it that take the pointer for a resize, as
//! libdecor's and GTK's frames do. The content's surface stays the
//! renderer's alone (ADR-0063); the frame is drawn here, on the owner
//! thread, into shared memory, with the title shaped and rasterized by the
//! same text stack the renderer draws text with (`flui-painting`: Parley
//! shapes, swash rasterizes).
//!
//! # What the compositor sees
//!
//! The window's geometry (`xdg_surface.set_window_geometry`) is the title
//! bar and the content: the strips lie outside it, so the compositor places,
//! snaps and tiles the visible window, and the size a configure gives is the
//! geometry's, of which the content is what lies below the title bar. The
//! size limits the window sends are the content's plus the title bar.
//!
//! # What the frame does
//!
//! - **Buttons** after what the compositor says it does (`wm_capabilities`;
//!   all of them where it says nothing): close always, maximize where it
//!   maximizes and the window is resizable, minimize where it minimizes.
//!   A button acts on its release over it, as a desktop's do.
//! - **A press on the title bar** moves the window, through
//!   `xdg_toplevel.move` with the press's seat and serial: the compositor
//!   takes the pointer from there. A second press within
//!   [`DOUBLE_CLICK`] maximizes or restores the window. A secondary press
//!   shows the compositor's window menu (`show_window_menu`) where the
//!   pointer is.
//! - **A press on a margin** resizes the window from that edge or corner
//!   (`xdg_toplevel.resize`), and the pointer over it shows the edge's
//!   cursor.
//! - **The frame goes away** where it has no place: all of it in
//!   fullscreen; the margins of a maximized window and of a window that
//!   cannot be resized; the margin of each edge the compositor tiled.

#![forbid(unsafe_code)]

use cursor_icon::CursorIcon;
use flui_foundation::geometry::Size;

use super::configure::{TiledEdges, ToplevelStates, WmCapabilities};

/// The title bar's height, in logical pixels.
pub(crate) const HEADER: f64 = 37.0;
/// How far the resize margins reach outside the window, in logical pixels.
pub(crate) const MARGIN: f64 = 10.0;
/// How far a corner's resize zone reaches along each edge from the corner,
/// in logical pixels, inside the window and out.
const CORNER: f64 = 24.0;
/// A button's diameter, in logical pixels.
const BUTTON: f64 = 24.0;
/// The space between two buttons, and between the last and the bar's end.
const BUTTON_GAP: f64 = 8.0;
/// The title's font size, in logical pixels.
pub(crate) const TITLE_SIZE: f64 = 15.0;
/// The radius of a floating window's top corners, in logical pixels.
const RADIUS: f64 = 8.0;
/// The longest a second press on the title bar may follow the first and
/// still count as a double press, in milliseconds: GTK's default.
pub(crate) const DOUBLE_CLICK: u32 = 400;
/// The widest or tallest a frame surface is drawn, in device pixels: past
/// it a compositor's configure asks for more memory than any screen shows,
/// and the frame is not drawn.
pub(crate) const MAX_EXTENT: u32 = 16_384;

/// The evdev codes of the pointer buttons the frame answers.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

/// An edge or corner a window is resized from, with its
/// `xdg_toplevel.resize_edge` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Edge {
    /// The `xdg_toplevel.resize_edge` value.
    pub(crate) const fn protocol(self) -> u32 {
        match self {
            Self::Top => 1,
            Self::Bottom => 2,
            Self::Left => 4,
            Self::TopLeft => 5,
            Self::BottomLeft => 6,
            Self::Right => 8,
            Self::TopRight => 9,
            Self::BottomRight => 10,
        }
    }

    /// The cursor the pointer shows over it.
    pub(crate) const fn cursor(self) -> CursorIcon {
        match self {
            Self::Top => CursorIcon::NResize,
            Self::Bottom => CursorIcon::SResize,
            Self::Left => CursorIcon::WResize,
            Self::Right => CursorIcon::EResize,
            Self::TopLeft => CursorIcon::NwResize,
            Self::TopRight => CursorIcon::NeResize,
            Self::BottomLeft => CursorIcon::SwResize,
            Self::BottomRight => CursorIcon::SeResize,
        }
    }

    /// The edge as it stays where the compositor tiled some edges: a tiled
    /// side is not resized from, so a corner keeps only its other side.
    const fn untiled(self, tiled: TiledEdges) -> Option<Self> {
        let (mut top, mut bottom, mut left, mut right) = match self {
            Self::Top => (true, false, false, false),
            Self::Bottom => (false, true, false, false),
            Self::Left => (false, false, true, false),
            Self::Right => (false, false, false, true),
            Self::TopLeft => (true, false, true, false),
            Self::TopRight => (true, false, false, true),
            Self::BottomLeft => (false, true, true, false),
            Self::BottomRight => (false, true, false, true),
        };
        top &= !tiled.top;
        bottom &= !tiled.bottom;
        left &= !tiled.left;
        right &= !tiled.right;
        match (top, bottom, left, right) {
            (true, _, true, _) => Some(Self::TopLeft),
            (true, _, _, true) => Some(Self::TopRight),
            (_, true, true, _) => Some(Self::BottomLeft),
            (_, true, _, true) => Some(Self::BottomRight),
            (true, ..) => Some(Self::Top),
            (_, true, ..) => Some(Self::Bottom),
            (_, _, true, _) => Some(Self::Left),
            (_, _, _, true) => Some(Self::Right),
            _ => None,
        }
    }
}

/// A button of the title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    Minimize,
    Maximize,
    Close,
}

/// What lies under a point of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// The title bar, away from its buttons.
    Caption,
    /// A button.
    Button(Button),
    /// A resize margin.
    Edge(Edge),
}

/// One surface of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    Header,
    Top,
    Bottom,
    Left,
    Right,
}

impl Piece {
    /// Every piece, the title bar first.
    pub(crate) const ALL: [Self; 5] = [
        Self::Header,
        Self::Top,
        Self::Bottom,
        Self::Left,
        Self::Right,
    ];
}

/// A rectangle in logical pixels, relative to the content surface's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// What the compositor lets the frame offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offers {
    /// It minimizes a window on request.
    pub(crate) minimize: bool,
    /// It maximizes a window on request.
    pub(crate) maximize: bool,
    /// It shows a window menu on request.
    pub(crate) window_menu: bool,
}

impl Offers {
    /// What `announced` lets the frame offer: everything where the
    /// compositor announced nothing (before `xdg_wm_base` version 5), as the
    /// window cannot know what it ignores.
    pub(crate) const fn from(announced: Option<WmCapabilities>) -> Self {
        match announced {
            Some(announced) => Self {
                minimize: announced.minimize,
                maximize: announced.maximize,
                window_menu: announced.window_menu,
            },
            None => Self {
                minimize: true,
                maximize: true,
                window_menu: true,
            },
        }
    }
}

/// What a client-drawn frame is laid out from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// The content's size, in logical pixels.
    pub(crate) content: Size<f64>,
    /// The toplevel's states.
    pub(crate) states: ToplevelStates,
    /// Whether the user may resize the window.
    pub(crate) resizable: bool,
    /// What the compositor lets the frame offer.
    pub(crate) offers: Offers,
}

impl Frame {
    /// Whether the frame is shown: never in fullscreen.
    pub(crate) const fn shown(&self) -> bool {
        !self.states.fullscreen
    }

    /// The title bar's height: [`HEADER`] while the frame is shown.
    pub(crate) const fn header(&self) -> f64 {
        if self.shown() { HEADER } else { 0.0 }
    }

    /// The edges the margins are on: none for a frame not shown, a
    /// maximized window or one the user may not resize; none on an edge the
    /// compositor tiled.
    pub(crate) const fn edges(&self) -> TiledEdges {
        if !self.shown() || self.states.maximized || !self.resizable {
            return TiledEdges::NONE;
        }
        let tiled = self.states.tiled;
        TiledEdges {
            left: !tiled.left,
            right: !tiled.right,
            top: !tiled.top,
            bottom: !tiled.bottom,
        }
    }

    /// The window's geometry relative to the content's origin: the title bar
    /// and the content.
    pub(crate) const fn geometry(&self) -> Rect {
        let header = self.header();
        Rect {
            x: 0.0,
            y: -header,
            width: self.content.width,
            height: self.content.height + header,
        }
    }

    /// Where `piece` lies relative to the content's origin, or `None` where
    /// it is not shown.
    pub(crate) fn rect(&self, piece: Piece) -> Option<Rect> {
        let Size { width, height } = self.content;
        let header = self.header();
        let edges = self.edges();
        // The top and bottom margins reach past the sides only where the
        // side has a margin of its own.
        let left = if edges.left { -MARGIN } else { 0.0 };
        let right = if edges.right { width + MARGIN } else { width };
        let rect = match piece {
            Piece::Header => self.shown().then_some(Rect {
                x: 0.0,
                y: -header,
                width,
                height: header,
            }),
            Piece::Top => edges.top.then_some(Rect {
                x: left,
                y: -header - MARGIN,
                width: right - left,
                height: MARGIN,
            }),
            Piece::Bottom => edges.bottom.then_some(Rect {
                x: left,
                y: height,
                width: right - left,
                height: MARGIN,
            }),
            Piece::Left => edges.left.then_some(Rect {
                x: -MARGIN,
                y: -header,
                width: MARGIN,
                height: header + height,
            }),
            Piece::Right => edges.right.then_some(Rect {
                x: width,
                y: -header,
                width: MARGIN,
                height: header + height,
            }),
        };
        rect.filter(|rect| rect.width > 0.0 && rect.height > 0.0)
    }

    /// The buttons the title bar shows, from its end: close always,
    /// maximize where the compositor maximizes and the window is resizable,
    /// minimize where the compositor minimizes.
    pub(crate) fn buttons(&self) -> impl Iterator<Item = Button> {
        let maximize = self.offers.maximize && self.resizable;
        [
            Some(Button::Close),
            maximize.then_some(Button::Maximize),
            self.offers.minimize.then_some(Button::Minimize),
        ]
        .into_iter()
        .flatten()
    }

    /// Where `button` lies in the title bar, relative to its origin; `None`
    /// for a button the bar does not show.
    pub(crate) fn button_rect(&self, button: Button) -> Option<Rect> {
        let index = self.buttons().position(|shown| shown == button)?;
        let steps = f64::from(u32::try_from(index).ok()?);
        let x = self.content.width - (BUTTON_GAP + BUTTON) * (steps + 1.0);
        Some(Rect {
            x,
            y: (HEADER - BUTTON) / 2.0,
            width: BUTTON,
            height: BUTTON,
        })
    }

    /// How much of the title bar's width the buttons take from each side,
    /// so the title, centred, keeps clear of them.
    fn buttons_extent(&self) -> f64 {
        let count = f64::from(u32::try_from(self.buttons().count()).unwrap_or(3));
        (BUTTON_GAP + BUTTON) * count + BUTTON_GAP
    }

    /// The widest the title is drawn, in logical pixels.
    pub(crate) fn title_room(&self) -> f64 {
        (self.content.width - 2.0 * self.buttons_extent()).max(0.0)
    }

    /// What lies at (`x`, `y`) on `piece`, in that piece's own logical
    /// coordinates; `None` off the frame.
    pub(crate) fn part_at(&self, piece: Piece, x: f64, y: f64) -> Option<Part> {
        let rect = self.rect(piece)?;
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        if piece == Piece::Header {
            let hit = self.buttons().find(|&button| {
                self.button_rect(button).is_some_and(|area| {
                    x >= area.x
                        && x < area.x + area.width
                        && y >= area.y
                        && y < area.y + area.height
                })
            });
            return Some(hit.map_or(Part::Caption, Part::Button));
        }
        // On the content's coordinates, where the corners are measured.
        let (px, py) = (rect.x + x, rect.y + y);
        let reach = CORNER - MARGIN;
        let top = -self.header();
        let bottom = self.content.height;
        let near_left = px < reach;
        let near_right = px > self.content.width - reach;
        let near_top = py < top + reach;
        let near_bottom = py > bottom - reach;
        let edge = match piece {
            Piece::Top | Piece::Bottom => {
                let top_side = piece == Piece::Top;
                match (near_left, near_right) {
                    (true, _) if top_side => Edge::TopLeft,
                    (_, true) if top_side => Edge::TopRight,
                    _ if top_side => Edge::Top,
                    (true, _) => Edge::BottomLeft,
                    (_, true) => Edge::BottomRight,
                    _ => Edge::Bottom,
                }
            }
            Piece::Left if near_top => Edge::TopLeft,
            Piece::Left if near_bottom => Edge::BottomLeft,
            Piece::Left => Edge::Left,
            Piece::Right if near_top => Edge::TopRight,
            Piece::Right if near_bottom => Edge::BottomRight,
            _ => Edge::Right,
        };
        edge.untiled(self.states.tiled).map(Part::Edge)
    }

    /// The content size a configure of `width` by `height` (the window
    /// geometry's, `0` on an axis left to the window) gives, in the same
    /// terms: the title bar comes off the height.
    pub(crate) fn content_of(&self, width: i32, height: i32) -> (i32, i32) {
        let header = header_extent(self.header());
        let height = if height > 0 {
            height.saturating_sub(header).max(1)
        } else {
            height
        };
        (width, height)
    }
}

/// The title bar's height as the protocol's `int`.
fn header_extent(header: f64) -> i32 {
    rounded(header)
}

/// What a press or release on the frame asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Move it with the pointer (`xdg_toplevel.move`).
    Move,
    /// Resize it from an edge (`xdg_toplevel.resize`).
    Resize(Edge),
    /// Show the compositor's window menu at a point relative to the
    /// content's origin (`xdg_toplevel.show_window_menu`).
    Menu { x: i32, y: i32 },
    /// Ask the window's handler to close it.
    Close,
    /// Minimize it.
    Minimize,
    /// Maximize it.
    Maximize,
    /// Take it out of maximized.
    Restore,
}

/// The pointer over a frame: the part it is over, the button held, and
/// when the title bar was last pressed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct PointerOnFrame {
    /// The part under the pointer, and where it is relative to the content's
    /// origin.
    over: Option<(Part, f64, f64)>,
    /// The button pressed with the primary button and not released yet.
    held: Option<Button>,
    /// When the title bar was last pressed with the primary button, in the
    /// compositor's milliseconds.
    last_caption: Option<u32>,
}

impl PointerOnFrame {
    /// The part under the pointer.
    pub(crate) fn hovered(&self) -> Option<Part> {
        self.over.map(|(part, ..)| part)
    }

    /// The button pressed and not released yet.
    pub(crate) const fn held(&self) -> Option<Button> {
        self.held
    }

    /// The pointer is at (`x`, `y`) on `piece` of `frame`; the cursor it
    /// shows there.
    pub(crate) fn moved(&mut self, frame: &Frame, piece: Piece, x: f64, y: f64) -> CursorIcon {
        let part = frame.part_at(piece, x, y);
        self.over = part.and_then(|part| {
            let rect = frame.rect(piece)?;
            Some((part, rect.x + x, rect.y + y))
        });
        match part {
            Some(Part::Edge(edge)) => edge.cursor(),
            _ => CursorIcon::Default,
        }
    }

    /// The pointer left the frame: a button held is let go.
    pub(crate) fn left(&mut self) {
        self.over = None;
        self.held = None;
    }

    /// Pointer button `code` (evdev) went down or up at `time` (the
    /// compositor's milliseconds) over the part last hovered; what that asks
    /// of the window.
    pub(crate) fn button(
        &mut self,
        frame: &Frame,
        code: u32,
        pressed: bool,
        time: u32,
    ) -> Option<Action> {
        let (part, x, y) = self.over?;
        match (code, pressed, part) {
            (BTN_LEFT, true, Part::Caption) => {
                let double = self
                    .last_caption
                    .is_some_and(|last| time.wrapping_sub(last) < DOUBLE_CLICK);
                self.last_caption = (!double).then_some(time);
                let toggles = frame.offers.maximize && frame.resizable;
                Some(match (double && toggles, frame.states.maximized) {
                    (true, true) => Action::Restore,
                    (true, false) => Action::Maximize,
                    (false, _) => Action::Move,
                })
            }
            (BTN_LEFT, true, Part::Edge(edge)) => Some(Action::Resize(edge)),
            (BTN_LEFT, true, Part::Button(button)) => {
                self.held = Some(button);
                None
            }
            (BTN_LEFT, false, part) => {
                let held = self.held.take()?;
                (part == Part::Button(held)).then_some(match held {
                    Button::Close => Action::Close,
                    Button::Minimize => Action::Minimize,
                    Button::Maximize if frame.states.maximized => Action::Restore,
                    Button::Maximize => Action::Maximize,
                })
            }
            (BTN_RIGHT, true, Part::Caption | Part::Button(_)) if frame.offers.window_menu => {
                self.last_caption = None;
                Some(Action::Menu {
                    x: rounded(x),
                    y: rounded(y),
                })
            }
            _ => None,
        }
    }
}

/// `value` rounded to the protocol's `int`, held within its range.
pub(crate) fn rounded(value: f64) -> i32 {
    let value = value.round();
    if !value.is_finite() {
        0
    } else if value >= f64::from(i32::MAX) {
        i32::MAX
    } else if value <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        // In range and integral: the cast is exact.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "checked against the i32 range and rounded above"
        )]
        let value = value as i32;
        value
    }
}

/// How the title bar looks: dark or light, after the desktop's preference,
/// and active or not, after the compositor's activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Look {
    pub(crate) dark: bool,
    pub(crate) active: bool,
}

/// A straight colour, `[r, g, b]`.
type Rgb = [u8; 3];

impl Look {
    /// The bar's background.
    fn background(self) -> Rgb {
        match (self.dark, self.active) {
            (false, true) => [235, 235, 235],
            (false, false) => [250, 250, 250],
            (true, true) => [48, 48, 48],
            (true, false) => [36, 36, 36],
        }
    }

    /// The title's and the icons' colour.
    pub(crate) fn foreground(self) -> Rgb {
        match (self.dark, self.active) {
            (false, true) => [46, 46, 46],
            (false, false) => [146, 146, 146],
            (true, true) => [255, 255, 255],
            (true, false) => [145, 145, 145],
        }
    }

    /// The background mixed toward the foreground by `amount` in `0..=1`.
    fn shade(self, amount: f64) -> Rgb {
        let (back, front) = (self.background(), self.foreground());
        let mut mixed = [0; 3];
        for channel in 0..3 {
            let value = f64::from(back[channel])
                + (f64::from(front[channel]) - f64::from(back[channel])) * amount;
            mixed[channel] = channel_byte(value);
        }
        mixed
    }
}

/// A channel value in `0..=255`, rounded.
fn channel_byte(value: f64) -> u8 {
    let value = value.round().clamp(0.0, 255.0);
    // Clamped to the byte's range and rounded: the cast is exact.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 and rounded above"
    )]
    let value = value as u8;
    value
}

/// A title rasterized as coverage, one byte a device pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TitleMask {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) coverage: Vec<u8>,
}

/// An image in `wl_shm`'s ARGB8888: premultiplied, each pixel a
/// little-endian `u32`, so blue, green, red and alpha bytes in that order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pixels {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

impl Pixels {
    /// A transparent image of `width` by `height` device pixels; `None` for
    /// an empty one or one past [`MAX_EXTENT`].
    pub(crate) fn try_new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 || width > MAX_EXTENT || height > MAX_EXTENT {
            return None;
        }
        let bytes = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        Some(Self {
            width,
            height,
            data: vec![0; bytes],
        })
    }

    /// The pixel at (`x`, `y`) as straight `[r, g, b, a]`, for a test.
    #[cfg(test)]
    pub(crate) fn rgba(&self, x: u32, y: u32) -> [u8; 4] {
        let at = self.offset(x, y);
        let [blue, green, red, alpha] = [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ];
        if alpha == 0 {
            return [0, 0, 0, 0];
        }
        let straight = |channel: u8| channel_byte(f64::from(channel) * 255.0 / f64::from(alpha));
        [straight(red), straight(green), straight(blue), alpha]
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        let index = u64::from(y) * u64::from(self.width) + u64::from(x);
        usize::try_from(index * 4).unwrap_or(usize::MAX)
    }

    /// Lay `color` over the pixel at (`x`, `y`) with `coverage` in `0..=1`.
    fn blend(&mut self, x: u32, y: u32, color: Rgb, coverage: f64) {
        if coverage <= 0.0 || x >= self.width || y >= self.height {
            return;
        }
        let coverage = coverage.min(1.0);
        let at = self.offset(x, y);
        let Some(pixel) = self.data.get_mut(at..at + 4) else {
            return;
        };
        let keep = 1.0 - coverage;
        // Premultiplied: the source's channels are its colour times its
        // coverage, in the stored order blue, green, red.
        let source = [color[2], color[1], color[0]];
        for (stored, channel) in pixel.iter_mut().zip(source) {
            *stored = channel_byte(f64::from(channel) * coverage + f64::from(*stored) * keep);
        }
        pixel[3] = channel_byte(255.0 * coverage + f64::from(pixel[3]) * keep);
    }

    /// Scale every pixel at (`x`, `y`) by `coverage`: what a mask over an
    /// already drawn area keeps.
    fn mask(&mut self, x: u32, y: u32, coverage: f64) {
        let at = self.offset(x, y);
        if let Some(pixel) = self.data.get_mut(at..at + 4) {
            for stored in pixel {
                *stored = channel_byte(f64::from(*stored) * coverage.clamp(0.0, 1.0));
            }
        }
    }

    /// Fill the whole image with `color`.
    fn fill(&mut self, color: Rgb) {
        for pixel in self.data.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[color[2], color[1], color[0], 255]);
        }
    }

    /// Lay `color` over the pixels whose centres lie within `distance` of
    /// the shape `distance_to` measures, antialiased over a pixel, within
    /// `bounds` (left, top, right, bottom in device pixels).
    fn cover(
        &mut self,
        bounds: (f64, f64, f64, f64),
        color: Rgb,
        coverage_at: impl Fn(f64, f64) -> f64,
    ) {
        let first = |value: f64| channel_index(value.floor());
        let last = |value: f64, limit: u32| channel_index(value.ceil()).min(limit);
        let (x0, y0) = (first(bounds.0), first(bounds.1));
        let (x1, y1) = (last(bounds.2, self.width), last(bounds.3, self.height));
        for y in y0..y1 {
            for x in x0..x1 {
                let coverage = coverage_at(f64::from(x) + 0.5, f64::from(y) + 0.5);
                self.blend(x, y, color, coverage);
            }
        }
    }
}

/// A device coordinate as a pixel index: negative and non-finite values are
/// zero, and the largest is [`MAX_EXTENT`].
fn channel_index(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let value = value.min(f64::from(MAX_EXTENT));
    // Clamped to 0..=MAX_EXTENT and whole: the cast is exact.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=MAX_EXTENT and whole above"
    )]
    let value = value as u32;
    value
}

/// How much of the pixel centred at a point a shape `distance` away covers,
/// with its edge `half` from its middle: one pixel's antialiasing.
fn edge_coverage(distance: f64, half: f64) -> f64 {
    (half - distance + 0.5).clamp(0.0, 1.0)
}

/// The distance from (`px`, `py`) to the segment from `a` to `b`.
fn segment_distance(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.mul_add(dx, dy * dy);
    let t = if length > 0.0 {
        ((px - a.0).mul_add(dx, (py - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (t.mul_add(dx, a.0), t.mul_add(dy, a.1));
    (px - cx).hypot(py - cy)
}

/// A device extent for `logical` at `scale`: rounded, at least one pixel;
/// `None` for one that is no size or past [`MAX_EXTENT`].
pub(crate) fn device_extent(logical: f64, scale: f64) -> Option<u32> {
    let device = (logical * scale).round();
    (device.is_finite() && device >= 1.0 && device <= f64::from(MAX_EXTENT))
        .then(|| channel_index(device))
}

/// What a title bar is drawn from, beside its frame: the look, the button
/// hovered and the one held, and the title.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeaderLook<'a> {
    pub(crate) look: Look,
    pub(crate) hovered: Option<Button>,
    pub(crate) held: Option<Button>,
    pub(crate) title: Option<&'a TitleMask>,
}

/// The title bar of `frame` at `scale`: its background (rounded at the top
/// corners of a floating window), a line along its foot, its buttons with
/// their icons, and the title centred between them. `None` where the bar is
/// not shown or its extent is no size or past [`MAX_EXTENT`].
pub(crate) fn paint_header(frame: &Frame, scale: f64, look: HeaderLook<'_>) -> Option<Pixels> {
    let rect = frame.rect(Piece::Header)?;
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let mut pixels = Pixels::try_new(
        device_extent(rect.width, scale)?,
        device_extent(rect.height, scale)?,
    )?;
    let colors = look.look;
    pixels.fill(colors.background());
    let (width, height) = (f64::from(pixels.width), f64::from(pixels.height));
    // A line one device pixel tall along the foot.
    let line = colors.shade(0.12);
    pixels.cover((0.0, height - 1.0, width, height), line, |_, _| 1.0);
    for button in frame.buttons() {
        let Some(area) = frame.button_rect(button) else {
            continue;
        };
        let (cx, cy) = (
            (area.x + area.width / 2.0) * scale,
            (area.y + area.height / 2.0) * scale,
        );
        let radius = area.width / 2.0 * scale;
        let amount = if look.held == Some(button) {
            0.26
        } else if look.hovered == Some(button) {
            0.17
        } else {
            0.09
        };
        let bounds = (
            cx - radius - 1.0,
            cy - radius - 1.0,
            cx + radius + 1.0,
            cy + radius + 1.0,
        );
        pixels.cover(bounds, colors.shade(amount), |x, y| {
            edge_coverage((x - cx).hypot(y - cy), radius)
        });
        paint_icon(
            &mut pixels,
            button,
            frame.states.maximized,
            (cx, cy),
            scale,
            colors.foreground(),
        );
    }
    if let Some(title) = look.title {
        let room = device_extent(frame.title_room(), scale).unwrap_or(0);
        let shown = title.width.min(room);
        if shown > 0 {
            let left = (pixels.width.saturating_sub(shown)) / 2;
            let top = pixels.height.saturating_sub(title.height) / 2;
            let color = colors.foreground();
            for row in 0..title.height {
                for column in 0..shown {
                    let index = u64::from(row) * u64::from(title.width) + u64::from(column);
                    let coverage = usize::try_from(index)
                        .ok()
                        .and_then(|index| title.coverage.get(index))
                        .copied()
                        .unwrap_or(0);
                    pixels.blend(left + column, top + row, color, f64::from(coverage) / 255.0);
                }
            }
        }
    }
    let rounded = !frame.states.maximized && !frame.states.tiled.any();
    if rounded {
        round_top_corners(&mut pixels, RADIUS * scale);
    }
    Some(pixels)
}

/// Cut the top corners of `pixels` round at `radius` device pixels.
fn round_top_corners(pixels: &mut Pixels, radius: f64) {
    let width = f64::from(pixels.width);
    let reach = channel_index(radius.ceil()).min(pixels.height);
    for y in 0..reach {
        for x in 0..reach.min(pixels.width) {
            let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let distance = (radius - px).hypot(radius - py);
            let inside = (px >= radius) || (py >= radius);
            let coverage = if inside {
                1.0
            } else {
                edge_coverage(distance, radius)
            };
            pixels.mask(x, y, coverage);
            let mirrored = pixels.width - 1 - x;
            let mirrored_px = f64::from(mirrored) + 0.5;
            let distance = (mirrored_px - (width - radius)).hypot(radius - py);
            let inside = (mirrored_px <= width - radius) || (py >= radius);
            let coverage = if inside {
                1.0
            } else {
                edge_coverage(distance, radius)
            };
            pixels.mask(mirrored, y, coverage);
        }
    }
}

/// Draw `button`'s icon centred on `centre` (device pixels): a cross for
/// close, a square for maximize (two for restore), a bar for minimize.
fn paint_icon(
    pixels: &mut Pixels,
    button: Button,
    maximized: bool,
    centre: (f64, f64),
    scale: f64,
    color: Rgb,
) {
    let (cx, cy) = centre;
    let arm = 4.0 * scale;
    let half = (0.6 * scale).max(0.5);
    let segments: Vec<((f64, f64), (f64, f64))> = match button {
        Button::Close => vec![
            ((cx - arm, cy - arm), (cx + arm, cy + arm)),
            ((cx - arm, cy + arm), (cx + arm, cy - arm)),
        ],
        Button::Minimize => vec![((cx - arm, cy + arm * 0.75), (cx + arm, cy + arm * 0.75))],
        Button::Maximize if maximized => {
            let small = arm * 0.75;
            let offset = arm * 0.4;
            let (x, y) = (cx - offset, cy + offset);
            let mut lines = square((x, y), small);
            // The square behind shows its top and right sides only.
            let (bx, by) = (cx + offset, cy - offset);
            lines.push(((bx - small, by - small), (bx + small, by - small)));
            lines.push(((bx + small, by - small), (bx + small, by + small)));
            lines
        }
        Button::Maximize => square((cx, cy), arm),
    };
    for (a, b) in segments {
        let bounds = (
            a.0.min(b.0) - half - 1.0,
            a.1.min(b.1) - half - 1.0,
            a.0.max(b.0) + half + 1.0,
            a.1.max(b.1) + half + 1.0,
        );
        pixels.cover(bounds, color, |x, y| {
            edge_coverage(segment_distance(x, y, a, b), half)
        });
    }
}

/// The four sides of the square of half-side `half` centred on `centre`.
fn square(centre: (f64, f64), half: f64) -> Vec<((f64, f64), (f64, f64))> {
    let (cx, cy) = centre;
    let (l, r, t, b) = (cx - half, cx + half, cy - half, cy + half);
    vec![
        ((l, t), (r, t)),
        ((r, t), (r, b)),
        ((r, b), (l, b)),
        ((l, b), (l, t)),
    ]
}

/// A margin's image: transparent, so the window shows no frame there while
/// the margin still takes the pointer.
pub(crate) fn paint_margin(width: u32, height: u32) -> Option<Pixels> {
    Pixels::try_new(width, height)
}

pub(crate) mod title {
    //! The title, shaped and rasterized by FLUI's text stack: Parley shapes
    //! it over a font collection, swash rasterizes its glyphs, as the
    //! renderer's own text is drawn.

    use flui_painting::glyphs::SwashRasterizer;
    use flui_painting::parley_text::ParagraphSpec;
    use flui_painting::text_layout::{FontCollection, TextContext};
    use flui_painting::typography::{FontWeight, TextAlign, TextDirection, TextStyle};
    use flui_painting::{GlyphContent, GlyphRasterizer};

    use super::{MAX_EXTENT, TITLE_SIZE, TitleMask, device_extent};

    /// What a title is shaped and rasterized with, kept between titles.
    pub(crate) struct TitleText {
        context: TextContext,
        rasterizer: SwashRasterizer,
    }

    impl std::fmt::Debug for TitleText {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TitleText").finish_non_exhaustive()
        }
    }

    impl TitleText {
        /// Titles shaped over `fonts`.
        pub(crate) fn new(fonts: &FontCollection) -> Self {
            Self {
                context: TextContext::new(fonts),
                rasterizer: SwashRasterizer::new(),
            }
        }

        /// The generation of the fonts the titles are shaped over, which
        /// rises as faces are added.
        pub(crate) fn generation(&self) -> u64 {
            self.context.fonts().generation()
        }

        /// `text` on one line at most `room` logical pixels wide, cut with an
        /// ellipsis where it is wider, rasterized at `scale`: its coverage.
        /// `None` for an empty title or one that cannot be shaped.
        pub(crate) fn mask(&mut self, text: &str, room: f64, scale: f64) -> Option<TitleMask> {
            if text.is_empty() || !(room.is_finite() && room > 0.0) {
                return None;
            }
            let style = TextStyle::default()
                .with_font_size(TITLE_SIZE)
                .with_font_weight(FontWeight::W600);
            let spans = vec![(text.to_owned(), Some(style))];
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a title bar's logical sizes are far inside f32"
            )]
            let (font_size, room_f32, scale_f32) = (TITLE_SIZE as f32, room as f32, scale as f32);
            let layout = self
                .context
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size,
                    font_weight_adjustment: 0,
                    max_width: Some(room_f32),
                    min_width: 0.0,
                    text_align: TextAlign::Start,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: Some(1),
                    ellipsis: Some("\u{2026}"),
                })
                .ok()?;
            let metrics = layout.metrics();
            let width = device_extent(metrics.max_line_width.min(room), scale)?;
            let height = device_extent(metrics.height, scale)?;
            let bytes = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
            let mut coverage = vec![0_u8; bytes];
            let shaped = layout.to_shaped(None);
            for run in shaped.runs() {
                let Ok(key) = self.rasterizer.fonts_mut().prepare_run(&run) else {
                    continue;
                };
                for glyph in run.placed_glyphs(key, (0.0, 0.0), scale_f32) {
                    let Some(image) = self.rasterizer.rasterize(glyph.key) else {
                        continue;
                    };
                    let texel = image.content().bytes_per_texel();
                    let left = i64::from(glyph.x) + i64::from(image.left());
                    let top = i64::from(glyph.y) - i64::from(image.top());
                    for row in 0..image.height() {
                        for column in 0..image.width() {
                            let (x, y) = (left + i64::from(column), top + i64::from(row));
                            if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
                                continue;
                            }
                            let index = (u64::from(row) * u64::from(image.width())
                                + u64::from(column))
                                * u64::from(texel);
                            // Coverage is the mask's byte, or a colour
                            // glyph's alpha: the bar draws the title in one
                            // colour.
                            let offset = match image.content() {
                                GlyphContent::Mask => 0,
                                GlyphContent::Color => 3,
                            };
                            let Some(&value) = usize::try_from(index + offset)
                                .ok()
                                .and_then(|index| image.data().get(index))
                            else {
                                continue;
                            };
                            #[expect(
                                clippy::cast_sign_loss,
                                reason = "x and y are checked within 0..MAX_EXTENT above"
                            )]
                            let at = (y as u64) * u64::from(width) + (x as u64);
                            if let Some(cell) =
                                usize::try_from(at).ok().and_then(|at| coverage.get_mut(at))
                            {
                                *cell = (*cell).max(value);
                            }
                        }
                    }
                }
            }
            debug_assert!(width <= MAX_EXTENT && height <= MAX_EXTENT);
            Some(TitleMask {
                width,
                height,
                coverage,
            })
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) mod surfaces {
    //! The frame on the compositor: a subsurface of the content's surface
    //! for each piece, drawn into shared memory on the owner thread, and the
    //! record the seats' input tells what the pointer does over them.
    //!
    //! Each subsurface is synchronized with the content's surface (the
    //! default), so a piece drawn for a new size shows with the commit that
    //! gives the content that size, never before it. A piece is drawn again
    //! only when what it shows changed; a buffer goes back to the compositor
    //! with each draw and is destroyed when the compositor releases it, so
    //! one turn draws each piece at most once.

    use std::os::fd::AsFd;
    use std::os::unix::fs::FileExt;
    use std::sync::{Arc, OnceLock, Weak};

    use cursor_icon::CursorIcon;
    use flui_painting::text_layout::FontCollection;
    use parking_lot::Mutex;
    use wayland_client::protocol::wl_buffer::{self, WlBuffer};
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::protocol::wl_shm::{self, WlShm};
    use wayland_client::protocol::wl_shm_pool::{self, WlShmPool};
    use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
    use wayland_client::protocol::wl_subsurface::{self, WlSubsurface};
    use wayland_client::protocol::wl_surface::{self, WlSurface};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
    use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

    use super::title::TitleText;
    use super::{
        Action, Button, Frame, HeaderLook, Look, Part, Piece, PointerOnFrame, Rect, TitleMask,
        device_extent, paint_header, paint_margin, rounded,
    };
    use crate::executor::BackgroundExecutor;
    use crate::owner::linux::wayland::{FramePointer, LoopWaker, WaylandState};

    /// The fonts the host's windows draw their titles with, made the first
    /// time a window draws its own frame: the bundled faces at once, and the
    /// host's installed faces once a background scan added them, which wakes
    /// the loop so the titles are drawn again with them. Shared by every
    /// window of a host.
    #[derive(Clone, Default)]
    pub(crate) struct TitleFonts(Arc<OnceLock<FontCollection>>);

    impl std::fmt::Debug for TitleFonts {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TitleFonts").finish_non_exhaustive()
        }
    }

    impl TitleFonts {
        /// The collection, made and fed on `executor` the first time, with
        /// `waker` woken once the host's faces are in.
        pub(crate) fn get(
            &self,
            executor: &BackgroundExecutor,
            waker: &Weak<LoopWaker>,
        ) -> FontCollection {
            self.0
                .get_or_init(|| {
                    let (fonts, feed) = FontCollection::with_host_feed();
                    let waker = waker.clone();
                    // Detached: the feed adds to a collection it shares, and
                    // ends on its own.
                    drop(executor.handle().spawn_blocking(move || {
                        feed.run();
                        if let Some(waker) = waker.upgrade() {
                            waker.wake();
                        }
                    }));
                    fonts
                })
                .clone()
        }
    }

    /// A move, a resize or a window menu the frame asked for, with the
    /// seat and serial of the press that asked.
    #[derive(Debug, Clone)]
    pub(crate) struct Asked {
        pub(crate) action: Action,
        pub(crate) seat: WlSeat,
        pub(crate) serial: u32,
    }

    /// What the pointer did over a frame, recorded by the seats' input and
    /// carried out by the window as it settles. One action at most waits: a
    /// later press replaces it.
    #[derive(Default)]
    struct Pointing {
        /// The frame's surfaces and the piece each is.
        pieces: Vec<(WlSurface, Piece)>,
        /// The frame as last laid out; `None` while it is not shown.
        frame: Option<Frame>,
        pointer: PointerOnFrame,
        asked: Option<Asked>,
        /// The moves, resizes and menus carried out, with the serial each
        /// named, for a test.
        #[cfg(any(test, feature = "testing"))]
        carried: Vec<(Action, u32)>,
    }

    /// A window's frame as the seats' input reaches it. See [`Pointing`].
    #[derive(Default)]
    pub(crate) struct FrameRecord(Mutex<Pointing>);

    impl FrameRecord {
        /// The action the pointer asked for since the last look.
        pub(crate) fn take_asked(&self) -> Option<Asked> {
            self.0.lock().asked.take()
        }

        /// Record that the window carried out `action` naming `serial`, for
        /// a test.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn carried(&self, action: Action, serial: u32) {
            let mut pointing = self.0.lock();
            if pointing.carried.len() < 64 {
                pointing.carried.push((action, serial));
            }
        }

        /// The moves, resizes and menus carried out, with their serials.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn carried_out(&self) -> Vec<(Action, u32)> {
            self.0.lock().carried.clone()
        }

        /// The button hovered and the one held.
        fn buttons(&self) -> (Option<Button>, Option<Button>) {
            let pointing = self.0.lock();
            let hovered = match pointing.pointer.hovered() {
                Some(Part::Button(button)) => Some(button),
                _ => None,
            };
            (hovered, pointing.pointer.held())
        }
    }

    impl FramePointer for FrameRecord {
        fn holds(&self, surface: &WlSurface) -> bool {
            self.0.lock().pieces.iter().any(|(held, _)| held == surface)
        }

        fn moved(&self, surface: &WlSurface, x: f64, y: f64) -> CursorIcon {
            let mut pointing = self.0.lock();
            let piece = pointing
                .pieces
                .iter()
                .find(|(held, _)| held == surface)
                .map(|(_, piece)| *piece);
            let (Some(piece), Some(frame)) = (piece, pointing.frame) else {
                return CursorIcon::Default;
            };
            pointing.pointer.moved(&frame, piece, x, y)
        }

        fn left(&self) {
            self.0.lock().pointer.left();
        }

        fn button(&self, seat: &WlSeat, serial: u32, time: u32, code: u32, pressed: bool) {
            let mut pointing = self.0.lock();
            let Some(frame) = pointing.frame else {
                return;
            };
            if let Some(action) = pointing.pointer.button(&frame, code, pressed, time) {
                pointing.asked = Some(Asked {
                    action,
                    seat: seat.clone(),
                    serial,
                });
            }
        }
    }

    /// The globals a frame is made with.
    #[derive(Clone)]
    pub(crate) struct FrameGlobals {
        pub(crate) compositor: WlCompositor,
        pub(crate) subcompositor: WlSubcompositor,
        pub(crate) shm: WlShm,
        pub(crate) viewporter: Option<WpViewporter>,
        pub(crate) queue: QueueHandle<WaylandState>,
    }

    /// What a piece shows: drawn again only when this changes.
    #[derive(Debug, Clone, PartialEq)]
    struct Shown {
        rect: Rect,
        scale: f64,
        /// The title bar's look; `None` for a margin.
        header: Option<HeaderShown>,
    }

    /// What the title bar shows beside its rectangle.
    #[derive(Debug, Clone, PartialEq)]
    struct HeaderShown {
        frame: Frame,
        look: Look,
        hovered: Option<Button>,
        held: Option<Button>,
        title: String,
        fonts: u64,
    }

    /// One piece's surface.
    struct PieceSurface {
        piece: Piece,
        surface: WlSurface,
        subsurface: WlSubsurface,
        viewport: Option<WpViewport>,
        shown: Option<Shown>,
    }

    /// A frame piece's surface: the content's surface names the outputs, so
    /// what a piece's says is not read.
    pub(crate) struct FramePiece;

    /// A buffer a frame piece was drawn into, destroyed once the compositor
    /// releases it.
    pub(crate) struct FrameBuffer;

    /// The title last rasterized, kept while what it was drawn for holds.
    struct TitleKept {
        text: String,
        room: f64,
        scale: f64,
        fonts: u64,
        mask: Option<TitleMask>,
    }

    /// A window's own frame on the compositor. See the module docs.
    pub(crate) struct ClientFrame {
        pieces: Vec<PieceSurface>,
        record: Arc<FrameRecord>,
        globals: FrameGlobals,
        text: TitleText,
        title: Option<TitleKept>,
    }

    impl std::fmt::Debug for ClientFrame {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ClientFrame").finish_non_exhaustive()
        }
    }

    impl ClientFrame {
        /// The frame's surfaces as subsurfaces of `parent`, the content's,
        /// reporting to `record`; nothing is shown until [`Self::update`].
        pub(crate) fn new(
            parent: &WlSurface,
            globals: FrameGlobals,
            record: Arc<FrameRecord>,
            fonts: &FontCollection,
        ) -> Self {
            let queue = &globals.queue;
            let pieces: Vec<PieceSurface> = Piece::ALL
                .into_iter()
                .map(|piece| {
                    let surface = globals.compositor.create_surface(queue, FramePiece);
                    let subsurface =
                        globals
                            .subcompositor
                            .get_subsurface(&surface, parent, queue, ());
                    let viewport = globals
                        .viewporter
                        .as_ref()
                        .map(|viewporter| viewporter.get_viewport(&surface, queue, ()));
                    PieceSurface {
                        piece,
                        surface,
                        subsurface,
                        viewport,
                        shown: None,
                    }
                })
                .collect();
            record.0.lock().pieces = pieces
                .iter()
                .map(|piece| (piece.surface.clone(), piece.piece))
                .collect();
            Self {
                pieces,
                record,
                text: TitleText::new(fonts),
                globals,
                title: None,
            }
        }

        /// Show `frame` (`None`: none) at `scale`, with `look` and `title`:
        /// draw each piece whose image changed, hide each no longer shown.
        /// Returns whether any piece was committed, which shows with the
        /// content surface's next commit.
        pub(crate) fn update(
            &mut self,
            frame: Option<Frame>,
            scale: f64,
            look: Look,
            title: &str,
        ) -> bool {
            self.record.0.lock().frame = frame;
            let (hovered, held) = self.record.buttons();
            // Without a viewport a buffer is the surface's size: drawn at
            // scale one, which the compositor scales as it does the content.
            let scale = if self.globals.viewporter.is_some() && scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            let fonts = self.text.generation();
            let mut committed = false;
            for index in 0..self.pieces.len() {
                let piece = self.pieces[index].piece;
                let shown = frame.and_then(|frame| {
                    let rect = frame.rect(piece)?;
                    Some(Shown {
                        rect,
                        scale,
                        header: (piece == Piece::Header).then(|| HeaderShown {
                            frame,
                            look,
                            hovered,
                            held,
                            title: title.to_owned(),
                            fonts,
                        }),
                    })
                });
                if self.pieces[index].shown == shown {
                    continue;
                }
                let drawn = shown.as_ref().and_then(|shown| self.draw(index, shown));
                let part = &mut self.pieces[index];
                if drawn.is_none() && part.shown.is_some() {
                    part.surface.attach(None, 0, 0);
                    part.surface.commit();
                }
                part.shown = drawn.and(shown);
                committed = true;
            }
            committed
        }

        /// Draw piece `index` as `shown` and commit it; `None` where it
        /// could not be drawn (no size, past the largest extent, no shared
        /// memory).
        fn draw(&mut self, index: usize, shown: &Shown) -> Option<()> {
            let rect = shown.rect;
            let pixels = if let Some(header) = &shown.header {
                let room = header.frame.title_room();
                let mask = self.title_mask(&header.title, room, shown.scale);
                paint_header(
                    &header.frame,
                    shown.scale,
                    HeaderLook {
                        look: header.look,
                        hovered: header.hovered,
                        held: header.held,
                        title: mask,
                    },
                )?
            } else if self.globals.viewporter.is_some() {
                // A transparent pixel the viewport stretches over the margin.
                paint_margin(1, 1)?
            } else {
                paint_margin(
                    device_extent(rect.width, 1.0)?,
                    device_extent(rect.height, 1.0)?,
                )?
            };
            let buffer = shm_buffer(&self.globals.shm, &self.globals.queue, &pixels)?;
            let part = &self.pieces[index];
            part.subsurface
                .set_position(rounded(rect.x), rounded(rect.y));
            if let Some(viewport) = &part.viewport {
                viewport.set_destination(rounded(rect.width).max(1), rounded(rect.height).max(1));
            }
            part.surface.attach(Some(&buffer), 0, 0);
            part.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
            part.surface.commit();
            Some(())
        }

        /// The title's coverage for `text` at most `room` wide at `scale`,
        /// rasterized again only when one of them or the fonts changed.
        fn title_mask(&mut self, text: &str, room: f64, scale: f64) -> Option<&TitleMask> {
            let fonts = self.text.generation();
            let kept = self.title.as_ref().is_some_and(|kept| {
                kept.text == text && kept.room == room && kept.scale == scale && kept.fonts == fonts
            });
            if !kept {
                let mask = self.text.mask(text, room, scale);
                self.title = Some(TitleKept {
                    text: text.to_owned(),
                    room,
                    scale,
                    fonts,
                    mask,
                });
            }
            self.title.as_ref().and_then(|kept| kept.mask.as_ref())
        }

        /// What each piece shows now, as (piece, rectangle), for a test.
        #[cfg(any(test, feature = "testing"))]
        pub(crate) fn shown(&self) -> Vec<(Piece, Rect)> {
            self.pieces
                .iter()
                .filter_map(|part| part.shown.as_ref().map(|shown| (part.piece, shown.rect)))
                .collect()
        }

        /// Destroy the frame's objects; the input reaches none of its
        /// surfaces from here.
        pub(crate) fn destroy(self) {
            {
                let mut pointing = self.record.0.lock();
                pointing.pieces.clear();
                pointing.frame = None;
                pointing.asked = None;
            }
            for part in self.pieces {
                if let Some(viewport) = part.viewport {
                    viewport.destroy();
                }
                part.subsurface.destroy();
                part.surface.destroy();
            }
        }
    }

    /// `pixels` in a `wl_shm` buffer of its own: a memory file written with
    /// the image, shared with the compositor through a pool made for it.
    fn shm_buffer(
        shm: &WlShm,
        queue: &QueueHandle<WaylandState>,
        pixels: &super::Pixels,
    ) -> Option<WlBuffer> {
        let width = i32::try_from(pixels.width).ok()?;
        let height = i32::try_from(pixels.height).ok()?;
        let stride = width.checked_mul(4)?;
        let size = i32::try_from(pixels.data.len()).ok()?;
        let memory =
            rustix::fs::memfd_create("flui-window-frame", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
        let file = std::fs::File::from(memory);
        file.write_all_at(&pixels.data, 0).ok()?;
        let pool = shm.create_pool(file.as_fd(), size, queue, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            stride,
            wl_shm::Format::Argb8888,
            queue,
            FrameBuffer,
        );
        pool.destroy();
        Some(buffer)
    }

    impl Dispatch<WlBuffer, FrameBuffer> for WaylandState {
        fn event(
            _: &mut Self,
            buffer: &WlBuffer,
            event: wl_buffer::Event,
            _: &FrameBuffer,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_buffer::Event::Release = event {
                buffer.destroy();
            }
        }
    }

    impl Dispatch<WlSurface, FramePiece> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlSurface,
            _: wl_surface::Event,
            _: &FramePiece,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The content's surface names the outputs and the scale.
        }
    }

    impl Dispatch<WlSubsurface, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlSubsurface,
            _: wl_subsurface::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A subsurface has no events.
        }
    }

    impl Dispatch<WlShmPool, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlShmPool,
            _: wl_shm_pool::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A pool has no events.
        }
    }
}

#[cfg(test)]
mod tests;

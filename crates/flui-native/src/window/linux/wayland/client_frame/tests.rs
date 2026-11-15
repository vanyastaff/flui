use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_painting::text_layout::FontCollection;

use super::title::TitleText;
use super::*;
use crate::window::linux::wayland::configure::{TiledEdges, ToplevelStates, WmCapabilities};

const EVERYTHING: Offers = Offers {
    minimize: true,
    maximize: true,
    window_menu: true,
};

fn framed(states: ToplevelStates) -> Frame {
    Frame {
        content: Size::new(400.0, 300.0),
        states,
        resizable: true,
        offers: EVERYTHING,
        title_bar: true,
    }
}

fn floating() -> Frame {
    framed(ToplevelStates::default())
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A floating window's frame is a title bar above its content and four
/// margins outside both; its geometry is the bar and the content, and a
/// configure's height loses the bar to the content.
#[test]
fn a_frame_lies_around_its_content() {
    let frame = floating();
    assert_eq!(
        frame.geometry(),
        Rect {
            x: 0.0,
            y: -HEADER,
            width: 400.0,
            height: 300.0 + HEADER,
        }
    );
    assert_eq!(
        frame.rect(Piece::Header),
        Some(rect(0.0, -HEADER, 400.0, HEADER))
    );
    assert_eq!(
        frame.rect(Piece::Top),
        Some(rect(
            -MARGIN,
            -HEADER - MARGIN,
            400.0 + 2.0 * MARGIN,
            MARGIN
        ))
    );
    assert_eq!(
        frame.rect(Piece::Bottom),
        Some(rect(-MARGIN, 300.0, 400.0 + 2.0 * MARGIN, MARGIN))
    );
    assert_eq!(
        frame.rect(Piece::Left),
        Some(rect(-MARGIN, -HEADER, MARGIN, HEADER + 300.0))
    );
    assert_eq!(
        frame.rect(Piece::Right),
        Some(rect(400.0, -HEADER, MARGIN, HEADER + 300.0))
    );
    assert_eq!(frame.content_of(400, 337), (400, 300));
    assert_eq!(frame.content_of(0, 0), (0, 0), "an axis left to the window");
    assert_eq!(frame.content_of(400, 20), (400, 1), "never below a pixel");
}

/// The frame goes where it has no place: all of it in fullscreen, the
/// margins of a maximized window and of one that cannot be resized, and the
/// margin of each tiled edge, which the top and bottom margins no longer
/// reach past.
#[test]
fn the_frame_goes_away_in_fullscreen_and_at_tiled_edges() {
    let fullscreen = framed(ToplevelStates {
        fullscreen: true,
        ..ToplevelStates::default()
    });
    for piece in Piece::ALL {
        assert_eq!(fullscreen.rect(piece), None, "{piece:?} in fullscreen");
    }
    assert_eq!(
        fullscreen.geometry(),
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        }
    );
    assert_eq!(fullscreen.content_of(1920, 1080), (1920, 1080));

    let maximized = framed(ToplevelStates {
        maximized: true,
        ..ToplevelStates::default()
    });
    assert!(maximized.rect(Piece::Header).is_some());
    for piece in [Piece::Top, Piece::Bottom, Piece::Left, Piece::Right] {
        assert_eq!(
            maximized.rect(piece),
            None,
            "{piece:?} of a maximized window"
        );
    }
    assert_eq!(maximized.content_of(1920, 1080), (1920, 1043));

    let fixed = Frame {
        resizable: false,
        ..floating()
    };
    assert_eq!(fixed.edges(), TiledEdges::NONE);

    let tiled = framed(ToplevelStates {
        tiled: TiledEdges {
            left: true,
            top: true,
            ..TiledEdges::NONE
        },
        ..ToplevelStates::default()
    });
    assert_eq!(tiled.rect(Piece::Left), None);
    assert_eq!(tiled.rect(Piece::Top), None);
    assert!(tiled.rect(Piece::Header).is_some(), "the title bar stays");
    assert_eq!(
        tiled.rect(Piece::Bottom),
        Some(rect(0.0, 300.0, 400.0 + MARGIN, MARGIN)),
        "the bottom margin keeps clear of the tiled left edge"
    );
}

/// The title bar shows close always, maximize where the compositor
/// maximizes and the window may be resized, minimize where it minimizes; a
/// compositor that announced nothing is taken to offer all.
#[test]
fn a_title_bar_shows_the_buttons_its_compositor_offers() {
    let all: Vec<Button> = floating().buttons().collect();
    assert_eq!(all, [Button::Close, Button::Maximize, Button::Minimize]);
    assert_eq!(Offers::from(None), EVERYTHING);
    let announced = Offers::from(Some(WmCapabilities {
        window_menu: false,
        maximize: false,
        fullscreen: true,
        minimize: true,
    }));
    let only = Frame {
        offers: announced,
        ..floating()
    };
    assert_eq!(
        only.buttons().collect::<Vec<_>>(),
        [Button::Close, Button::Minimize]
    );
    let fixed = Frame {
        resizable: false,
        ..floating()
    };
    assert!(!fixed.buttons().any(|button| button == Button::Maximize));
    // The buttons sit at the bar's end, close last.
    let close = floating().button_rect(Button::Close).expect("shown");
    let maximize = floating().button_rect(Button::Maximize).expect("shown");
    assert!(close.x > maximize.x);
    assert!(close.x + close.width <= 400.0);
    assert_eq!(only.button_rect(Button::Maximize), None);
}

/// A point names the part under it: a button, the caption, or the edge or
/// corner of a margin, which loses a side the compositor tiled.
#[test]
fn a_point_on_the_frame_names_its_part() {
    let frame = floating();
    let close = frame.button_rect(Button::Close).expect("shown");
    assert_eq!(
        frame.part_at(Piece::Header, close.x + 12.0, close.y + 12.0),
        Some(Part::Button(Button::Close))
    );
    assert_eq!(
        frame.part_at(Piece::Header, 20.0, 18.0),
        Some(Part::Caption)
    );
    for (piece, x, y, edge) in [
        (Piece::Top, 200.0, 5.0, Edge::Top),
        (Piece::Top, 2.0, 5.0, Edge::TopLeft),
        (Piece::Top, 415.0, 5.0, Edge::TopRight),
        (Piece::Bottom, 200.0, 5.0, Edge::Bottom),
        (Piece::Bottom, 3.0, 5.0, Edge::BottomLeft),
        (Piece::Bottom, 418.0, 5.0, Edge::BottomRight),
        (Piece::Left, 5.0, 150.0, Edge::Left),
        (Piece::Left, 5.0, 2.0, Edge::TopLeft),
        (Piece::Left, 5.0, 330.0, Edge::BottomLeft),
        (Piece::Right, 5.0, 150.0, Edge::Right),
        (Piece::Right, 5.0, 2.0, Edge::TopRight),
        (Piece::Right, 5.0, 330.0, Edge::BottomRight),
    ] {
        assert_eq!(
            frame.part_at(piece, x, y),
            Some(Part::Edge(edge)),
            "{piece:?} at ({x}, {y})"
        );
    }
    assert_eq!(frame.part_at(Piece::Header, f64::NAN, 1.0), None);
    let tiled = ToplevelStates {
        tiled: TiledEdges {
            right: true,
            ..TiledEdges::NONE
        },
        ..ToplevelStates::default()
    };
    let right_tiled = framed(tiled);
    assert_eq!(
        right_tiled.part_at(Piece::Top, 405.0, 5.0),
        Some(Part::Edge(Edge::Top)),
        "a corner on a tiled side keeps its other side"
    );
    assert_eq!(right_tiled.part_at(Piece::Right, 5.0, 150.0), None);
    // xdg-shell's `resize_edge` numbers.
    for (edge, value) in [
        (Edge::Top, 1),
        (Edge::Bottom, 2),
        (Edge::Left, 4),
        (Edge::TopLeft, 5),
        (Edge::BottomLeft, 6),
        (Edge::Right, 8),
        (Edge::TopRight, 9),
        (Edge::BottomRight, 10),
    ] {
        assert_eq!(edge.protocol(), value, "{edge:?}");
    }
    assert_eq!(Edge::BottomRight.cursor(), CursorIcon::SeResize);
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

/// A press on the caption moves the window, a second within the double
/// press maximizes or restores it, a press on a margin resizes it, a button
/// acts on its release over it, and a secondary press shows the window menu
/// where the pointer is, relative to the content.
#[test]
fn the_pointer_on_a_frame_asks_for_what_it_pressed() {
    let frame = floating();
    let mut pointer = PointerOnFrame::default();

    assert_eq!(
        pointer.moved(&frame, Piece::Header, 20.0, 18.0),
        CursorIcon::Default
    );
    assert_eq!(pointer.hovered(), Some(Part::Caption));
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, true, 1_000),
        Some(Action::Move)
    );
    assert_eq!(pointer.button(&frame, BTN_LEFT, false, 1_050), None);
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, true, 1_200),
        Some(Action::Maximize),
        "a double press"
    );
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, true, 1_300),
        Some(Action::Move),
        "the double press is spent"
    );
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, true, 1_300 + DOUBLE_CLICK),
        Some(Action::Move),
        "too late for a double press"
    );
    let maximized = Frame {
        states: ToplevelStates {
            maximized: true,
            ..ToplevelStates::default()
        },
        ..frame
    };
    let mut wrapped = PointerOnFrame::default();
    wrapped.moved(&maximized, Piece::Header, 20.0, 18.0);
    assert_eq!(
        wrapped.button(&maximized, BTN_LEFT, true, u32::MAX - 10),
        Some(Action::Move)
    );
    assert_eq!(
        wrapped.button(&maximized, BTN_LEFT, true, 20),
        Some(Action::Restore),
        "across the millisecond counter's wrap"
    );

    assert_eq!(
        pointer.moved(&frame, Piece::Bottom, 418.0, 5.0),
        CursorIcon::SeResize
    );
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, true, 5_000),
        Some(Action::Resize(Edge::BottomRight))
    );

    let close = frame.button_rect(Button::Close).expect("shown");
    pointer.moved(&frame, Piece::Header, close.x + 12.0, close.y + 12.0);
    assert_eq!(pointer.button(&frame, BTN_LEFT, true, 6_000), None);
    assert_eq!(pointer.held(), Some(Button::Close));
    assert_eq!(
        pointer.button(&frame, BTN_LEFT, false, 6_100),
        Some(Action::Close)
    );
    // Released elsewhere, the button does nothing.
    pointer.button(&frame, BTN_LEFT, true, 7_000);
    pointer.moved(&frame, Piece::Header, 20.0, 18.0);
    assert_eq!(pointer.button(&frame, BTN_LEFT, false, 7_100), None);
    // A pointer that leaves lets go of the button.
    pointer.moved(&frame, Piece::Header, close.x + 12.0, close.y + 12.0);
    pointer.button(&frame, BTN_LEFT, true, 8_000);
    pointer.left();
    assert_eq!(pointer.held(), None);

    let mut menu = PointerOnFrame::default();
    menu.moved(&frame, Piece::Header, 120.0, 10.0);
    assert_eq!(
        menu.button(&frame, BTN_RIGHT, true, 9_000),
        Some(Action::Menu { x: 120, y: 10 - 37 }),
        "relative to the content's surface"
    );
    let no_menu = Frame {
        offers: Offers {
            window_menu: false,
            ..EVERYTHING
        },
        ..frame
    };
    assert_eq!(menu.button(&no_menu, BTN_RIGHT, true, 9_100), None);
    let mut maximize = PointerOnFrame::default();
    let button = maximized.button_rect(Button::Maximize).expect("shown");
    maximize.moved(&maximized, Piece::Header, button.x + 1.0, button.y + 1.0);
    maximize.button(&maximized, BTN_LEFT, true, 10_000);
    assert_eq!(
        maximize.button(&maximized, BTN_LEFT, false, 10_010),
        Some(Action::Restore)
    );
}

fn title_mask(width: u32, height: u32) -> TitleMask {
    TitleMask {
        width,
        height,
        coverage: vec![255; usize::try_from(width * height).expect("small")],
    }
}

const LIGHT: Look = Look {
    dark: false,
    active: true,
};

/// The title bar is its background with rounded top corners on a floating
/// window, square ones on a maximized one, its buttons darker where hovered,
/// their icons in the title's colour, and the title centred.
#[test]
fn a_title_bar_is_drawn_with_its_buttons_and_title() {
    let frame = floating();
    let plain = HeaderLook {
        look: LIGHT,
        hovered: None,
        held: None,
        title: None,
    };
    let pixels = paint_header(&frame, BufferScale::ONE, plain).expect("drawn");
    assert_eq!((pixels.width, pixels.height), (400, 37));
    let background = LIGHT.background();
    assert_eq!(
        pixels.rgba(60, 10),
        [background[0], background[1], background[2], 255]
    );
    assert_eq!(pixels.rgba(0, 0)[3], 0, "a rounded corner is transparent");
    assert_eq!(pixels.rgba(399, 0)[3], 0);
    let maximized = Frame {
        states: ToplevelStates {
            maximized: true,
            ..ToplevelStates::default()
        },
        ..frame
    };
    assert_eq!(
        paint_header(&maximized, BufferScale::ONE, plain)
            .expect("drawn")
            .rgba(0, 0)[3],
        255,
        "a maximized window's corners are square"
    );

    let close = frame.button_rect(Button::Close).expect("shown");
    // A point of the button's disc away from its icon.
    let (x, y) = (
        channel_index(close.x + 4.0),
        channel_index(close.y + close.height / 2.0),
    );
    let idle = pixels.rgba(x, y);
    let hovered = paint_header(
        &frame,
        BufferScale::ONE,
        HeaderLook {
            hovered: Some(Button::Close),
            ..plain
        },
    )
    .expect("drawn")
    .rgba(x, y);
    assert!(
        hovered[0] < idle[0],
        "hovered {hovered:?} over idle {idle:?}"
    );
    assert!(idle[0] < background[0], "the disc shows on the bar");
    // The cross passes through the button's centre.
    let centre = pixels.rgba(
        channel_index(close.x + close.width / 2.0),
        channel_index(close.y + close.height / 2.0),
    );
    assert!(
        centre[0] < idle[0],
        "the icon {centre:?} over the disc {idle:?}"
    );

    let mask = title_mask(40, 10);
    let titled = paint_header(
        &frame,
        BufferScale::ONE,
        HeaderLook {
            title: Some(&mask),
            ..plain
        },
    )
    .expect("drawn");
    let foreground = LIGHT.foreground();
    assert_eq!(
        titled.rgba(200, 18),
        [foreground[0], foreground[1], foreground[2], 255],
        "the title is centred"
    );
    assert_eq!(titled.rgba(150, 18), pixels.rgba(150, 18), "and no wider");

    let dark = paint_header(
        &frame,
        BufferScale::try_new(2.0).expect("a scale"),
        HeaderLook {
            look: Look {
                dark: true,
                active: false,
            },
            ..plain
        },
    )
    .expect("drawn");
    assert_eq!((dark.width, dark.height), (800, 74), "drawn at the scale");
    assert!(dark.rgba(120, 20)[0] < 64, "a dark bar");
}

const PLAIN: HeaderLook<'static> = HeaderLook {
    look: LIGHT,
    hovered: None,
    held: None,
    title: None,
};

/// A compositor names any scale it likes. At 442 the 37-pixel title bar of
/// a 37-pixel window would be 16 354 pixels a side, each within the largest
/// extent and a gibibyte in all, drawn again at every hover. The frame is
/// drawn at the scale the window's buffer is, which holds no scale past the
/// largest, and no image of that area is made whatever asks for it.
#[test]
fn a_hostile_scale_draws_no_gibibyte() {
    let narrow = Frame {
        content: Size::new(37.0, 300.0),
        ..floating()
    };
    let hostile = BufferScale::try_new(442.0).expect("a scale");
    let pixels = paint_header(&narrow, hostile, PLAIN).expect("drawn");
    assert_eq!(
        (pixels.width, pixels.height),
        (296, 296),
        "at the largest scale"
    );

    let side = device_extent(37.0, DevicePixelRatio::try_new(442.0).expect("a ratio"))
        .expect("within the largest extent");
    assert_eq!(side, 16_354);
    assert!(
        Pixels::try_new(side, side).is_none(),
        "each side is within the largest extent, the area is not"
    );
    assert!(Pixels::try_new(MAX_EXTENT, 297).is_none());
    // The widest bar at the largest scale is the most a frame draws.
    assert_eq!(
        f64::from(MAX_PIXELS),
        f64::from(MAX_EXTENT) * HEADER * BufferScale::MAX
    );
    let widest = Pixels::try_new(MAX_EXTENT, 296).expect("within the area");
    assert_eq!(widest.data.len(), 4 * 16_384 * 296);
}

/// Every window that publishes metrics has its whole frame. A window 3 000
/// logical pixels wide at the largest scale would be 24 000 device pixels
/// across, past the largest surface: it is drawn at the scale that fits
/// its content, and the title bar, which spans it, and all four margins
/// (one pixel the viewport stretches) are drawn and take the pointer,
/// wide or tall, at the cap or past it in logical pixels. Without a
/// viewport, at scale one, a margin along a side at the cap is drawn within it.
#[test]
fn a_window_that_publishes_metrics_draws_its_whole_frame() {
    use crate::window::linux::wayland::scale::metrics;

    let largest = BufferScale::try_new(BufferScale::MAX).expect("a scale");
    let cap = f64::from(MAX_EXTENT);
    for (width, height) in [
        (cap / BufferScale::MAX, 300.0),
        (3_000.0, 300.0),
        (cap * 2.0, 300.0),
        (300.0, cap * 2.0),
        (cap, cap),
    ] {
        let wide = Frame {
            content: Size::new(width, height),
            ..floating()
        };
        let scale = largest.fitted(wide.content);
        let published = metrics(wide.content, scale).expect("the content is published");
        let pixels = paint_header(&wide, scale, PLAIN).expect("its title bar is drawn");
        assert_eq!(
            i32::try_from(pixels.width).expect("within i32"),
            published.physical().width,
            "{width} x {height}: the bar spans the content"
        );
        for piece in [Piece::Top, Piece::Bottom, Piece::Left, Piece::Right] {
            let rect = wide
                .rect(piece)
                .expect("a floating window has every margin");
            for viewported in [true, false] {
                assert!(
                    margin_size(rect, viewported).is_some(),
                    "{width} x {height}: {piece:?} drawn, viewported {viewported}"
                );
            }
            let (x, y) = (rect.width / 2.0, rect.height / 2.0);
            assert!(
                matches!(wide.part_at(piece, x, y), Some(Part::Edge(_))),
                "{width} x {height}: {piece:?} takes the pointer"
            );
        }
    }
}

/// A compositor that releases no buffer is lent no more than the set: each
/// draw destroys the oldest for the one it makes, and the frame's end
/// destroys the rest.
#[test]
fn a_compositor_that_releases_nothing_is_lent_a_bounded_set() {
    let mut buffers = BufferSet::default();
    let mut made = 0_usize;
    let mut destroyed = Vec::new();
    for draw in 1..=10_000_usize {
        let lent = buffers
            .lend(
                (400, 37),
                |_| {
                    made += 1;
                    Some(made)
                },
                |gone| destroyed.push(gone),
            )
            .copied();
        assert_eq!(
            lent,
            Some(draw),
            "nothing released, nothing drawn into again"
        );
        assert!(buffers.len() <= MAX_BUFFERS, "{} kept", buffers.len());
    }
    assert_eq!(
        destroyed,
        (1..=10_000 - MAX_BUFFERS).collect::<Vec<usize>>(),
        "the oldest first, the newest kept"
    );
    buffers.clear(|gone| destroyed.push(gone));
    assert_eq!(destroyed.len(), 10_000, "the frame's end destroys the rest");
    assert_eq!(buffers.len(), 0);
}

/// A buffer the compositor released is drawn into again, so a frame that is
/// redrawn makes no memory after its second buffer; one of another size is
/// destroyed for the size drawn now; and where no memory can be had nothing
/// is lent.
#[test]
fn a_released_buffer_is_drawn_into_again() {
    use std::cell::RefCell;

    let marks: RefCell<Vec<Lent>> = RefCell::default();
    let make = |lent: &Lent| {
        marks.borrow_mut().push(lent.clone());
        Some(marks.borrow().len())
    };
    let destroyed: RefCell<Vec<usize>> = RefCell::default();
    let destroy = |gone| destroyed.borrow_mut().push(gone);
    let mut buffers = BufferSet::default();
    let bar = (400, 37);

    assert_eq!(buffers.lend(bar, make, destroy).copied(), Some(1));
    assert_eq!(
        buffers.lend(bar, make, destroy).copied(),
        Some(2),
        "1 is shown"
    );
    for _ in 0..5_000 {
        marks.borrow()[0].release();
        assert_eq!(buffers.lend(bar, make, destroy).copied(), Some(1));
        marks.borrow()[1].release();
        assert_eq!(buffers.lend(bar, make, destroy).copied(), Some(2));
    }
    assert_eq!(marks.borrow().len(), 2, "two buffers serve every draw");
    assert_eq!(buffers.made(), 2, "the set counts what it made");
    assert!(destroyed.borrow().is_empty());

    // The window is resized: buffer 1, released, fits nothing any more.
    marks.borrow()[0].release();
    assert_eq!(buffers.lend((800, 37), make, destroy).copied(), Some(3));
    assert_eq!(*destroyed.borrow(), [1]);
    assert_eq!(buffers.len(), 2);

    assert_eq!(buffers.lend((1, 1), |_| None::<usize>, destroy), None);
    assert_eq!(buffers.len(), 2, "what is lent stays");
}

/// Each seat's pointer hovers, holds and presses on its own: another seat's
/// pointer leaving the frame lets go of nothing this one holds, and a press
/// is answered where its own pointer is. A seat that goes is followed no
/// more, and seats that come and go without end take no more places than
/// the frame follows, none of them a place whose pointer holds a button.
#[test]
fn each_seats_pointer_is_followed_on_its_own() {
    let frame = floating();
    let close = frame.button_rect(Button::Close).expect("shown");
    let (over_x, over_y) = (close.x + 12.0, close.y + 12.0);
    let mut pointers = FramePointers::<u32>::default();

    pointers.moved(&1, &frame, Piece::Header, over_x, over_y);
    assert_eq!(pointers.button(&1, &frame, BTN_LEFT, true, 1_000), None);
    assert_eq!(pointers.held(), Some(Button::Close));

    pointers.moved(&2, &frame, Piece::Header, 20.0, 18.0);
    pointers.left(&2);
    assert_eq!(pointers.hovered(), Some(Button::Close), "seat 1 hovers on");
    assert_eq!(pointers.held(), Some(Button::Close), "and holds on");
    assert_eq!(
        pointers.moved(&2, &frame, Piece::Bottom, 418.0, 5.0),
        CursorIcon::SeResize
    );
    assert_eq!(
        pointers.button(&2, &frame, BTN_LEFT, true, 1_100),
        Some(Action::Resize(Edge::BottomRight)),
        "where seat 2's pointer is"
    );

    for seat in 3..10_003 {
        pointers.moved(&seat, &frame, Piece::Header, 20.0, 18.0);
        pointers.left(&seat);
        assert!(pointers.followed() <= MAX_SEATS, "seat {seat}");
    }
    assert_eq!(
        pointers.button(&1, &frame, BTN_LEFT, false, 1_200),
        Some(Action::Close),
        "seat 1 kept its place and its button"
    );

    pointers.moved(&1, &frame, Piece::Header, over_x, over_y);
    pointers.button(&1, &frame, BTN_LEFT, true, 2_000);
    pointers.forget(&1);
    assert_eq!((pointers.hovered(), pointers.held()), (None, None));
    assert_eq!(
        pointers.button(&1, &frame, BTN_LEFT, false, 2_100),
        None,
        "a seat that went holds nothing"
    );
}

/// A surface below version 4 has no `damage_buffer`: a compositor that
/// offers `wl_compositor` up to 3 is sent the surface's damage, never a
/// request it would close the connection for.
#[test]
fn damage_is_sent_with_the_request_the_surface_has() {
    for (version, sent) in [
        (1, Damage::Surface),
        (3, Damage::Surface),
        (4, Damage::Buffer),
        (6, Damage::Buffer),
    ] {
        assert_eq!(Damage::on(version), sent, "wl_surface version {version}");
    }
}

/// A frame the compositor sizes past any screen is drawn at the largest
/// extent, as every host clamps a window extent; one it does not show is not
/// drawn, and a margin is a transparent image.
#[test]
fn a_frame_past_the_largest_extent_is_drawn_at_it() {
    let plain = PLAIN;
    let wide = Frame {
        content: Size::new(1e7, 300.0),
        ..floating()
    };
    let bar = paint_header(&wide, BufferScale::ONE, plain).expect("drawn");
    assert_eq!((bar.width, bar.height), (MAX_EXTENT, 37));
    let fullscreen = framed(ToplevelStates {
        fullscreen: true,
        ..ToplevelStates::default()
    });
    assert!(paint_header(&fullscreen, BufferScale::ONE, plain).is_none());
    let one = DevicePixelRatio::ONE;
    assert_eq!(device_extent(f64::INFINITY, one), None);
    assert_eq!(device_extent(0.2, one), Some(1));
    assert_eq!(device_extent(36.5, one), Some(37));
    let margin = paint_margin(3, 2).expect("drawn");
    assert!(margin.data.iter().all(|byte| *byte == 0));
    assert!(paint_margin(0, 2).is_none());
    assert!(paint_margin(MAX_EXTENT + 1, 2).is_none());
}

/// The title is shaped over the font collection and rasterized: it has
/// ink, fits the room it is given by an ellipsis, and doubles at scale two.
#[test]
fn a_title_is_shaped_and_rasterized() {
    let fonts = FontCollection::new();
    let mut text = TitleText::new(&fonts);
    let mask = text
        .mask("Client frame", 300.0, BufferScale::ONE)
        .expect("shaped");
    assert!(mask.width > 20 && mask.width <= 300, "{}", mask.width);
    assert!(mask.height >= 15, "{}", mask.height);
    assert!(
        mask.coverage.iter().filter(|value| **value > 128).count() > 50,
        "the title has ink"
    );
    let double = text
        .mask(
            "Client frame",
            300.0,
            BufferScale::try_new(2.0).expect("a scale"),
        )
        .expect("shaped");
    assert!(double.width >= mask.width * 2 - 2 && double.width <= mask.width * 2 + 2);
    let long = "A title far longer than the room the title bar leaves it ".repeat(4);
    let cut = text.mask(&long, 120.0, BufferScale::ONE).expect("shaped");
    assert!(cut.width <= 120, "cut to the room: {}", cut.width);
    assert!(text.mask("", 300.0, BufferScale::ONE).is_none());
    assert!(text.mask("x", 0.0, BufferScale::ONE).is_none());
    assert_eq!(text.generation(), fonts.generation());
}

/// Over a frame without its title bar, a primary press on a move region is
/// the window's: the pointer's travel starts the move, a second press soon
/// after and near maximizes, a secondary press asks for the window menu where
/// the compositor shows one, and the release of a taken press is taken too.
/// A press anywhere else, or over a frame with its own title bar, or none, is
/// the application's.
#[test]
fn a_press_on_the_applications_bar_is_the_windows() {
    use std::time::{Duration, Instant};

    use flui_platform::input_regions::{InputRegion, InputRegionKind};

    use ContentAnswer::{App, Taken};

    let bare = Frame {
        title_bar: false,
        ..floating()
    };
    let mut chrome = ContentChrome {
        scale: 1.0,
        ..ContentChrome::default()
    };
    let _changed = chrome.projected.update(
        &[InputRegion::new(
            InputRegionKind::WindowMove,
            flui_foundation::geometry::Rect::from_ltrb(0.0, 0.0, 400.0, 32.0),
        )],
        1.0,
        None,
    );
    let at = Instant::now();
    let mut pointer = PointerOnContent::default();

    assert_eq!(pointer.moved(&chrome, 200.0, 16.0), None, "no press held");
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, true, at),
        Taken(None)
    );
    assert_eq!(pointer.moved(&chrome, 202.0, 17.0), None, "within the slop");
    assert_eq!(pointer.moved(&chrome, 220.0, 30.0), Some(Action::Move));
    assert_eq!(
        pointer.moved(&chrome, 240.0, 30.0),
        None,
        "one move a press"
    );
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, false, at),
        Taken(None)
    );

    let later = at + Duration::from_secs(2);
    pointer.moved(&chrome, 100.0, 10.0);
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, true, later),
        Taken(None)
    );
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, false, later),
        Taken(None)
    );
    assert_eq!(
        pointer.button(
            &chrome,
            Some(&bare),
            BTN_LEFT,
            true,
            later + Duration::from_millis(100)
        ),
        Taken(Some(Action::Maximize)),
        "a double-click"
    );
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, false, later),
        Taken(None)
    );
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_RIGHT, true, later),
        Taken(Some(Action::Menu { x: 100, y: 10 }))
    );
    let no_menu = Frame {
        offers: Offers {
            window_menu: false,
            ..EVERYTHING
        },
        ..bare
    };
    assert_eq!(
        pointer.button(&chrome, Some(&no_menu), BTN_RIGHT, true, later),
        App
    );

    pointer.moved(&chrome, 100.0, 200.0);
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, true, later),
        App,
        "the content"
    );
    assert_eq!(
        pointer.button(&chrome, Some(&bare), BTN_LEFT, false, later),
        App
    );
    pointer.moved(&chrome, 100.0, 10.0);
    assert_eq!(
        pointer.button(&chrome, Some(&floating()), BTN_LEFT, true, later),
        App,
        "the frame's own title bar"
    );
    assert_eq!(
        pointer.button(&chrome, None, BTN_LEFT, true, later),
        App,
        "no frame"
    );

    // A press on the bar waits for its own seat's pointer to travel, and
    // the move it starts names that press.
    let mut pointers = FramePointers::<u32>::default();
    let press = ContentPress {
        serial: 5,
        code: BTN_LEFT,
        pressed: true,
    };
    assert_eq!(pointers.content_moved(&1, &chrome, 200.0, 16.0), None);
    assert_eq!(
        pointers.content_button(&1, &chrome, Some(&bare), press, later),
        Taken(None)
    );
    assert_eq!(
        pointers.content_moved(&2, &chrome, 300.0, 30.0),
        None,
        "seat 2 pressed nothing"
    );
    assert_eq!(pointers.content_moved(&1, &chrome, 230.0, 30.0), Some(5));
    assert_eq!(pointers.content_moved(&1, &chrome, 260.0, 30.0), None);
}

/// A move, a resize and the window menu are carried out under the press's
/// seat and serial; a close, minimize, maximize or restore names no seat.
#[test]
fn only_a_grab_request_names_its_seat() {
    for (action, names) in [
        (Action::Move, true),
        (Action::Resize(Edge::Left), true),
        (Action::Menu { x: 1, y: 2 }, true),
        (Action::Close, false),
        (Action::Minimize, false),
        (Action::Maximize, false),
        (Action::Restore, false),
    ] {
        assert_eq!(action.names_seat(), names, "{action:?}");
    }
}

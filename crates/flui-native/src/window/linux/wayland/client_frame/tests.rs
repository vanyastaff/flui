use flui_foundation::geometry::Size;
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
    let pixels = paint_header(&frame, 1.0, plain).expect("drawn");
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
        paint_header(&maximized, 1.0, plain)
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
        1.0,
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
        1.0,
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
        2.0,
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

/// A frame the compositor sizes past any screen, at no scale, or not shown
/// is not drawn, and a margin is a transparent image.
#[test]
fn a_frame_past_the_largest_extent_is_not_drawn() {
    let plain = HeaderLook {
        look: LIGHT,
        hovered: None,
        held: None,
        title: None,
    };
    let wide = Frame {
        content: Size::new(1e7, 300.0),
        ..floating()
    };
    assert!(paint_header(&wide, 1.0, plain).is_none());
    assert!(paint_header(&floating(), f64::NAN, plain).is_none());
    assert!(paint_header(&floating(), 0.0, plain).is_none());
    let fullscreen = framed(ToplevelStates {
        fullscreen: true,
        ..ToplevelStates::default()
    });
    assert!(paint_header(&fullscreen, 1.0, plain).is_none());
    assert_eq!(device_extent(f64::INFINITY, 1.0), None);
    assert_eq!(device_extent(0.2, 1.0), None);
    assert_eq!(device_extent(36.5, 1.0), Some(37));
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
    let mask = text.mask("Client frame", 300.0, 1.0).expect("shaped");
    assert!(mask.width > 20 && mask.width <= 300, "{}", mask.width);
    assert!(mask.height >= 15, "{}", mask.height);
    assert!(
        mask.coverage.iter().filter(|value| **value > 128).count() > 50,
        "the title has ink"
    );
    let double = text.mask("Client frame", 300.0, 2.0).expect("shaped");
    assert!(double.width >= mask.width * 2 - 2 && double.width <= mask.width * 2 + 2);
    let long = "A title far longer than the room the title bar leaves it ".repeat(4);
    let cut = text.mask(&long, 120.0, 1.0).expect("shaped");
    assert!(cut.width <= 120, "cut to the room: {}", cut.width);
    assert!(text.mask("", 300.0, 1.0).is_none());
    assert!(text.mask("x", 0.0, 1.0).is_none());
    assert_eq!(text.generation(), fonts.generation());
}

//! The gallery's screens, one per interaction widget.

mod cards;
mod collapsing;
mod focus;
mod gestures;
mod menu;
mod pages;
mod refresh;
mod sheet;
mod swipe;
mod viewer;

use flui::prelude::*;

use crate::style;

/// A screen of the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    /// `SwipeActions` rows and `Dismissible` rows.
    Swipe,
    /// A list under `RefreshIndicator`.
    Refresh,
    /// A `CollapsingHeader` over a list.
    Collapsing,
    /// A `PageView` of three pages.
    Pages,
    /// An `AnchoredDrag` bottom sheet over a map.
    Sheet,
    /// An `InteractiveViewer` over a painted grid.
    Viewer,
    /// Cards moved between columns with `Draggable` and `DropTarget`.
    Cards,
    /// Rows with a `ContextMenuArea`.
    Menu,
    /// A `GestureDetector` counting what it recognised.
    Gestures,
    /// Buttons, a text field and a nested navigator under keyboard focus.
    Focus,
}

impl Screen {
    /// Every screen, in sidebar order.
    pub const ALL: [Self; 10] = [
        Self::Swipe,
        Self::Refresh,
        Self::Collapsing,
        Self::Pages,
        Self::Sheet,
        Self::Viewer,
        Self::Cards,
        Self::Menu,
        Self::Gestures,
        Self::Focus,
    ];

    /// The sidebar entry's label, which also names its button.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Swipe => "Swipe actions",
            Self::Refresh => "Pull to refresh",
            Self::Collapsing => "Collapsing header",
            Self::Pages => "Pages",
            Self::Sheet => "Bottom sheet",
            Self::Viewer => "Zoom and pan",
            Self::Cards => "Drag and drop",
            Self::Menu => "Context menu",
            Self::Gestures => "Gestures",
            Self::Focus => "Focus and keys",
        }
    }

    pub(crate) fn view(self) -> BoxedView {
        match self {
            Self::Swipe => swipe::SwipeScreen.into_view().boxed(),
            Self::Refresh => refresh::RefreshScreen.into_view().boxed(),
            Self::Collapsing => collapsing::CollapsingScreen.into_view().boxed(),
            Self::Pages => pages::PagesScreen.into_view().boxed(),
            Self::Sheet => sheet::SheetScreen.into_view().boxed(),
            Self::Viewer => viewer::ViewerScreen.into_view().boxed(),
            Self::Cards => cards::CardsScreen.into_view().boxed(),
            Self::Menu => menu::MenuScreen.into_view().boxed(),
            Self::Gestures => gestures::GesturesScreen.into_view().boxed(),
            Self::Focus => focus::FocusScreen.into_view().boxed(),
        }
    }
}

/// A screen's frame: its title, what to try, the readout of the last
/// outcome, then its content filling the rest.
fn page(
    title: &str,
    hint: &str,
    readout: impl IntoView,
    content: impl IntoView,
) -> Column<Vec<BoxedView>> {
    Column::new(vec![
        style::title(title).boxed(),
        SizedBox::height(4.0).boxed(),
        style::caption(hint).boxed(),
        SizedBox::height(12.0).boxed(),
        Align::new(Alignment::CENTER_LEFT).child(readout).boxed(),
        SizedBox::height(16.0).boxed(),
        Expanded::new(content).boxed(),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
}

/// A row of a list: a white strip of `height` with `content` inset.
fn list_row(height: f64, content: impl IntoView) -> Container {
    Container::new()
        .color(style::SURFACE)
        .height(height)
        .padding(EdgeInsets::symmetric(0.0, 16.0))
        .alignment(Alignment::CENTER_LEFT)
        .child(content)
}

/// A hairline between rows.
fn divider() -> ColoredBox {
    ColoredBox::new(style::LINE).child(SizedBox::height(1.0))
}

/// A circle with an initial, as a row's avatar.
fn avatar(initial: char, color: Color) -> ClipOval {
    ClipOval::new().child(
        Container::new()
            .color(color)
            .width(36.0)
            .height(36.0)
            .alignment(Alignment::CENTER)
            .child(style::tinted(initial.to_string(), style::SURFACE)),
    )
}

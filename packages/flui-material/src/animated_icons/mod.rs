//! [`AnimatedIcons`]: the Material animated icon pairs, as [`AnimatedIconData`] for
//! `flui-widgets`' [`AnimatedIcon`](flui_sdk::widgets::AnimatedIcon).
//!
//! The data is the key frames of the Material animated icons, taken from Flutter's generated
//! data (BSD-3-Clause; `assets/animated_icons/LICENSE-flutter.txt`) at the revision the SVG
//! frames in `assets/animated_icons` name. `cargo xtask icon-import flutter` writes those
//! frames from a Flutter checkout and `cargo xtask icon-import material --write` the constants
//! here from the frames; `cargo xtask checks` fails when the two disagree. The format, the
//! interpolation and the drawing are FLUI's own.

use flui_sdk::widgets::AnimatedIconData;

#[expect(
    clippy::unreadable_literal,
    reason = "key frames written by `cargo xtask icon-import material`, read by no one"
)]
mod data {
    pub(super) mod add_event;
    pub(super) mod arrow_menu;
    pub(super) mod close_menu;
    pub(super) mod ellipsis_search;
    pub(super) mod event_add;
    pub(super) mod home_menu;
    pub(super) mod list_view;
    pub(super) mod menu_arrow;
    pub(super) mod menu_close;
    pub(super) mod menu_home;
    pub(super) mod pause_play;
    pub(super) mod play_pause;
    pub(super) mod search_ellipsis;
    pub(super) mod view_list;
}

/// The Material animated icon pairs. Each goes from its first icon at progress 0 to its
/// second at 1; its twin runs the other way.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedIcons;

impl AnimatedIcons {
    /// Add to event.
    pub const ADD_EVENT: AnimatedIconData = data::add_event::DATA;

    /// Arrow to menu. Mirrors under right-to-left text.
    pub const ARROW_MENU: AnimatedIconData = data::arrow_menu::DATA;

    /// Close to menu.
    pub const CLOSE_MENU: AnimatedIconData = data::close_menu::DATA;

    /// Ellipsis to search.
    pub const ELLIPSIS_SEARCH: AnimatedIconData = data::ellipsis_search::DATA;

    /// Event to add.
    pub const EVENT_ADD: AnimatedIconData = data::event_add::DATA;

    /// Home to menu.
    pub const HOME_MENU: AnimatedIconData = data::home_menu::DATA;

    /// List to view. Mirrors under right-to-left text.
    pub const LIST_VIEW: AnimatedIconData = data::list_view::DATA;

    /// Menu to arrow. Mirrors under right-to-left text.
    pub const MENU_ARROW: AnimatedIconData = data::menu_arrow::DATA;

    /// Menu to close.
    pub const MENU_CLOSE: AnimatedIconData = data::menu_close::DATA;

    /// Menu to home.
    pub const MENU_HOME: AnimatedIconData = data::menu_home::DATA;

    /// Pause to play.
    pub const PAUSE_PLAY: AnimatedIconData = data::pause_play::DATA;

    /// Play to pause.
    pub const PLAY_PAUSE: AnimatedIconData = data::play_pause::DATA;

    /// Search to ellipsis.
    pub const SEARCH_ELLIPSIS: AnimatedIconData = data::search_ellipsis::DATA;

    /// View to list. Mirrors under right-to-left text.
    pub const VIEW_LIST: AnimatedIconData = data::view_list::DATA;
}

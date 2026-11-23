//! # `flui_material`
//!
//! Material Design 3 for FLUI: the theme ([`ThemeData`], [`ColorScheme`],
//! [`TextTheme`], [`Theme`]), the application shell ([`MaterialApp`] over
//! `flui-widgets`' `WidgetsApp`, ADR-0042), the surfaces ([`Material`],
//! [`InkWell`], [`Card`]) and the component catalog: buttons, app bars and
//! [`Scaffold`], dialogs, text fields, lists, tables, navigation bars, chips,
//! tabs, drawers, snack bars, progress indicators and the container
//! transform.
//!
//! Every type is at the crate root. A component's M3 token values are its
//! defaults; a theme slot (`ThemeData::elevated_button_theme` and the rest)
//! overrides them per property, and the widget's own builder overrides both.
//!
//! The baseline palettes are [`ColorScheme::light`] and [`ColorScheme::dark`]
//! and the type scale is [`english_like_2021`]. Seed-colour schemes and the
//! dense and tall type scales are not built yet.
//!
//! ```rust
//! use flui_material::{Theme, ThemeData};
//! use flui_sdk::widgets::SizedBox;
//!
//! let _themed = Theme::new(ThemeData::dark(), SizedBox::shrink());
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
#![deny(missing_docs)]
#![warn(clippy::disallowed_types)]

mod animated_icons;
mod app;
mod app_bar;
mod back_button;
mod button_style;
mod button_style_button;
mod card;
mod carousel_view;
mod checkbox;
mod chip;
mod color_scheme;
mod data_table;
mod dialog;
mod divider;
mod drawer;
mod elevated_button;
mod event_callback;
mod filled_button;
mod flexible_space_bar;
mod floating_action_button;
mod horizontal_side;
mod icon_button;
mod ink_well;
mod input_decorator;
mod list_tile;
mod material;
mod navigation_bar;
mod open_container;
mod outlined_button;
mod page_transitions;
mod progress_indicator;
mod radio;
mod ripple;
mod scaffold;
mod scaffold_messenger;
mod shape;
mod sliver_app_bar;
mod snack_bar;
mod state_color;
mod switch;
mod tab_bar_view;
mod tab_controller;
mod tabs;
mod text_button;
mod text_field;
mod text_form_field;
mod text_theme;
mod theme;
mod theme_data;
mod theme_lerp;
mod typography;

pub use animated_icons::AnimatedIcons;
pub use app::{MaterialApp, ThemeMode};
pub use app_bar::{AppBar, DEFAULT_TOOLBAR_HEIGHT};
pub use back_button::BackButton;
pub use button_style::ButtonStyle;
pub use card::Card;
pub use carousel_view::CarouselView;
pub use checkbox::{Checkbox, CheckboxState};
pub use chip::{Chip, FilterChip};
pub use color_scheme::ColorScheme;
pub use data_table::{DataCell, DataColumn, DataRow, DataTable};
pub use dialog::{AlertDialog, Dialog, show_dialog};
pub use divider::{Divider, VerticalDivider};
pub use drawer::{Drawer, DrawerAlignment, DrawerController, DrawerControllerState, DrawerHandle};
pub use elevated_button::ElevatedButton;
pub use filled_button::FilledButton;
pub use flexible_space_bar::{FlexibleSpaceBar, FlexibleSpaceBarData, FlexibleSpaceBarSettings};
pub use floating_action_button::{FloatingActionButton, FloatingActionButtonState};
pub use icon_button::IconButton;
pub use ink_well::{InkWell, InkWellState};
pub use input_decorator::{InputDecoration, InputDecorator, InputDecoratorState};
pub use list_tile::ListTile;
pub use material::Material;
pub use navigation_bar::{NavigationBar, NavigationDestination};
pub use open_container::{
    CloseAction, ContainerTransition, OpenAction, OpenContainer, OpenContainerState,
};
pub use outlined_button::OutlinedButton;
pub use page_transitions::{PageTransition, PageTransitionsTheme, SharedAxis};
pub use progress_indicator::{
    CircularProgressIndicator, CircularProgressIndicatorState, CircularWavyProgressIndicator,
    CircularWavyProgressIndicatorState, ContainedLoadingIndicator, ContainedLoadingIndicatorState,
    LinearProgressIndicator, LinearProgressIndicatorState, LinearWavyProgressIndicator,
    LinearWavyProgressIndicatorState, LoadingIndicator, LoadingIndicatorState,
};
pub use radio::{Radio, RadioState};
pub use ripple::{RippleConfiguration, RippleScope, StateLayerAlpha};
pub use scaffold::{Scaffold, ScaffoldScope, ScaffoldState};
pub use scaffold_messenger::{
    ScaffoldMessenger, ScaffoldMessengerHandle, ScaffoldMessengerScope, ScaffoldMessengerState,
    SnackBarClosedReason, SnackBarController,
};
pub use shape::MaterialShape;
pub use sliver_app_bar::SliverAppBar;
pub use snack_bar::{SnackBar, SnackBarAction, SnackBarActionState};
pub use switch::{Switch, SwitchState};
pub use tab_bar_view::{TabBarView, TabBarViewState};
pub use tab_controller::{
    DefaultTabController, DefaultTabControllerState, TabController, TabPosition,
};
pub use tabs::{Tab, TabBar, TabBarState, TabIndicatorAnimation};
pub use text_button::TextButton;
pub use text_field::{MaterialTextFieldState, TextField};
pub use text_form_field::{TextFormField, TextFormFieldState};
pub use text_theme::TextTheme;
pub use theme::{AnimatedTheme, Theme};
pub use theme_data::{
    AppBarThemeData, CardThemeData, CarouselViewThemeData, CheckboxThemeData, ChipThemeData,
    DataTableThemeData, DialogThemeData, DividerThemeData, ElevatedButtonThemeData, FabThemeData,
    FilledButtonThemeData, IconButtonThemeData, InputDecorationThemeData, ListTileThemeData,
    MaterialStyle, NavigationBarThemeData, OutlinedButtonThemeData, RadioThemeData,
    SwitchThemeData, TabBarThemeData, TextButtonThemeData, ThemeData,
};
pub use typography::english_like_2021;

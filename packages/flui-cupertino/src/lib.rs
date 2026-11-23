//! # `flui_cupertino`
//!
//! The iOS-style (Cupertino) widget catalog for FLUI.
//!
//! - Color and theme: [`CupertinoColors`], [`CupertinoColor`] and
//!   [`CupertinoDynamicColor`] (the brightness- and contrast-adaptive color
//!   system), [`CupertinoTextThemeData`], [`CupertinoThemeData`] and the
//!   [`CupertinoTheme`] inherited widget that publishes it to a subtree.
//! - Application shell: [`CupertinoApp`] (ADR-0042) over `flui-widgets`'
//!   design-neutral `WidgetsApp`, which also installs the iOS parallax page
//!   slide for its routes.
//! - Components: [`CupertinoButton`], [`CupertinoActivityIndicator`],
//!   [`CupertinoPageControl`], [`CupertinoNavigationBar`] with
//!   [`CupertinoPageScaffold`], and [`CupertinoTabBar`] with
//!   [`CupertinoTabScaffold`] and [`CupertinoTabController`] (the lazy-build,
//!   state-retaining tabbed root layout).
//!
//! Every item is exported at the crate root; the modules are private. The
//! constant tables (the dynamic-color statics, the default text styles, the
//! default theme, the per-size button geometry, the bar heights and border
//! colors) carry the iOS system values. Each type's documentation states what
//! it leaves out of Flutter's counterpart.
//!
//! Other Cupertino components (`CupertinoTextField`, pickers, action sheets,
//! `CupertinoSliverNavigationBar` and large titles) are not provided. The
//! interface-elevation axis of [`CupertinoDynamicColor`] is not resolved:
//! the elevated variants are stored and resolution uses base elevation,
//! because FLUI has no interface-level ambient. Brightness and contrast
//! resolve from the ambient theme and media data.
//!
//! ## ADR-0088: built on `flui-sdk` alone
//!
//! This crate is an official package: its only FLUI dependency is `flui-sdk`
//! (plus `tracing`), and it names the framework as `flui_sdk::…`. The button
//! and the tab bar also import `flui_sdk::widgets::__private::node`, the
//! interactive-node seam ADR-0322 §1 keeps doc-hidden for the official
//! catalogs. It never depends on `flui-material`: the two design systems are
//! independent (ADR-0028), and the kind rule (ADR-0081 §3, ADR-0088 §2)
//! refuses that edge in any dependency kind. It adds no render object of its
//! own: components compose existing widgets (`FadeTransition`,
//! `DecoratedBox`, `Stack`/`Positioned`, `Offstage`, `HeroMode`, …) and name
//! no render-layer value.
//!
//! ## Example
//!
//! ```rust
//! use flui_cupertino::{CupertinoTheme, CupertinoThemeData};
//! use flui_sdk::widgets::SizedBox;
//!
//! let _themed = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
//! ```

#![deny(missing_docs)]

mod activity_indicator;
mod app;
mod bottom_tab_bar;
mod button;
mod colors;
mod nav_bar;
mod page_control;
mod page_scaffold;
mod tab_scaffold;
mod text_theme;
mod theme;

pub use activity_indicator::{CupertinoActivityIndicator, CupertinoActivityIndicatorState};
pub use app::CupertinoApp;
pub use bottom_tab_bar::{CupertinoTabBar, CupertinoTabBarItem};
pub use button::{CupertinoButton, CupertinoButtonSize, CupertinoButtonState};
pub use colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
pub use nav_bar::CupertinoNavigationBar;
pub use page_control::CupertinoPageControl;
pub use page_scaffold::CupertinoPageScaffold;
pub use tab_scaffold::{CupertinoTabController, CupertinoTabScaffold, CupertinoTabScaffoldState};
pub use text_theme::CupertinoTextThemeData;
pub use theme::{CupertinoTheme, CupertinoThemeData};

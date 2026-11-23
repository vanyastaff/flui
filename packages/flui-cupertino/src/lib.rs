//! # `flui_cupertino`
//!
//! The iOS-style (Cupertino) widget catalog for FLUI.
//!
//! - Color and theme: [`CupertinoColors`] (UIKit's palette), [`CupertinoColor`]
//!   and [`CupertinoDynamicColor`] (the brightness-, contrast- and level-adaptive
//!   color system), [`CupertinoUserInterfaceLevel`] (the base or elevated
//!   level a subtree's colors resolve for), [`CupertinoThemeData`] and
//!   [`CupertinoTextThemeData`] (what an
//!   author writes), the [`CupertinoTheme`] inherited widget that publishes
//!   them to a subtree, and [`ResolvedCupertinoTheme`] with
//!   [`ResolvedCupertinoTextTheme`] (what widgets read, resolved for their
//!   context).
//! - Application shell: [`CupertinoApp`] (ADR-0042) over `flui-widgets`'
//!   design-neutral `WidgetsApp`, which also installs the iOS parallax page
//!   slide for its routes.
//! - Components: [`CupertinoButton`], [`CupertinoActivityIndicator`],
//!   [`CupertinoPageControl`], [`CupertinoNavigationBar`] with
//!   [`CupertinoPageScaffold`], and [`CupertinoTabBar`] with
//!   [`CupertinoTabScaffold`] and [`CupertinoTabController`] (the lazy-build,
//!   state-retaining tabbed root layout). Both bars are
//!   [`ObstructingPreferredSizeView`]s: a scaffold lets content run under a
//!   bar whose background is translucent, which the bar blurs.
//!
//! Every item is exported at the crate root; the modules are private. The
//! constant tables (the dynamic-color statics, the default text styles, the
//! default theme, the per-size button geometry, the bar heights and border
//! colors) carry the iOS system values. Each type's documentation states what
//! it leaves out of Flutter's counterpart.
//!
//! Other Cupertino components (`CupertinoTextField`, pickers, action sheets,
//! `CupertinoSliverNavigationBar` and large titles) are not provided, so
//! nothing in this package installs an elevated
//! [`CupertinoUserInterfaceLevel`]; an application wraps what it presents
//! above its main content in one. A dynamic color takes its brightness from
//! the ambient theme, its contrast from the media data and its level from
//! the nearest `CupertinoUserInterfaceLevel`.
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
//! `DecoratedBox`, `BackdropFilter`, `Stack`/`Positioned`, `Offstage`,
//! `HeroMode`, …), and the navigation bar lays out its slots with a private
//! `MultiChildLayoutDelegate` under `CustomMultiChildLayout`.
//!
//! ## Example
//!
//! ```rust
//! use flui_cupertino::{CupertinoColors, CupertinoTheme, CupertinoThemeData};
//! use flui_sdk::platform::Brightness;
//! use flui_sdk::view::prelude::*;
//! use flui_sdk::widgets::Text;
//!
//! #[derive(Clone, StatelessView)]
//! struct Greeting;
//!
//! impl StatelessView for Greeting {
//!     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
//!         // Every colour of the resolved theme is concrete for this context.
//!         let theme = CupertinoTheme::of(ctx);
//!         Text::new("Hello").style(theme.text_theme.nav_title_text_style.clone())
//!     }
//! }
//!
//! let mut data = CupertinoThemeData::default();
//! data.brightness = Some(Brightness::Dark);
//! data.primary_color = Some(CupertinoColors::SYSTEM_RED.into());
//! let _themed = CupertinoTheme::new(data, Greeting);
//! ```

#![deny(missing_docs)]

mod activity_indicator;
mod app;
mod bar_background;
mod bottom_tab_bar;
mod button;
mod colors;
mod interface_level;
mod nav_bar;
mod navigation_toolbar;
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
pub use interface_level::{CupertinoUserInterfaceLevel, CupertinoUserInterfaceLevelData};
pub use nav_bar::CupertinoNavigationBar;
pub use page_control::CupertinoPageControl;
pub use page_scaffold::{CupertinoPageScaffold, ObstructingPreferredSizeView};
pub use tab_scaffold::{CupertinoTabController, CupertinoTabScaffold, CupertinoTabScaffoldState};
pub use text_theme::{CupertinoTextThemeData, ResolvedCupertinoTextTheme};
pub use theme::{CupertinoTheme, CupertinoThemeData, ResolvedCupertinoTheme};

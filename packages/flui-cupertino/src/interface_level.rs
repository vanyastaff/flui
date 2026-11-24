//! The interface level a subtree is drawn at: [`CupertinoUserInterfaceLevel`]
//! and the level it publishes, [`CupertinoUserInterfaceLevelData`].

use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};

/// The visual level of a subtree's content, which picks between a
/// [`CupertinoDynamicColor`](crate::CupertinoDynamicColor)'s base and
/// elevated variants (UIKit's `UIUserInterfaceLevel`).
///
/// UIKit draws a window's main content at the base level and content that
/// sits above it, such as a sheet or an alert, at the elevated level, where
/// dark backgrounds are a lighter grey so the layers stay apart.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/interface_level.dart:15`.
#[expect(
    clippy::exhaustive_enums,
    reason = "a dynamic colour carries a variant for each of these two levels and \
              no other, so a third level would have no colour to resolve to; UIKit's \
              `unspecified` is the absence of a level, which `maybe_of` returns as `None`"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CupertinoUserInterfaceLevelData {
    /// The level of a window's main content. Without a
    /// [`CupertinoUserInterfaceLevel`] ancestor, a subtree is at this level.
    #[default]
    Base,
    /// The level of content above the main content, such as a sheet or a
    /// dialog.
    Elevated,
}

/// Publishes a [`CupertinoUserInterfaceLevelData`] to its subtree: every
/// dynamic colour resolved below it, the theme's included, picks the
/// variant of that level.
///
/// Nothing in this package installs one; an application wraps the content it
/// presents above its main content (a sheet, a dialog) in an
/// [`Elevated`](CupertinoUserInterfaceLevelData::Elevated) level, as Flutter's
/// Cupertino dialogs and sheets do.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/interface_level.dart:41`.
///
/// # Example
///
/// ```rust
/// use flui_cupertino::{CupertinoUserInterfaceLevel, CupertinoUserInterfaceLevelData};
/// use flui_sdk::widgets::SizedBox;
///
/// let _sheet = CupertinoUserInterfaceLevel::new(
///     CupertinoUserInterfaceLevelData::Elevated,
///     SizedBox::shrink(),
/// );
/// ```
#[derive(Clone)]
pub struct CupertinoUserInterfaceLevel {
    level: CupertinoUserInterfaceLevelData,
    child: BoxedView,
}

impl CupertinoUserInterfaceLevel {
    /// Wrap `child` in a subtree drawn at `level`.
    #[must_use]
    pub fn new(level: CupertinoUserInterfaceLevelData, child: impl IntoView) -> Self {
        Self {
            level,
            child: child.into_view().boxed(),
        }
    }

    /// The level of the nearest [`CupertinoUserInterfaceLevel`], registering
    /// a dependency on it; [`Base`](CupertinoUserInterfaceLevelData::Base)
    /// without one, the level a colour resolves for when nothing says
    /// otherwise.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> CupertinoUserInterfaceLevelData {
        Self::maybe_of(ctx).unwrap_or_default()
    }

    /// The level of the nearest [`CupertinoUserInterfaceLevel`], registering
    /// a dependency on it, or `None` without one.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<CupertinoUserInterfaceLevelData> {
        ctx.depend_on::<Self, _>(|scope| scope.level)
    }
}

impl std::fmt::Debug for CupertinoUserInterfaceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoUserInterfaceLevel")
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

impl InheritedView for CupertinoUserInterfaceLevel {
    type Data = CupertinoUserInterfaceLevelData;

    fn data(&self) -> &Self::Data {
        &self.level
    }

    fn child(&self) -> &BoxedView {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.level != old.level
    }
}

impl_inherited_view!(CupertinoUserInterfaceLevel);

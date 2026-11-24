//! [`BackButton`] — an [`IconButton`] with a back-arrow glyph that pops the
//! route it is on.
//!
//! # Composition
//!
//! This type composes [`IconButton`], wiring
//! [`PageHandle::maybe_pop`](flui_sdk::widgets::PageHandle::maybe_pop)
//! as the default press handler and [`BackButton::on_pressed`] as the override
//! that replaces it (for example to pop the platform's navigation stack
//! instead of the route).
//!
//! # Glyph: the `arrow_back` codepoint
//!
//! The Material spec uses a platform-specific glyph (an iOS-style chevron on
//! iOS/macOS, the `arrow_back` arrow everywhere else). FLUI has no
//! platform/action-icon-theme support to switch on, so
//! [`back_arrow_icon_data`] carries the `arrow_back` icon's identity:
//! codepoint `0xE5C4` in the upstream `material-design-icons` numbering of the
//! face (not that of Flutter's `Icons`), font family `"Material Icons"`, `match_text_direction:
//! true`. The face of that family is in every application that depends on
//! this package, which turns on `flui-sdk`'s `material-icons` feature
//! (ADR-0397). **Named limitation:** no
//! iOS/macOS-specific glyph switch: it waits on platform detection, not on
//! anything specific to this type. `match_text_direction` is applied: under a
//! right-to-left `Directionality`, `Icon` mirrors the arrow so it points to the
//! reading start.

use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Icon, IconData, PageHandle};

use crate::button_style_button::PressCallback;
use crate::icon_button::IconButton;

/// The Material `arrow_back` icon (the module docs' "Glyph" section).
#[must_use]
pub(crate) fn back_arrow_icon_data() -> IconData {
    IconData {
        match_text_direction: true,
        ..IconData::new(0xE5C4).with_font_family("Material Icons")
    }
}

/// An [`IconButton`] with a back-arrow glyph. With no [`Self::on_pressed`]
/// override, tapping it calls
/// [`PageHandle::maybe_pop`](flui_sdk::widgets::PageHandle::maybe_pop)
/// on the route it is on, Flutter's `Navigator.maybePop(context)`. Outside
/// every route (and with no override), the button mounts disabled
/// rather than panicking.
///
/// ```rust
/// use flui_material::BackButton;
///
/// let _default = BackButton::new();
/// let _overridden = BackButton::new().on_pressed(|_ctx| { /* custom pop */ });
/// ```
#[derive(Clone, Default, StatelessView)]
pub struct BackButton {
    on_pressed: Option<PressCallback>,
}

impl std::fmt::Debug for BackButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackButton")
            .field("has_override", &self.on_pressed.is_some())
            .finish()
    }
}

impl BackButton {
    /// A `BackButton` with no override: tapping it calls
    /// [`PageHandle::maybe_pop`](flui_sdk::widgets::PageHandle::maybe_pop)
    /// on the route it is on.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the default `maybe_pop` behavior with `callback`.
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventContext<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(crate::event_callback::press_callback(callback));
        self
    }
}

impl StatelessView for BackButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let icon = Icon::new(back_arrow_icon_data());
        let mut button = IconButton::new(icon);

        if let Some(on_pressed) = self.on_pressed.clone() {
            button = button.on_pressed(move |ctx| on_pressed(ctx));
        } else if let Some(page) = PageHandle::of(ctx) {
            button = button.on_pressed(move |_ctx| {
                page.maybe_pop();
            });
        }

        button
    }
}

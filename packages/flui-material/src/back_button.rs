//! [`BackButton`] — an [`IconButton`] with a back-arrow glyph that pops the
//! nearest [`Navigator`](flui_sdk::widgets::Navigator).
//!
//! # Composition
//!
//! This type composes [`IconButton`], wiring
//! [`NavigatorHandle::maybe_pop`](flui_sdk::widgets::NavigatorHandle::maybe_pop)
//! as the default press handler and [`BackButton::on_pressed`] as the override
//! that replaces it (for example to pop the platform's navigation stack
//! instead of the `Navigator`).
//!
//! # Glyph: the `arrow_back` codepoint, not a bundled asset
//!
//! The Material spec uses a platform-specific glyph (an iOS-style chevron on
//! iOS/macOS, the `arrow_back` arrow everywhere else). FLUI has no
//! platform/action-icon-theme substrate to switch on, and — as
//! [`Icon`]'s own module docs state plainly — **no
//! bundled icon font**: every codepoint shapes to tofu (the "missing glyph"
//! box) until font-registration infrastructure lands, regardless of which
//! icon is requested. Given that pre-existing, already-named rendering gap,
//! [`back_arrow_icon_data`] carries the `arrow_back` icon's exact identity —
//! codepoint `0xE092`, font family `"Material Icons"`, `match_text_direction:
//! true` — rather than inventing a substitute glyph or a hand-drawn path (no
//! such drawn-path convention exists in this crate). **Named limitation:** no
//! iOS/macOS-specific glyph switch, and (per `Icon`'s own docs)
//! `match_text_direction` is carried on the data but not yet applied by
//! `Icon::build` — both wait on their respective missing substrates
//! (platform detection; RTL mirroring), not on anything specific to this
//! type.

use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Icon, IconData, NavigatorHandle};

use crate::button_style_button::PressCallback;
use crate::icon_button::IconButton;

/// The Material `arrow_back` icon. See the module docs' "Glyph" section for
/// why this codepoint is used even with no bundled icon font to shape it
/// against.
#[must_use]
pub fn back_arrow_icon_data() -> IconData {
    IconData {
        match_text_direction: true,
        ..IconData::new(0xE092).with_font_family("Material Icons")
    }
}

/// An [`IconButton`] with a back-arrow glyph. With no [`Self::on_pressed`]
/// override, tapping it calls
/// [`NavigatorHandle::maybe_pop`](flui_sdk::widgets::NavigatorHandle::maybe_pop)
/// on the nearest enclosing [`Navigator`](flui_sdk::widgets::Navigator). With
/// no navigator ancestor at all (and no override), the button mounts disabled
/// rather than panicking.
///
/// ```rust
/// use flui_material::BackButton;
///
/// let _default = BackButton::new();
/// let _overridden = BackButton::new().on_pressed(|_cx| { /* custom pop */ });
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
    /// [`NavigatorHandle::maybe_pop`](flui_sdk::widgets::NavigatorHandle::maybe_pop)
    /// on the nearest [`Navigator`](flui_sdk::widgets::Navigator).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the default `maybe_pop` behavior with `callback`.
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
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
            button = button.on_pressed(move |cx| on_pressed(cx));
        } else if let Some(navigator) = NavigatorHandle::maybe_of(ctx) {
            button = button.on_pressed(move |_cx| {
                navigator.maybe_pop();
            });
        }

        button
    }
}

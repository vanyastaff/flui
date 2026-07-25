//! [`BackButton`] — an [`IconButton`] with a back-arrow glyph that pops the
//! nearest [`Navigator`](flui_widgets::Navigator).
//!
//! # Flutter parity
//!
//! `material/action_buttons.dart`'s `BackButton`/`BackButtonIcon` (oracle
//! tag `3.44.0`; `material/back_button.dart` is a pure re-export of the same
//! file, so this cites `action_buttons.dart` directly). The oracle's
//! `BackButton extends _ActionButton extends IconButton`, whose default
//! `_onPressedCallback` calls `Navigator.maybePop(context)` — this type
//! composes [`IconButton`] the same way (not by subclassing, since Rust has
//! no implementation inheritance), wiring
//! [`NavigatorHandle::maybe_pop`](flui_widgets::NavigatorHandle::maybe_pop)
//! as the default handler and [`BackButton::on_pressed`] as the override that
//! replaces it — Flutter parity: "The `onPressed` callback can … be used to
//! pop the platform's navigation stack … instead of Flutter's `Navigator`."
//!
//! # Glyph: `Icons.arrow_back`'s codepoint, not a bundled asset
//!
//! `BackButtonIcon` resolves a platform-specific glyph
//! (`Icons.arrow_back_ios_new_rounded` on iOS/macOS, `Icons.arrow_back`
//! everywhere else, including web) through the ambient `Theme.platform` and
//! an `ActionIconTheme` override hook. FLUI has no `TargetPlatform`/
//! `ActionIconTheme` substrate to switch on, and — as
//! [`Icon`]'s own module docs state plainly — **no
//! bundled icon font**: every codepoint shapes to tofu (the "missing glyph"
//! box) until font-registration infrastructure lands, regardless of which
//! icon is requested. Given that pre-existing, already-named rendering gap,
//! [`back_arrow_icon_data`] carries `Icons.arrow_back`'s exact identity —
//! codepoint `0xE092`, font family `"Material Icons"`, `match_text_direction:
//! true` (`icons.dart`'s `arrow_back` constant, tag `3.44.0`) — rather than
//! inventing a substitute glyph or a hand-drawn path (no such drawn-path
//! convention exists in this crate). **Named divergence:** no
//! iOS/macOS-specific glyph switch, and (per `Icon`'s own docs)
//! `match_text_direction` is carried on the data but not yet applied by
//! `Icon::build` — both wait on their respective missing substrates
//! (platform detection; RTL mirroring), not on anything specific to this
//! type.

use flui_view::prelude::*;
use flui_widgets::{Icon, IconData, NavigatorHandle};

use crate::button_style_button::PressCallback;
use crate::icon_button::IconButton;

/// `Icons.arrow_back` — Flutter parity: `icons.dart`'s `arrow_back` constant
/// (tag `3.44.0`). See the module docs' "Glyph" section for why this
/// codepoint is used even with no bundled icon font to shape it against.
#[must_use]
pub fn back_arrow_icon_data() -> IconData {
    IconData {
        match_text_direction: true,
        ..IconData::new(0xE092).with_font_family("Material Icons")
    }
}

/// An [`IconButton`] with a back-arrow glyph. With no [`Self::on_pressed`]
/// override, tapping it calls
/// [`NavigatorHandle::maybe_pop`](flui_widgets::NavigatorHandle::maybe_pop)
/// on the nearest enclosing [`Navigator`](flui_widgets::Navigator) — Flutter
/// parity: `Navigator.maybePop(context)`. With no navigator ancestor at all
/// (and no override), the button mounts disabled rather than panicking — a
/// named divergence from the oracle, which unconditionally assumes an
/// ancestor `Navigator` exists.
///
/// ```rust
/// use flui_material::BackButton;
///
/// let _default = BackButton::new();
/// let _overridden = BackButton::new().on_pressed(|| { /* custom pop */ });
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
    /// [`NavigatorHandle::maybe_pop`](flui_widgets::NavigatorHandle::maybe_pop)
    /// on the nearest [`Navigator`](flui_widgets::Navigator).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the default `maybe_pop` behavior with `callback`.
    #[must_use]
    pub fn on_pressed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(callback));
        self
    }
}

impl StatelessView for BackButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let icon = Icon::new(back_arrow_icon_data());
        let mut button = IconButton::new(icon);

        if let Some(on_pressed) = self.on_pressed.clone() {
            button = button.on_pressed(move || on_pressed());
        } else if let Some(navigator) = NavigatorHandle::maybe_of(ctx) {
            button = button.on_pressed(move || {
                navigator.maybe_pop();
            });
        }

        button
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_arrow_icon_data_matches_icons_arrow_back() {
        let icon = back_arrow_icon_data();
        assert_eq!(icon.code_point, 0xE092);
        assert_eq!(icon.font_family.as_deref(), Some("Material Icons"));
        assert!(icon.match_text_direction);
    }

    #[test]
    fn new_has_no_override() {
        assert!(BackButton::new().on_pressed.is_none());
    }

    #[test]
    fn on_pressed_sets_an_override() {
        let button = BackButton::new().on_pressed(|| {});
        assert!(button.on_pressed.is_some());
    }

    #[test]
    fn debug_reports_whether_an_override_is_set_without_the_closure() {
        let debug = format!("{:?}", BackButton::new().on_pressed(|| {}));
        assert!(debug.contains("has_override: true"));
    }
}

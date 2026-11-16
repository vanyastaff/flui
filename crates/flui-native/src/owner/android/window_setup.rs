//! How the Activity's window is set up for the soft keyboard: one mode on
//! every API level.
//!
//! The window keeps `SOFT_INPUT_ADJUST_RESIZE` and stops fitting the system
//! windows: `Window.setDecorFitsSystemWindows(false)` from API 30, the
//! decor view's `SYSTEM_UI_FLAG_LAYOUT_STABLE`, `_LAYOUT_HIDE_NAVIGATION` and
//! `_LAYOUT_FULLSCREEN` below it. In that state Android dispatches the input
//! method's inset to the views (`WindowInsets.Type.ime()` from API 30, the
//! system window inset below it) instead of shrinking the content, so the
//! surface keeps its size and the keyboard reaches layout as its inset
//! (ADR-0277). `NativeActivity.onCreate` sets the mode itself, which is why
//! a manifest attribute could not.
//!
//! This is the decision as data, the table of record. `FluiActivity.onCreate`
//! applies it, after `NativeActivity.onCreate` and before the window's first
//! layout, from its constants `SOFT_INPUT_MODE`, `DECOR_DOES_NOT_FIT_FROM_API`
//! and `EDGE_TO_EDGE_LAYOUT_FLAGS`; `fluiactivity_applies_this_table` holds
//! those constants to what [`window_setup`] answers on each side of the
//! boundary.

use crate::version::gate::android::{AndroidApi, DecorFitsSystemWindows};

/// `WindowManager.LayoutParams.SOFT_INPUT_STATE_UNSPECIFIED`.
pub(crate) const SOFT_INPUT_STATE_UNSPECIFIED: i32 = 0;
/// `WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE`.
pub(crate) const SOFT_INPUT_ADJUST_RESIZE: i32 = 0x10;
/// `View.SYSTEM_UI_FLAG_LAYOUT_STABLE`.
pub(crate) const SYSTEM_UI_FLAG_LAYOUT_STABLE: i32 = 0x100;
/// `View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION`.
pub(crate) const SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION: i32 = 0x200;
/// `View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN`.
pub(crate) const SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN: i32 = 0x400;

/// How the window stops fitting the system windows on one API level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeToEdge {
    /// `Window.setDecorFitsSystemWindows(false)` (API 30 and later).
    DecorDoesNotFit,
    /// These `SYSTEM_UI_FLAG_LAYOUT_*` flags added to the decor view's
    /// system UI visibility (below API 30).
    LayoutFlags(i32),
}

/// What the window is set up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowSetup {
    /// The soft input mode, `Window.setSoftInputMode`.
    pub(crate) soft_input: i32,
    /// How the window lays out edge to edge.
    pub(crate) edge_to_edge: EdgeToEdge,
}

/// The window's setup on API level `api`.
pub(crate) fn window_setup(api: AndroidApi) -> WindowSetup {
    WindowSetup {
        soft_input: SOFT_INPUT_STATE_UNSPECIFIED | SOFT_INPUT_ADJUST_RESIZE,
        edge_to_edge: if api.since::<DecorFitsSystemWindows>().is_some() {
            EdgeToEdge::DecorDoesNotFit
        } else {
            EdgeToEdge::LayoutFlags(
                SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN,
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EdgeToEdge, SOFT_INPUT_ADJUST_RESIZE, SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN,
        SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION, SYSTEM_UI_FLAG_LAYOUT_STABLE, window_setup,
    };
    use crate::version::gate::android::AndroidApi;

    /// One soft input mode on every level, `adjustResize`, with the window
    /// edge to edge: through the layout flags below API 30, through
    /// `setDecorFitsSystemWindows(false)` from it; an unknown level is
    /// treated as the floor.
    #[test]
    fn the_window_resizes_its_insets_not_its_surface_on_every_level() {
        let flags = EdgeToEdge::LayoutFlags(
            SYSTEM_UI_FLAG_LAYOUT_STABLE
                | SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN,
        );
        for (level, edge_to_edge) in [
            (0, flags),
            (23, flags),
            (29, flags),
            (30, EdgeToEdge::DecorDoesNotFit),
            (36, EdgeToEdge::DecorDoesNotFit),
        ] {
            let setup = window_setup(AndroidApi::from_level(level));
            assert_eq!(setup.soft_input, SOFT_INPUT_ADJUST_RESIZE, "API {level}");
            assert_eq!(setup.edge_to_edge, edge_to_edge, "API {level}");
        }
    }

    /// `FluiActivity` applies this table: its soft input mode, the level
    /// from which it stops the decor fitting the system windows, and the
    /// layout flags it adds below that level are what [`window_setup`]
    /// answers on each side of the boundary.
    #[test]
    fn fluiactivity_applies_this_table() {
        let activity = crate::owner::android::java_host::java_source(
            crate::owner::android::java_host::ACTIVITY_CLASS,
        );
        let below = window_setup(AndroidApi::from_level(29));
        let from = window_setup(AndroidApi::from_level(30));
        let EdgeToEdge::LayoutFlags(flags) = below.edge_to_edge else {
            panic!("below API 30 the window lays out through the flags");
        };
        assert_eq!(from.edge_to_edge, EdgeToEdge::DecorDoesNotFit);
        assert_eq!(below.soft_input, from.soft_input, "one mode on every level");
        for stated in [
            format!("static final int SOFT_INPUT_MODE = {};", from.soft_input),
            "static final int DECOR_DOES_NOT_FIT_FROM_API = 30;".to_owned(),
            format!("static final int EDGE_TO_EDGE_LAYOUT_FLAGS = {flags};"),
            "if (Build.VERSION.SDK_INT >= DECOR_DOES_NOT_FIT_FROM_API) {".to_owned(),
            "getWindow().setSoftInputMode(SOFT_INPUT_MODE);".to_owned(),
        ] {
            assert!(activity.contains(&stated), "{stated:?} in FluiActivity");
        }
    }
}

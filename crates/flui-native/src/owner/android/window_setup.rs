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

    /// The value of `FluiActivity`'s `static final int name`.
    fn constant(source: &str, name: &str) -> i32 {
        let stated = format!("static final int {name} = ");
        let at = source
            .find(&stated)
            .unwrap_or_else(|| panic!("{name} in FluiActivity"));
        let rest = &source[at + stated.len()..];
        rest[..rest.find(';').expect("a constant ends its line")]
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{name} is a number"))
    }

    /// The text between `start` and the first `end` after it.
    fn between<'s>(source: &'s str, start: &str, end: &str) -> &'s str {
        let at = source
            .find(start)
            .unwrap_or_else(|| panic!("{start:?} in FluiActivity"));
        let rest = &source[at + start.len()..];
        &rest[..rest
            .find(end)
            .unwrap_or_else(|| panic!("{end:?} after {start:?}"))]
    }

    /// `FluiActivity` applies this table: after `NativeActivity.onCreate` it
    /// sets the table's soft input mode, and from the table's boundary level
    /// on it takes the branch that stops the decor fitting the system
    /// windows, below it the branch that adds the table's layout flags. Each
    /// branch is read for what it applies, so swapped branches, another
    /// boundary or other flags fail.
    #[test]
    fn fluiactivity_applies_this_table() {
        let activity = crate::owner::android::java_host::java_source(
            crate::owner::android::java_host::ACTIVITY_CLASS,
        );
        let on_create = between(
            &activity,
            "protected void onCreate(Bundle savedInstanceState) {",
            "\n    }\n",
        );
        let boundary = constant(&activity, "DECOR_DOES_NOT_FIT_FROM_API");
        let below = window_setup(AndroidApi::from_level(
            u32::try_from(boundary - 1).expect("a level"),
        ));
        let from = window_setup(AndroidApi::from_level(
            u32::try_from(boundary).expect("a level"),
        ));
        assert_ne!(
            below.edge_to_edge, from.edge_to_edge,
            "FluiActivity's boundary is the table's"
        );

        let soft_input = constant(&activity, "SOFT_INPUT_MODE");
        assert_eq!(
            (soft_input, soft_input),
            (below.soft_input, from.soft_input)
        );
        let order = [
            "super.onCreate(savedInstanceState);",
            "getWindow().setSoftInputMode(SOFT_INPUT_MODE);",
            "if (Build.VERSION.SDK_INT >= DECOR_DOES_NOT_FIT_FROM_API) {",
        ]
        .map(|step| {
            on_create
                .find(step)
                .unwrap_or_else(|| panic!("{step:?} in onCreate"))
        });
        assert!(
            order.is_sorted(),
            "the mode is set after NativeActivity's own"
        );

        // What each branch applies, read from the method it calls.
        let flags = constant(&activity, "EDGE_TO_EDGE_LAYOUT_FLAGS");
        let decor = between(&activity, "static void edgeToEdge(Window window) {", "}");
        let layout = between(
            &activity,
            "private static void layoutEdgeToEdge(Window window) {",
            "\n    }",
        );
        assert!(decor.contains("window.setDecorFitsSystemWindows(false);"));
        assert!(layout.contains("| EDGE_TO_EDGE_LAYOUT_FLAGS"));
        let applies = |branch: &str| {
            let branch = branch.trim();
            if branch == "Api30.edgeToEdge(getWindow());" {
                EdgeToEdge::DecorDoesNotFit
            } else if branch == "layoutEdgeToEdge(getWindow());" {
                EdgeToEdge::LayoutFlags(flags)
            } else {
                panic!("a branch FluiActivity's table does not know: {branch:?}")
            }
        };
        let then = between(
            on_create,
            "if (Build.VERSION.SDK_INT >= DECOR_DOES_NOT_FIT_FROM_API) {",
            "} else {",
        );
        let otherwise = between(on_create, "} else {", "}");
        assert_eq!(applies(then), from.edge_to_edge, "from API {boundary}");
        assert_eq!(
            applies(otherwise),
            below.edge_to_edge,
            "below API {boundary}"
        );
    }
}

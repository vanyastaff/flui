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

/// Set `app`'s window up for API level `api`. Java main thread.
///
/// # Errors
///
/// A JNI call failed; the window keeps what `NativeActivity` set.
#[cfg(target_os = "android")]
pub(crate) fn apply(
    app: &android_activity::AndroidApp,
    api: AndroidApi,
) -> Result<(), flui_platform::PreferenceQueryError> {
    use jni::{JValue, jni_sig, jni_str};

    let setup = window_setup(api);
    crate::owner::android::activity::with_activity(app, |env, activity| {
        env.with_local_frame(8, |env| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            env.call_method(
                &window,
                jni_str!("setSoftInputMode"),
                jni_sig!("(I)V"),
                &[JValue::Int(setup.soft_input)],
            )?;
            match setup.edge_to_edge {
                EdgeToEdge::DecorDoesNotFit => {
                    env.call_method(
                        &window,
                        jni_str!("setDecorFitsSystemWindows"),
                        jni_sig!("(Z)V"),
                        &[JValue::Bool(false)],
                    )?;
                }
                EdgeToEdge::LayoutFlags(layout) => {
                    let decor = env
                        .call_method(
                            &window,
                            jni_str!("getDecorView"),
                            jni_sig!("()Landroid/view/View;"),
                            &[],
                        )?
                        .l()?;
                    // Added to what is set: the status bar's appearance
                    // lives in the same flags.
                    let flags = env
                        .call_method(
                            &decor,
                            jni_str!("getSystemUiVisibility"),
                            jni_sig!("()I"),
                            &[],
                        )?
                        .i()?;
                    env.call_method(
                        &decor,
                        jni_str!("setSystemUiVisibility"),
                        jni_sig!("(I)V"),
                        &[JValue::Int(flags | layout)],
                    )?;
                }
            }
            Ok(())
        })
    })
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
}

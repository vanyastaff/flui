//! The status-bar icon appearance a window's content asks for.
//!
//! "Light status bars" in Android's vocabulary means a light bar background
//! with dark icons, so [`SystemUiOverlayStyle::Dark`] sets the flag. With
//! [`InsetsController`] it goes through
//! `WindowInsetsController.setSystemBarsAppearance`; otherwise through the
//! `SYSTEM_UI_FLAG_LIGHT_STATUS_BAR` visibility flag, which the floor (API 23)
//! has. View calls run on the Java main thread, in submission order.

use android_activity::AndroidApp;
use jni::{JValue, jni_sig, jni_str};

use flui_platform::{PlatformSystemChrome, SystemUiOverlayStyle};

use crate::shared::os_version::android::{AndroidApi, InsetsController};

/// `WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS`.
const APPEARANCE_LIGHT_STATUS_BARS: i32 = 0x8;
/// `View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`.
const SYSTEM_UI_FLAG_LIGHT_STATUS_BAR: i32 = 0x2000;

/// `PlatformSystemChrome` for the activity's window.
pub struct AndroidSystemChrome {
    app: AndroidApp,
    api: AndroidApi,
}

impl std::fmt::Debug for AndroidSystemChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidSystemChrome")
            .field("api", &self.api)
            .finish_non_exhaustive()
    }
}

impl AndroidSystemChrome {
    pub(super) fn new(app: AndroidApp, api: AndroidApi) -> Self {
        Self { app, api }
    }
}

impl PlatformSystemChrome for AndroidSystemChrome {
    fn set_overlay_style(&self, style: SystemUiOverlayStyle) {
        let dark_icons = style == SystemUiOverlayStyle::Dark;
        let app = self.app.clone();
        let api = self.api;
        self.app.run_on_java_main_thread(Box::new(move || {
            if let Err(error) = apply(&app, api, dark_icons) {
                tracing::warn!(%error, "status bar appearance was not applied");
            }
        }));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Must run on the Java main thread.
fn apply(
    app: &AndroidApp,
    api: AndroidApi,
    dark_icons: bool,
) -> Result<(), flui_platform::PreferenceQueryError> {
    crate::owner::android::activity::with_activity(app, |env, activity| {
        let window = env
            .call_method(
                activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        if api.since::<InsetsController>().is_some() {
            let controller = env
                .call_method(
                    &window,
                    jni_str!("getInsetsController"),
                    jni_sig!("()Landroid/view/WindowInsetsController;"),
                    &[],
                )?
                .l()?;
            if !controller.is_null() {
                let appearance = if dark_icons {
                    APPEARANCE_LIGHT_STATUS_BARS
                } else {
                    0
                };
                env.call_method(
                    &controller,
                    jni_str!("setSystemBarsAppearance"),
                    jni_sig!("(II)V"),
                    &[
                        JValue::Int(appearance),
                        JValue::Int(APPEARANCE_LIGHT_STATUS_BARS),
                    ],
                )?;
                return Ok(());
            }
        }
        let decor = env
            .call_method(
                &window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let flags = env
            .call_method(
                &decor,
                jni_str!("getSystemUiVisibility"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let flags = if dark_icons {
            flags | SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
        } else {
            flags & !SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
        };
        env.call_method(
            &decor,
            jni_str!("setSystemUiVisibility"),
            jni_sig!("(I)V"),
            &[JValue::Int(flags)],
        )?;
        Ok(())
    })
}

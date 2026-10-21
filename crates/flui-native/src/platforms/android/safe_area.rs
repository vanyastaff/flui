//! Capture immutable system-bar/cutout geometry on the Java view thread.

use android_activity::AndroidApp;
use flui_foundation::geometry::{EdgeInsets, Size};
use jni::{JValue, jni_sig, jni_str};

use crate::shared::os_version::android::{AndroidApi, DisplayCutout, InsetsByType};

#[derive(Clone, Copy)]
pub(super) struct Sample {
    size: Size<i32>,
    insets: [i32; 4],
}

impl Sample {
    /// Whether this sample was taken at `size`.
    pub(super) fn is_for(self, size: Size<i32>) -> bool {
        self.size == size
    }

    pub(super) fn logical(self, size: Size<i32>, scale: f64) -> Option<EdgeInsets> {
        if self.size != size
            || size.width <= 0
            || size.height <= 0
            || !scale.is_finite()
            || scale <= 0.0
        {
            return None;
        }
        let [top, right, bottom, left] = self.insets;
        let edges = [
            f64::from(top.clamp(0, size.height)) / scale,
            f64::from(right.clamp(0, size.width)) / scale,
            f64::from(bottom.clamp(0, size.height)) / scale,
            f64::from(left.clamp(0, size.width)) / scale,
        ];
        edges
            .iter()
            .all(|edge| edge.is_finite())
            .then(|| EdgeInsets::new(edges[0], edges[1], edges[2], edges[3]))
    }
}

#[derive(Clone, Copy)]
pub(super) enum Query {
    Idle,
    Waiting,
    Ready(Option<Sample>),
}

pub(super) struct Capture {
    pub(super) accepted: EdgeInsets,
    pub(super) query: Query,
    /// A reply taken at a size the window no longer has is asked for again.
    pub(super) retry_stale: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            accepted: EdgeInsets::ZERO,
            query: Query::Idle,
            retry_stale: false,
        }
    }
}

/// Must run on Android's Java main thread. The insets are the system bars and
/// the display cutout; keyboard occlusion is not included. From API 30 they
/// are the bars as they are now, so a hidden bar (immersive or fullscreen
/// content) reserves no space. Below it the only reading without the keyboard
/// is the stable one, which keeps a hidden bar's space.
pub(super) fn sample(app: &AndroidApp, api: AndroidApi) -> Option<Sample> {
    super::preferences::with_activity(app, |env, activity| {
        let window = env
            .call_method(
                activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        let decor = env
            .call_method(
                &window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let insets = env
            .call_method(
                &decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?
            .l()?;
        if insets.is_null() {
            return Ok(None);
        }
        let width = env
            .call_method(&decor, jni_str!("getWidth"), jni_sig!("()I"), &[])?
            .i()?;
        let height = env
            .call_method(&decor, jni_str!("getHeight"), jni_sig!("()I"), &[])?
            .i()?;
        let mut edges = [0; 4];
        if api.since::<InsetsByType>().is_some() {
            let bars = env
                .call_static_method(
                    jni_str!("android/view/WindowInsets$Type"),
                    jni_str!("systemBars"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            let cutout = env
                .call_static_method(
                    jni_str!("android/view/WindowInsets$Type"),
                    jni_str!("displayCutout"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            let system = env
                .call_method(
                    &insets,
                    jni_str!("getInsets"),
                    jni_sig!("(I)Landroid/graphics/Insets;"),
                    &[JValue::Int(bars | cutout)],
                )?
                .l()?;
            for (edge, name) in edges.iter_mut().zip([
                jni_str!("top"),
                jni_str!("right"),
                jni_str!("bottom"),
                jni_str!("left"),
            ]) {
                *edge = env.get_field(&system, name, jni_sig!("I"))?.i()?;
            }
        } else {
            for (edge, name) in edges.iter_mut().zip([
                jni_str!("getStableInsetTop"),
                jni_str!("getStableInsetRight"),
                jni_str!("getStableInsetBottom"),
                jni_str!("getStableInsetLeft"),
            ]) {
                *edge = env.call_method(&insets, name, jni_sig!("()I"), &[])?.i()?;
            }
        }
        if api.since::<DisplayCutout>().is_some() && api.since::<InsetsByType>().is_none() {
            let cutout = env
                .call_method(
                    &insets,
                    jni_str!("getDisplayCutout"),
                    jni_sig!("()Landroid/view/DisplayCutout;"),
                    &[],
                )?
                .l()?;
            if !cutout.is_null() {
                for (edge, name) in edges.iter_mut().zip([
                    jni_str!("getSafeInsetTop"),
                    jni_str!("getSafeInsetRight"),
                    jni_str!("getSafeInsetBottom"),
                    jni_str!("getSafeInsetLeft"),
                ]) {
                    *edge = (*edge).max(env.call_method(&cutout, name, jni_sig!("()I"), &[])?.i()?);
                }
            }
        }
        Ok(Some(Sample {
            size: Size::new(width, height),
            insets: edges,
        }))
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_safe_area_native_metrics() {
        let size = Size::new(1080, 2400);
        let sample = Sample {
            size,
            insets: [128, 0, 63, 0],
        };
        assert_eq!(
            sample.logical(size, 2.625),
            Some(EdgeInsets::new(128.0 / 2.625, 0.0, 24.0, 0.0))
        );
        assert!(
            sample.logical(Size::new(2400, 1080), 2.625).is_none(),
            "an old portrait reply cannot enter a landscape frame"
        );
        for scale in [0.0, f64::INFINITY, f64::NAN, f64::MIN_POSITIVE / 100.0] {
            assert!(sample.logical(size, scale).is_none());
        }
    }
}

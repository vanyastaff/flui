//! Hand system-gesture exclusion rectangles to the Activity's decor view
//! (`View.setSystemGestureExclusionRects`, API 29+).

use android_activity::AndroidApp;
use jni::{JValue, jni_sig, jni_str};

/// Replaces the decor view's exclusion rectangles with `rects` (`[left, top, right,
/// bottom]` in the view's pixels). Must run on Android's Java main thread, which owns
/// the view hierarchy.
pub(super) fn apply(
    app: &AndroidApp,
    rects: &[[i32; 4]],
) -> Result<(), crate::PreferenceQueryError> {
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
        let capacity = i32::try_from(rects.len()).unwrap_or(i32::MAX);
        let list = env.new_object(
            jni_str!("java/util/ArrayList"),
            jni_sig!("(I)V"),
            &[JValue::Int(capacity)],
        )?;
        for &[left, top, right, bottom] in rects {
            let rect = env.new_object(
                jni_str!("android/graphics/Rect"),
                jni_sig!("(IIII)V"),
                &[
                    JValue::Int(left),
                    JValue::Int(top),
                    JValue::Int(right),
                    JValue::Int(bottom),
                ],
            )?;
            env.call_method(
                &list,
                jni_str!("add"),
                jni_sig!("(Ljava/lang/Object;)Z"),
                &[JValue::Object(&rect)],
            )?;
        }
        env.call_method(
            &decor,
            jni_str!("setSystemGestureExclusionRects"),
            jni_sig!("(Ljava/util/List;)V"),
            &[JValue::Object(&list)],
        )?;
        Ok(())
    })
}

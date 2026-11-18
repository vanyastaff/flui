//! The message box: an `AlertDialog` built, shown and dismissed in jobs on
//! Java's main thread, whose three listeners are one proxy.

use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{Activity, Completion, CompletionSource, MainThreadRefused};
use jni::objects::{Global, JObject};
use jni::{Env, JValue, jni_sig, jni_str};

use super::super::message_box::{
    BoxEvent, BoxLayout, Dismissal, body, dismiss_until_landed, event_of,
};
use super::{CallbackSlot, Java, in_frame};
use crate::dialog::{MessageBoxRequest, MessageLevel};

/// A message box between the owner thread, which may let it go, and Java's
/// main thread, which shows and dismisses it.
#[derive(Default)]
pub(crate) struct BoxOnScreen(Mutex<BoxState>);

#[derive(Default)]
struct BoxState {
    /// The dialog, once it is shown, until it is dismissed.
    dialog: Option<Global<JObject<'static>>>,
    /// The box's hold was dropped: a box not shown yet is not shown.
    let_go: bool,
}

impl BoxOnScreen {
    fn state(&self) -> std::sync::MutexGuard<'_, BoxState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Java {
    /// The `AlertDialog` of `request`, built and shown; its buttons, a
    /// back press and its dismissal call `listener`.
    fn show_dialog(
        &self,
        env: &mut Env<'_>,
        activity: &JObject<'_>,
        request: &MessageBoxRequest,
        layout: &BoxLayout,
        listener: &Arc<CallbackSlot>,
    ) -> jni::errors::Result<Global<JObject<'static>>> {
        let listener = self.proxy(
            env,
            &[
                jni_str!("android/content/DialogInterface$OnClickListener"),
                jni_str!("android/content/DialogInterface$OnCancelListener"),
                jni_str!("android/content/DialogInterface$OnDismissListener"),
            ],
            listener,
        )?;
        let builder = env.new_object(
            jni_str!("android/app/AlertDialog$Builder"),
            jni_sig!("(Landroid/content/Context;)V"),
            &[JValue::Object(activity)],
        )?;
        let message = env.new_string(body(request))?;
        env.call_method(
            &builder,
            jni_str!("setMessage"),
            jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;"),
            &[JValue::Object(&message)],
        )?;
        if let Some(title) = &request.title {
            let title = env.new_string(title)?;
            env.call_method(
                &builder,
                jni_str!("setTitle"),
                jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;"),
                &[JValue::Object(&title)],
            )?;
        }
        let icon = match request.level {
            MessageLevel::Info => jni_str!("ic_dialog_info"),
            MessageLevel::Warning | MessageLevel::Error => jni_str!("ic_dialog_alert"),
        };
        let icon = env
            .get_static_field(jni_str!("android/R$drawable"), icon, jni_sig!("I"))?
            .i()?;
        env.call_method(
            &builder,
            jni_str!("setIcon"),
            jni_sig!("(I)Landroid/app/AlertDialog$Builder;"),
            &[JValue::Int(icon)],
        )?;
        let setters = [
            jni_str!("setPositiveButton"),
            jni_str!("setNegativeButton"),
            jni_str!("setNeutralButton"),
        ];
        for (label, setter) in layout.labels(request).into_iter().zip(setters) {
            let Some(label) = label else {
                continue;
            };
            let label = env.new_string(label)?;
            env.call_method(
                &builder,
                setter,
                jni_sig!(
                    "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;"
                ),
                &[JValue::Object(&label), JValue::Object(&listener)],
            )?;
        }
        env.call_method(
            &builder,
            jni_str!("setOnCancelListener"),
            jni_sig!(
                "(Landroid/content/DialogInterface$OnCancelListener;)Landroid/app/AlertDialog$Builder;"
            ),
            &[JValue::Object(&listener)],
        )?;
        env.call_method(
            &builder,
            jni_str!("setOnDismissListener"),
            jni_sig!(
                "(Landroid/content/DialogInterface$OnDismissListener;)Landroid/app/AlertDialog$Builder;"
            ),
            &[JValue::Object(&listener)],
        )?;
        let dialog = env
            .call_method(
                &builder,
                jni_str!("create"),
                jni_sig!("()Landroid/app/AlertDialog;"),
                &[],
            )?
            .l()?;
        let leavable = JValue::Bool(layout.is_leavable());
        env.call_method(
            &dialog,
            jni_str!("setCancelable"),
            jni_sig!("(Z)V"),
            &[leavable],
        )?;
        env.call_method(
            &dialog,
            jni_str!("setCanceledOnTouchOutside"),
            jni_sig!("(Z)V"),
            &[leavable],
        )?;
        let shown = env.new_global_ref(&dialog)?;
        env.call_method(&dialog, jni_str!("show"), jni_sig!("()V"), &[])?;
        // The default answer's button takes the focus once the dialog is up
        // (its buttons exist only then), so Return presses it.
        if let Some(which) = layout.button_of(request.default_answer()) {
            let button = env
                .call_method(
                    &dialog,
                    jni_str!("getButton"),
                    jni_sig!("(I)Landroid/widget/Button;"),
                    &[JValue::Int(which)],
                )?
                .l()?;
            if !button.is_null() {
                env.call_method(&button, jni_str!("requestFocus"), jni_sig!("()Z"), &[])?;
            }
        }
        Ok(shown)
    }

    /// Ask for the message box of `request` on Java's main thread, where it
    /// is shown unless `on_screen` was let go meanwhile; `heard` hears each
    /// event of the box, `NotShown` when it could not be built. The
    /// completion says whether the host ran the work there at all.
    pub(crate) fn show_box(
        self: &Arc<Self>,
        activity: &Activity,
        completions: &CompletionSource,
        request: MessageBoxRequest,
        layout: BoxLayout,
        on_screen: &Arc<BoxOnScreen>,
        heard: impl Fn(BoxEvent) + Send + Sync + 'static,
    ) -> Completion<Result<(), MainThreadRefused>> {
        let java = Arc::clone(self);
        let on_screen = Arc::clone(on_screen);
        activity.on_main_thread(completions, move |frame| {
            if on_screen.state().let_go {
                return;
            }
            let heard = Arc::new(heard);
            let by_listener = Arc::clone(&heard);
            let listener = CallbackSlot::new(move |_, method, which| {
                if let Some(event) = event_of(method, which) {
                    by_listener(event);
                }
            });
            let shown = in_frame(frame, |env, activity| {
                let shown = java.show_dialog(env, activity, &request, &layout, &listener);
                if shown.is_err() {
                    env.exception_clear();
                }
                Ok(shown.ok())
            });
            if let Ok(Some(dialog)) = shown {
                on_screen.state().dialog = Some(dialog);
            } else {
                heard(BoxEvent::NotShown);
            }
        })
    }

    /// Let the box of `on_screen` go: one not shown yet is not shown, and
    /// one on screen is dismissed on Java's main thread.
    pub(crate) fn dismiss_box(
        activity: &Activity,
        completions: &CompletionSource,
        on_screen: &Arc<BoxOnScreen>,
    ) {
        on_screen.state().let_go = true;
        let on_screen = Arc::clone(on_screen);
        let make: Dismissal = std::rc::Rc::new(move || {
            let on_screen = Arc::clone(&on_screen);
            Box::new(move |frame| {
                let dialog = on_screen.state().dialog.take();
                if let Some(dialog) = dialog {
                    let _ = in_frame(frame, |env, _| {
                        let dismissed =
                            env.call_method(&dialog, jni_str!("dismiss"), jni_sig!("()V"), &[]);
                        if dismissed.is_err() {
                            env.exception_clear();
                        }
                        Ok(())
                    });
                }
            })
        });
        dismiss_until_landed(activity, completions, make);
    }
}

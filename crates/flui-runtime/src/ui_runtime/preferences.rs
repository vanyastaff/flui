use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use super::UiRuntime;
use crate::{owner::SystemPreferencesSnapshot, presentation::PresentationState};

pub(super) fn publish(presentation: &PresentationState, snapshot: &SystemPreferencesSnapshot) {
    // Commit admission policy before inherited publication can wake or reenter
    // user code. A later FIFO Down reads this source even without a frame.
    let mut settings = presentation.gestures().default_settings().clone();
    if let Some(timeout) = snapshot.values.gestures().long_press_timeout() {
        settings = settings.with_long_press_timeout(timeout);
    }
    presentation.gesture_settings.replace(settings);
    presentation
        .wheel_preferences
        .replace(snapshot.values.wheel().clone());
    presentation.media_query.update(|data| {
        data.text_scale_factor = snapshot.values.text_scale().unwrap_or(1.0);
        data.high_contrast = snapshot.values.high_contrast().unwrap_or(false);
    });
}

impl UiRuntime {
    pub(crate) fn preferences_belong_to(&self, origin: &Rc<()>) -> bool {
        self.preferences
            .borrow()
            .as_ref()
            .is_none_or(|current| Rc::ptr_eq(&current.origin, origin))
    }

    pub(crate) fn apply_preferences(&self, snapshot: SystemPreferencesSnapshot) {
        {
            let current = self.preferences.borrow();
            if let Some(current) = current.as_ref() {
                assert!(
                    Rc::ptr_eq(&current.origin, &snapshot.origin),
                    "BUG: runtime received another host's preference source"
                );
                if current.revision >= snapshot.revision {
                    return;
                }
            }
        }
        *self.preferences.borrow_mut() = Some(snapshot.clone());
        let mut first_failure = None;
        for presentation in self.presentations.iter() {
            if let Err(failure) =
                catch_unwind(AssertUnwindSafe(|| publish(presentation, &snapshot)))
            {
                crate::lifecycle_state::preserve_first_lifecycle_panic(
                    &mut first_failure,
                    Some(failure),
                    "system preferences publication",
                );
            }
        }
        if let Some(failure) = first_failure {
            resume_unwind(failure);
        }
    }
}

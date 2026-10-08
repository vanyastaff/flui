use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    time::Duration,
};

use flui_foundation::{MonotonicClock, geometry::DevicePixelRatio};
use flui_interaction::GestureSettings;
use flui_platform_api::{GestureGeometry, PreferenceQueryError, SystemPreferences};
use flui_rendering::binding::RendererBinding as _;

use super::UiRuntime;
use crate::{owner::SystemPreferencesSnapshot, presentation::PresentationState};

#[derive(Debug)]
pub(crate) struct GeometryProjection {
    context: DevicePixelRatio,
    accepted: Option<GestureGeometry>,
    barrier: Rc<()>,
    pending: Option<QueryDebt>,
}

impl Default for GeometryProjection {
    fn default() -> Self {
        Self {
            context: DevicePixelRatio::ONE,
            accepted: None,
            barrier: Rc::new(()),
            pending: None,
        }
    }
}

#[derive(Clone, Debug)]
struct QueryDebt {
    barrier: Rc<()>,
    deadline: web_time::Instant,
    delay: Duration,
}

impl GeometryProjection {
    pub(crate) fn next_wake(&self) -> Option<web_time::Instant> {
        self.pending.as_ref().map(|debt| debt.deadline)
    }

    fn geometry(&self) -> Option<GestureGeometry> {
        self.accepted
            .filter(|geometry| geometry.pixel_ratio() == self.context)
    }
}

pub(super) fn publish(
    presentation: &PresentationState,
    values: &SystemPreferences,
    now: web_time::Instant,
) {
    refresh(presentation, values, now, true);
}

fn refresh(
    presentation: &PresentationState,
    values: &SystemPreferences,
    now: web_time::Instant,
    barrier: bool,
) {
    if presentation.closing_requested.get() {
        return;
    }
    let context = presentation.renderer().root_pipeline_owner().with(|owner| {
        DevicePixelRatio::new(owner.device_pixel_ratio())
            .expect("BUG: renderer accepted invalid pixel ratio")
    });
    let attempt = {
        let mut state = presentation.gesture_geometry.borrow_mut();
        if barrier {
            // The in-flight attempt keeps the previous identity alive, so a
            // replacement cannot alias it even if allocator addresses recycle.
            let identity = Rc::new(());
            state.context = context;
            state.barrier = Rc::clone(&identity);
            let debt = QueryDebt {
                barrier: identity,
                deadline: now + Duration::from_millis(100),
                delay: Duration::from_millis(100),
            };
            state.pending = Some(debt.clone());
            debt
        } else {
            let Some(mut debt) = state
                .pending
                .as_ref()
                .filter(|debt| debt.deadline <= now)
                .cloned()
            else {
                return;
            };
            debt.delay = (debt.delay * 2).min(Duration::from_secs(1));
            debt.deadline = now + debt.delay;
            state.pending = Some(debt.clone());
            debt
        }
    };
    // Commit retry debt before native/user code; never lend the projection cell
    // or the preference snapshot's RefCell through this boundary.
    let query = catch_unwind(AssertUnwindSafe(|| {
        presentation
            .with_window(|window| window.gesture_geometry())
            .unwrap_or(Err(PreferenceQueryError::Unavailable))
    }));
    let mut first_failure = None;
    let mut diagnostic = None;
    let geometry = {
        let mut state = presentation.gesture_geometry.borrow_mut();
        if !Rc::ptr_eq(&state.barrier, &attempt.barrier) || presentation.closing_requested.get() {
            drop(state);
            if let Err(failure) = query {
                resume_unwind(failure);
            }
            return;
        }
        match query {
            Ok(Ok(geometry))
                if geometry.is_none_or(|geometry| geometry.pixel_ratio() == context) =>
            {
                // A successful query is acknowledged independently of the pure
                // projection. Re-reading an unrepresentable value cannot fix it.
                state.pending = None;
                match GestureSettings::resolve_preferences(
                    presentation.gestures().default_settings(),
                    values.gestures(),
                    geometry.as_ref(),
                ) {
                    Ok(_) => state.accepted = geometry,
                    Err(error) => diagnostic = Some(error.to_string()),
                }
            }
            Ok(Ok(_)) => {
                diagnostic =
                    Some("native gesture geometry belongs to another pixel ratio".to_owned())
            }
            Ok(Err(error)) => diagnostic = Some(error.to_string()),
            Err(failure) => first_failure = Some(failure),
        }
        state.geometry()
    };
    let settings = GestureSettings::resolve_preferences(
        presentation.gestures().default_settings(),
        values.gestures(),
        geometry.as_ref(),
    )
    .expect("BUG: retained gesture geometry was previously projected successfully");
    presentation.gesture_settings.replace(settings);
    presentation
        .wheel_preferences
        .replace(values.wheel().clone());
    if let Some(diagnostic) = diagnostic {
        let failure = catch_unwind(AssertUnwindSafe(
            || tracing::warn!(%diagnostic, "gesture preference projection retained its fallback"),
        ))
        .err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_failure,
            failure,
            "gesture preference diagnostic",
        );
    }
    let current = Rc::ptr_eq(
        &presentation.gesture_geometry.borrow().barrier,
        &attempt.barrier,
    ) && !presentation.closing_requested.get();
    if barrier && current {
        let failure = catch_unwind(AssertUnwindSafe(|| {
            presentation.media_query.update(|data| {
                data.text_scale_factor = values.text_scale().unwrap_or(1.0);
                data.high_contrast = values.high_contrast().unwrap_or(false);
            })
        }))
        .err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_failure,
            failure,
            "system preferences publication",
        );
    }
    if let Some(failure) = first_failure {
        resume_unwind(failure);
    }
}

impl UiRuntime {
    pub(crate) fn refresh_gesture_context_for(&self, id: flui_foundation::PresentationId) {
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        let context = presentation.renderer().root_pipeline_owner().with(|owner| {
            DevicePixelRatio::new(owner.device_pixel_ratio())
                .expect("BUG: renderer accepted invalid pixel ratio")
        });
        if presentation.gesture_geometry.borrow().context == context {
            return;
        }
        let values = self
            .preferences
            .borrow()
            .as_ref()
            .map(|snapshot| std::sync::Arc::clone(&snapshot.values));
        let unknown = SystemPreferences::default();
        refresh(
            presentation,
            values.as_deref().unwrap_or(&unknown),
            self.clock.now(),
            true,
        );
    }

    pub(super) fn service_gesture_geometry(&self) {
        let values = self
            .preferences
            .borrow()
            .as_ref()
            .map(|snapshot| std::sync::Arc::clone(&snapshot.values));
        let unknown = SystemPreferences::default();
        let now = self.clock.now();
        let mut first_failure = None;
        for presentation in self.presentations.iter() {
            let failure = catch_unwind(AssertUnwindSafe(|| {
                refresh(
                    presentation,
                    values.as_deref().unwrap_or(&unknown),
                    now,
                    false,
                )
            }))
            .err();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first_failure,
                failure,
                "gesture geometry retry",
            );
        }
        if let Some(failure) = first_failure {
            resume_unwind(failure);
        }
    }

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
            if let Err(failure) = catch_unwind(AssertUnwindSafe(|| {
                publish(presentation, &snapshot.values, self.clock.now())
            })) {
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

use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    time::Duration,
};

use flui_foundation::{MonotonicClock, PresentationId, geometry::DevicePixelRatio};
use flui_interaction::GestureSettings;
use flui_platform_api::{
    GestureGeometry, GesturePreferences, PreferenceQueryError, SystemPreferences,
};
use flui_rendering::binding::RendererBinding as _;

use super::UiRuntime;
use crate::{owner::SystemPreferencesSnapshot, presentation::PresentationState};

#[cfg(test)]
pub(crate) mod tests;

type RetryBudget = Rc<RefCell<Option<Vec<PresentationId>>>>;

/// An owner operation and its nested gates/pumps share one retry budget. The
/// owned guard leaves no borrow on the runtime while the host drives a frame.
pub(crate) enum GeometryTurn {
    Owner(RetryBudget),
    Joined(RetryBudget),
}

impl GeometryTurn {
    fn admit(&self, id: PresentationId) -> bool {
        let (Self::Owner(budget) | Self::Joined(budget)) = self;
        let mut turn = budget.borrow_mut();
        let attempts = turn
            .as_mut()
            .expect("BUG: geometry turn guard outlived its owner");
        if attempts.contains(&id) {
            return false;
        }
        attempts.push(id);
        true
    }
}

impl Drop for GeometryTurn {
    fn drop(&mut self) {
        if let Self::Owner(budget) = self {
            // Only framework IDs retire here, with no user code or runtime borrow.
            budget.borrow_mut().take();
        }
    }
}

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

    /// Acknowledgement and projection acceptance are separate: a successful
    /// exact query settles delivery even when its deterministic projection fails.
    fn accept(
        &mut self,
        baseline: &GestureSettings,
        preferences: &GesturePreferences,
        query: Result<Option<GestureGeometry>, PreferenceQueryError>,
    ) -> Option<String> {
        match query {
            Ok(geometry)
                if geometry.is_none_or(|geometry| geometry.pixel_ratio() == self.context) =>
            {
                self.pending = None;
                match GestureSettings::resolve_preferences(baseline, preferences, geometry.as_ref())
                {
                    Ok(_) => {
                        self.accepted = geometry;
                        None
                    }
                    Err(error) => Some(error.to_string()),
                }
            }
            Ok(_) => Some("native gesture geometry belongs to another pixel ratio".to_owned()),
            Err(error) => Some(error.to_string()),
        }
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
            Ok(query) => {
                diagnostic = state.accept(
                    presentation.gestures().default_settings(),
                    values.gestures(),
                    query,
                );
            }
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
            });
        }))
        .err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_failure,
            failure,
            "system preferences publication",
        );
    }
    // A slow failed getter/diagnostic must not return an already overdue retry.
    // This pacing is separate from the structural per-operation turn budget.
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let completed = MonotonicClock::now(presentation.clock().source());
        let mut state = presentation.gesture_geometry.borrow_mut();
        if let Some(debt) = state
            .pending
            .as_mut()
            .filter(|debt| Rc::ptr_eq(&debt.barrier, &attempt.barrier))
        {
            debt.deadline = debt.deadline.max(completed + debt.delay);
        }
    }))
    .err();
    crate::lifecycle_state::preserve_first_lifecycle_panic(
        &mut first_failure,
        failure,
        "gesture geometry retry pacing",
    );
    if let Some(failure) = first_failure {
        resume_unwind(failure);
    }
}

impl UiRuntime {
    pub(crate) fn begin_geometry_turn(&self) -> GeometryTurn {
        let mut turn = self.geometry_turn.borrow_mut();
        if turn.is_some() {
            GeometryTurn::Joined(Rc::clone(&self.geometry_turn))
        } else {
            *turn = Some(Vec::new());
            GeometryTurn::Owner(Rc::clone(&self.geometry_turn))
        }
    }
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

    pub(super) fn service_gesture_geometry(&self, turn: &GeometryTurn) {
        let values = self
            .preferences
            .borrow()
            .as_ref()
            .map(|snapshot| std::sync::Arc::clone(&snapshot.values));
        let unknown = SystemPreferences::default();
        let now = self.clock.now();
        let mut first_failure = None;
        for presentation in self.presentations.iter() {
            let due = !presentation.closing_requested.get()
                && presentation
                    .gesture_geometry
                    .borrow()
                    .next_wake()
                    .is_some_and(|deadline| deadline <= now);
            if !due || !turn.admit(presentation.id()) {
                continue;
            }
            let failure = catch_unwind(AssertUnwindSafe(|| {
                refresh(
                    presentation,
                    values.as_deref().unwrap_or(&unknown),
                    now,
                    false,
                );
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
                publish(presentation, &snapshot.values, self.clock.now());
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

#[cfg(test)]
pub(crate) fn checked_geometry_refusal_acknowledges_the_query_and_keeps_safe_admission() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        GestureArena, GestureRecognizer, TapGestureRecognizer, routing::PointerDispatch,
    };
    use flui_interaction::{
        events::{PointerEventExt, PointerKind},
        testing::input::{pointer_down, pointer_move, pointer_up},
    };
    use flui_platform_api::Distance;
    use std::cell::Cell;
    let baseline = GestureSettings::default()
        .try_with_touch_slop(10.0)
        .expect("hit slop")
        .try_with_pan_slop(20.0)
        .expect("pan ratio two");
    let old_geometry = GestureGeometry::new(DevicePixelRatio::ONE)
        .with_touch_slop(Distance::new(2.0).expect("touch slop"));
    let preferences =
        GesturePreferences::default().with_long_press_timeout(Duration::from_millis(200));
    for changed_context in [false, true] {
        let mut state = GeometryProjection::default();
        assert!(
            state
                .accept(
                    &baseline,
                    &GesturePreferences::default(),
                    Ok(Some(old_geometry))
                )
                .is_none()
        );
        if changed_context {
            state.context = DevicePixelRatio::new(2.0).expect("ratio");
        }
        state.pending = Some(QueryDebt {
            barrier: Rc::clone(&state.barrier),
            deadline: web_time::Instant::now(),
            delay: Duration::from_millis(100),
        });
        let refused = GestureGeometry::new(state.context)
            .with_touch_slop(Distance::new(f64::MAX).expect("finite native distance"));
        assert!(
            state
                .accept(&baseline, &preferences, Ok(Some(refused)))
                .is_some(),
            "ratio multiplication must refuse the otherwise valid native reading"
        );
        assert!(
            state.next_wake().is_none(),
            "successful read cannot leave a deterministic retry loop"
        );
        let settings = GestureSettings::resolve_preferences(
            &baseline,
            &preferences,
            state.geometry().as_ref(),
        )
        .expect("safe fallback");
        assert_eq!(
            settings.long_press_timeout(),
            Duration::from_millis(200),
            "geometry refusal cannot roll accepted timing back"
        );
        let taps = Rc::new(Cell::new(0));
        let output = Rc::clone(&taps);
        let arena = GestureArena::new();
        let tap = TapGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_tap(move |_| output.set(output.get() + 1))
            .build();
        let down = pointer_down(Offset::new(40.0, 40.0), PointerKind::Touch).expect("down");
        tap.add_pointer(PointerDispatch::at_root(&down));
        arena.close(down.pointer_id().expect("contact"));
        arena.drain_deferred_resolutions();
        for event in [
            pointer_move(Offset::new(45.0, 40.0), PointerKind::Touch).expect("move"),
            pointer_up(Offset::new(45.0, 40.0), PointerKind::Touch).expect("up"),
        ] {
            tap.handle_event(PointerDispatch::at_root(&event));
            arena.drain_deferred_resolutions();
        }
        assert_eq!(
            taps.get(),
            usize::from(changed_context),
            "same context retains native2; changed context admits baseline10 without publishing a partial overflowing profile"
        );
        assert!(state.accept(&baseline, &preferences, Ok(None)).is_none());
        let restored = GestureSettings::resolve_preferences(
            &baseline,
            &preferences,
            state.geometry().as_ref(),
        )
        .expect("restored baseline");
        assert_eq!(
            restored,
            baseline
                .clone()
                .with_long_press_timeout(Duration::from_millis(200)),
            "healthy absence restores the entire baseline with latest timing"
        );
    }
}

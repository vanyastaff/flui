//! Presentation-owned gesture arbitration and inherited input policy.

use flui_interaction::arena::{GestureArena, SweepModel};
use flui_interaction::{GestureSettingsProvider, WheelPreferencesProvider};
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Shares one binding-driven arena and its input policy with descendants.
///
/// The presentation installs its live gesture and wheel providers at the root.
/// An authored nested scope inherits both policies unless explicitly overridden:
/// `.settings(...)` overrides gestures independently of wheel observations.
/// Without an outer scope, absent policies use framework defaults.
///
/// This composition widget resolves inheritance before publishing one inherited
/// node containing the arena and both policies. Use [`Self::of`] to acquire the
/// arena; the public configuration is not itself an [`InheritedView`].
#[derive(Clone, StatelessView)]
pub struct GestureArenaScope {
    arena: GestureArena,
    settings: Option<GestureSettingsProvider>,
    wheel_preferences: Option<WheelPreferencesProvider>,
    child: BoxedView,
}

impl GestureArenaScope {
    /// Wrap `child` with the presentation's shared arena.
    ///
    /// # Panics
    /// Panics if `arena` is not binding-driven. A scope never creates a second
    /// owner of the arena's close/sweep lifecycle.
    #[must_use]
    pub fn new(arena: GestureArena, child: impl IntoView) -> Self {
        assert_eq!(
            arena.sweep_model(),
            SweepModel::BindingDriven,
            "BUG: GestureArenaScope requires a BindingDriven arena owned by the presentation \
             binding; SelfDriven arenas cannot back a presentation scope"
        );
        Self {
            arena,
            settings: None,
            wheel_preferences: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Explicitly override gesture policy, retaining inherited wheel policy.
    ///
    /// An authored [`flui_interaction::GestureSettings`] becomes a fixed profile;
    /// a presentation supplies its live provider. New admissions read the
    /// provider while active contacts retain their admitted settings.
    #[must_use]
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = Some(settings.into());
        self
    }

    /// Explicitly override wheel observations independently of gesture policy.
    ///
    /// Leaving this unset inherits the outer scope's exact provider, including
    /// future host updates. An authored fixed value replaces that inheritance.
    #[must_use]
    pub fn wheel_preferences(mut self, preferences: impl Into<WheelPreferencesProvider>) -> Self {
        self.wheel_preferences = Some(preferences.into());
        self
    }

    pub(crate) fn settings_of(ctx: &dyn BuildContext) -> GestureSettingsProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.settings.clone())
            .expect("BUG: gesture consumers must acquire settings beneath GestureArenaScope")
    }

    pub(crate) fn wheel_preferences_of(ctx: &dyn BuildContext) -> WheelPreferencesProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.wheel_preferences.clone())
            .expect("BUG: wheel consumers must acquire preferences beneath GestureArenaScope")
    }

    /// Acquire the exact arena without registering an inherited dependency.
    ///
    /// # Panics
    /// Panics outside a presentation scope. Consumers never create a private
    /// arena or take over the binding's lifecycle.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> GestureArena {
        ctx.get::<ResolvedGestureArenaScope, _>(|scope| scope.data.arena.clone())
            .expect(
                "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
            )
    }

    /// The shared arena configured for this scope.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }
}

impl std::fmt::Debug for GestureArenaScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScope")
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

impl StatelessView for GestureArenaScope {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let settings = self.settings.clone().unwrap_or_else(|| {
            ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.settings.clone())
                .unwrap_or_default()
        });
        let wheel_preferences = self.wheel_preferences.clone().unwrap_or_else(|| {
            ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| {
                scope.data.wheel_preferences.clone()
            })
            .unwrap_or_default()
        });
        ResolvedGestureArenaScope {
            data: GestureScopeData {
                arena: self.arena.clone(),
                settings,
                wheel_preferences,
            },
            child: self.child.clone(),
        }
    }
}

#[derive(Clone)]
struct GestureScopeData {
    arena: GestureArena,
    settings: GestureSettingsProvider,
    wheel_preferences: WheelPreferencesProvider,
}

#[derive(Clone)]
struct ResolvedGestureArenaScope {
    data: GestureScopeData,
    child: BoxedView,
}

impl InheritedView for ResolvedGestureArenaScope {
    type Data = GestureScopeData;
    fn data(&self) -> &Self::Data {
        &self.data
    }
    fn child(&self) -> &dyn View {
        &self.child
    }
    fn update_should_notify(&self, old: &Self) -> bool {
        self.data.settings != old.data.settings
            || self.data.wheel_preferences != old.data.wheel_preferences
    }
}

impl_inherited_view!(ResolvedGestureArenaScope);

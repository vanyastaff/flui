//! [`GestureArenaScope`] and [`GesturePolicy`]: the gesture arena and the inherited input policy.
//!
//! The presentation installs its live arena once; every recognizer below
//! acquires exactly that arena through [`GestureArenaScope`], and the scope
//! publishes the resolved settings as one inherited node.

use std::rc::Rc;

use flui_interaction::gesture::{GestureArena, SweepModel};
use flui_interaction::{gesture::GestureSettingsProvider, pointer::WheelPreferencesProvider};

use crate::impl_inherited_view;
use crate::prelude::*;

/// Shares one binding-driven arena and its input policy with descendants.
///
/// The presentation installs its live gesture and wheel providers at the root.
/// An authored nested scope inherits both policies unless explicitly overridden:
/// `.settings(...)` overrides gestures independently of wheel observations.
/// Without an outer scope, absent policies use framework defaults.
///
/// This composition widget resolves inheritance before publishing one inherited
/// node containing the arena and both policies. Use [`Self::of`] to acquire the
/// arena; the resolved form is the [`InheritedView`], not this configuration.
#[derive(Clone, StatefulView)]
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
            child: BoxedView::new(child.into_view()),
        }
    }

    /// Explicitly override gesture policy, retaining inherited wheel policy.
    ///
    /// An authored [`flui_interaction::gesture::GestureSettings`] becomes a fixed profile;
    /// a presentation supplies its live provider. New admissions read the
    /// provider while active contacts retain their admitted settings. Beneath
    /// an outer scope, the outer provider's accessibility timing floors still
    /// raise a shorter authored long-press or double-tap timing.
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

    /// The gesture settings the nearest scope has resolved for consumers of
    /// gesture data.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`]: a settings consumer is a
    /// gesture recognizer and only mounts beneath the presentation's scope.
    #[must_use]
    pub fn settings_of(ctx: &dyn BuildContext) -> GestureSettingsProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.settings().clone())
            .expect("BUG: gesture consumers must acquire settings beneath GestureArenaScope")
    }

    /// The scope's gesture settings, or the framework defaults outside one.
    #[must_use]
    pub fn settings_or_default_of(ctx: &dyn BuildContext) -> GestureSettingsProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.settings().clone())
            .unwrap_or_default()
    }

    /// The wheel observations the nearest scope has resolved. Wheel handlers
    /// that admit by pointer kind or min-target read them.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`]: a wheel consumer is a gesture
    /// recognizer and only mounts beneath the presentation's scope.
    #[must_use]
    pub fn wheel_preferences_of(ctx: &dyn BuildContext) -> WheelPreferencesProvider {
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
        // Compositions built from the handle are checked against the scope's
        // gesture policy (ADR-0208 delay ceiling).
        ctx.get::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
            .expect(
                "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
            )
    }

    /// [`Self::of`], registering a dependency: the caller's
    /// `did_change_dependencies` runs when the scope hands out a handle that
    /// admits elsewhere or another policy.
    ///
    /// # Panics
    /// Panics outside a presentation scope, as [`Self::of`] does.
    pub fn dependent_of(ctx: &dyn BuildContext) -> GestureArena {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
            .expect(
                "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
            )
    }

    /// [`Self::of`], or `None` outside a presentation scope.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<GestureArena> {
        ctx.get::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
    }

    /// The shared arena configured for this scope.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }
}

/// The arena handle and the gesture settings a widget builds its recognizers
/// for. Equal policies admit alike under the same settings, so recognizers
/// built for one serve the other.
///
/// A scope resolves its policy once and every consumer beneath it shares
/// that value, so holding or cloning a policy costs one reference.
#[derive(Clone, Debug)]
pub struct GesturePolicy(Rc<PolicyParts>);

#[derive(Debug)]
struct PolicyParts {
    arena: GestureArena,
    settings: GestureSettingsProvider,
}

impl GesturePolicy {
    fn new(arena: &GestureArena, settings: GestureSettingsProvider) -> Self {
        Self(Rc::new(PolicyParts {
            // Compositions built from the handle are checked against the
            // scope's gesture policy (ADR-0208 delay ceiling).
            arena: arena.with_settings(settings.clone()),
            settings,
        }))
    }

    /// The scope's policy. Reading it depends on the scope, so the caller's
    /// `did_change_dependencies` runs when the scope changes.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`], as [`GestureArenaScope::of`]
    /// does.
    pub fn of(ctx: &dyn BuildContext) -> Self {
        Self::maybe_of(ctx).expect(
            "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
        )
    }

    /// [`Self::of`], or `None` outside a presentation scope.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.clone())
    }

    /// The subtree's exact arena, configured with the resolved settings.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.0.arena
    }

    /// The resolved gesture settings the recognizers built for it admit with.
    #[must_use]
    pub fn settings(&self) -> &GestureSettingsProvider {
        &self.0.settings
    }
}

impl PartialEq for GesturePolicy {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.settings == other.0.settings && self.0.arena.admits_like(&other.0.arena))
    }
}

impl std::fmt::Debug for GestureArenaScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScope")
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

/// What a mounted [`GestureArenaScope`] publishes, resolved when the scope
/// or the policies above it change. The state keeps it across rebuilds, so a
/// scope whose arena and policies have not changed hands its consumers the
/// policy they already hold.
pub struct GestureArenaScopeState {
    /// What the scope itself authors, kept to resolve again when the
    /// policies above change.
    authored: Policies,
    inherited: Policies,
    data: GestureScopeData,
}

impl std::fmt::Debug for GestureArenaScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScopeState")
            .field("policy", &self.data.policy)
            .finish_non_exhaustive()
    }
}

/// The two policies of a scope, each absent where none is given.
#[derive(Clone, Default)]
struct Policies {
    settings: Option<GestureSettingsProvider>,
    wheel_preferences: Option<WheelPreferencesProvider>,
}

impl Policies {
    /// The policies of the scope above `ctx`, registering a dependency.
    fn inherited(ctx: &dyn BuildContext) -> Self {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| Self {
            settings: Some(scope.data.policy.settings().clone()),
            wheel_preferences: Some(scope.data.wheel_preferences.clone()),
        })
        .unwrap_or_default()
    }
}

impl GestureScopeData {
    /// What a scope of `arena` publishes when it authors `authored` beneath
    /// `inherited`. `current` is what it publishes now: its policy is kept
    /// while it still says the same.
    fn resolve(
        arena: &GestureArena,
        authored: &Policies,
        inherited: &Policies,
        current: Option<&Self>,
    ) -> Self {
        // An override beneath another scope still meets the host's
        // accessibility timing floors (ADR-0212 §3).
        let settings = match (authored.settings.clone(), &inherited.settings) {
            (Some(authored), Some(inherited)) => inherited.overridden_by(authored),
            (Some(authored), None) => authored,
            (None, inherited) => inherited.clone().unwrap_or_default(),
        };
        let wheel_preferences = authored
            .wheel_preferences
            .clone()
            .or_else(|| inherited.wheel_preferences.clone())
            .unwrap_or_default();
        let policy = current
            .map(|current| &current.policy)
            .filter(|current| *current.settings() == settings && current.arena().admits_like(arena))
            .map_or_else(
                || GesturePolicy::new(arena, settings.clone()),
                GesturePolicy::clone,
            );
        Self {
            policy,
            wheel_preferences,
        }
    }
}

impl StatefulView for GestureArenaScope {
    type State = GestureArenaScopeState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let (authored, inherited) = (self.authored(), Policies::inherited(ctx));
        GestureArenaScopeState {
            data: GestureScopeData::resolve(&self.arena, &authored, &inherited, None),
            authored,
            inherited,
        }
    }
}

impl GestureArenaScope {
    fn authored(&self) -> Policies {
        Policies {
            settings: self.settings.clone(),
            wheel_preferences: self.wheel_preferences.clone(),
        }
    }
}

impl ViewState<GestureArenaScope> for GestureArenaScopeState {
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.inherited = Policies::inherited(ctx);
        self.data = GestureScopeData::resolve(
            self.data.policy.arena(),
            &self.authored,
            &self.inherited,
            Some(&self.data),
        );
    }

    fn did_update_view(&mut self, _old: &GestureArenaScope, new: &GestureArenaScope) {
        self.authored = new.authored();
        self.data = GestureScopeData::resolve(
            &new.arena,
            &self.authored,
            &self.inherited,
            Some(&self.data),
        );
    }

    fn build(&self, view: &GestureArenaScope, _ctx: &dyn BuildContext) -> impl IntoView {
        ResolvedGestureArenaScope {
            data: self.data.clone(),
            child: view.child.clone(),
        }
    }
}

/// The resolved arena and both policies one inherited node carries.
#[derive(Clone)]
struct GestureScopeData {
    /// The arena and the settings as one shared value, which consumers hold.
    policy: GesturePolicy,
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
    fn child(&self) -> &BoxedView {
        &self.child
    }
    /// A consumer's recognizers join the arena and follow the policy they were
    /// built with, so a change of either reaches every dependent.
    fn update_should_notify(&self, old: &Self) -> bool {
        self.data.policy != old.data.policy
            || self.data.wheel_preferences != old.data.wheel_preferences
    }
}

impl_inherited_view!(ResolvedGestureArenaScope);

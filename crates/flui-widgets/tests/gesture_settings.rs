use flui_interaction::{GestureArena, GestureSettingsProvider};
use flui_view::{
    BoxedView, BuildContext, IntoView, LifecycleContext, StatefulView, ViewExt, ViewState,
};

/// Installs a scope over real presentation-owned gesture admission: with an
/// authored policy, or with none, which inherits the policy above.
#[derive(Clone, flui_view::prelude::StatefulView)]
pub(crate) struct SettingsScope {
    settings: Option<GestureSettingsProvider>,
    child: BoxedView,
}

impl SettingsScope {
    pub(crate) fn new(settings: impl Into<GestureSettingsProvider>, child: impl IntoView) -> Self {
        Self {
            settings: Some(settings.into()),
            child: child.into_view().boxed(),
        }
    }

    /// A scope that authors no settings.
    pub(crate) fn inheriting(child: impl IntoView) -> Self {
        Self {
            settings: None,
            child: child.into_view().boxed(),
        }
    }
}

pub(crate) struct SettingsScopeState {
    arena: Option<GestureArena>,
}

impl StatefulView for SettingsScope {
    type State = SettingsScopeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SettingsScopeState { arena: None }
    }
}

impl ViewState<SettingsScope> for SettingsScopeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.arena = Some(flui_view::environment::GestureArenaScope::of(ctx));
    }

    fn build(&self, view: &SettingsScope, _: &dyn BuildContext) -> impl IntoView {
        let scope = flui_view::environment::GestureArenaScope::new(
            self.arena
                .as_ref()
                .expect("mounted presentation arena")
                .clone(),
            view.child.clone(),
        );
        match &view.settings {
            Some(settings) => scope.settings(settings.clone()),
            None => scope,
        }
    }
}

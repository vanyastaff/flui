use flui_interaction::{GestureArena, GestureSettingsProvider};
use flui_view::{
    BoxedView, BuildContext, IntoView, LifecycleContext, StatefulView, ViewExt, ViewState,
};

/// Installs an authored policy over real presentation-owned gesture admission.
#[derive(Clone, flui_view::prelude::StatefulView)]
pub(crate) struct SettingsScope {
    settings: GestureSettingsProvider,
    child: BoxedView,
}

impl SettingsScope {
    pub(crate) fn new(settings: impl Into<GestureSettingsProvider>, child: impl IntoView) -> Self {
        Self {
            settings: settings.into(),
            child: child.into_view().boxed(),
        }
    }
}

pub(crate) struct SettingsScopeState {
    arena: Option<GestureArena>,
}

impl StatefulView for SettingsScope {
    type State = SettingsScopeState;

    fn create_state(&self) -> Self::State {
        SettingsScopeState { arena: None }
    }
}

impl ViewState<SettingsScope> for SettingsScopeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.arena = Some(flui_widgets::GestureArenaScope::of(ctx));
    }

    fn build(&self, view: &SettingsScope, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::GestureArenaScope::new(
            self.arena
                .as_ref()
                .expect("mounted presentation arena")
                .clone(),
            view.child.clone(),
        )
        .settings(view.settings.clone())
    }
}

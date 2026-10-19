//! [`HapticsMode`]: switch haptics off for a subtree (ADR-0210 §10).

use crate::context::{BuildContext, BuildContextExt};
use crate::element::ElementKind;
use crate::view::{BoxedView, InheritedView, IntoView, StatelessView, View};

/// Switches haptic feedback off (or back on) for its subtree.
///
/// While `enabled` is `false`, every haptics handle a descendant acquires
/// through `LifecycleContext::haptics` resolves its requests, of both tiers,
/// to `HapticOutcome::Suppressed` before they reach the platform. Nesting
/// composes as an AND: a `HapticsMode` inside a disabled one cannot switch
/// haptics back on. A widget re-acquires its handle in
/// `did_change_dependencies`, which runs when an enclosing mode changes.
///
/// # Examples
///
/// ```
/// use flui_view::{BoxedView, HapticsMode};
/// # fn quiet(content: BoxedView) -> HapticsMode {
/// HapticsMode::new(content).enabled(false)
/// # }
/// ```
#[derive(Clone)]
pub struct HapticsMode {
    child: BoxedView,
    enabled: bool,
}

impl std::fmt::Debug for HapticsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HapticsMode")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl HapticsMode {
    /// A haptics switch around `child`, on by default.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            enabled: true,
        }
    }

    /// This switch set to `enabled`.
    #[must_use]
    pub fn enabled(self, enabled: bool) -> Self {
        Self { enabled, ..self }
    }

    /// Depend on whether the enclosing switches leave haptics on; `true`
    /// without one.
    #[must_use]
    pub fn is_enabled(ctx: &(impl BuildContext + ?Sized)) -> bool {
        ctx.depend_on::<HapticsModeScope, _>(|scope| scope.enabled)
            .unwrap_or(true)
    }
}

impl StatelessView for HapticsMode {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        HapticsModeScope {
            enabled: Self::is_enabled(ctx) && self.enabled,
            child: self.child.clone(),
        }
    }
}

impl View for HapticsMode {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// `handle` as the element behind `ctx` may use it: switched off under a
/// disabled [`HapticsMode`], with a dependency on the enclosing mode.
pub(crate) fn scoped(
    ctx: &(impl BuildContext + ?Sized),
    handle: flui_interaction::HapticsHandle,
) -> flui_interaction::HapticsHandle {
    if HapticsMode::is_enabled(ctx) {
        handle
    } else {
        handle.switched_off()
    }
}

/// The effective switch, published to the subtree.
#[derive(Clone)]
struct HapticsModeScope {
    enabled: bool,
    child: BoxedView,
}

impl InheritedView for HapticsModeScope {
    type Data = bool;

    fn data(&self) -> &Self::Data {
        &self.enabled
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.enabled != old.enabled
    }
}

crate::impl_inherited_view!(HapticsModeScope);

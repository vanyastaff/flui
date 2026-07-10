//! [`PopScope`] — veto back-navigation and observe pop attempts.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/pop_scope.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f` (`PopScope`, `:83`), plus the
//! `ModalRoute` side: `registerPopEntry`/`unregisterPopEntry`
//! (`routes.dart:2117-2131`), the `_popEntries` veto in `popDisposition`
//! (`:2033-2042`), and the `onPopInvokedWithResult` fan-out (`:2045-2050`).
//!
//! A `can_pop = false` scope blocks **`maybe_pop` / back-navigation only**: a
//! programmatic `pop()` still pops, exactly as in Flutter — `canPop` guards
//! the routes the *user* can leave, not the ones code can. Either way, every
//! registered scope hears the outcome through
//! [`on_pop_invoked`](PopScope::on_pop_invoked) with `did_pop` saying whether
//! the route actually left.
//!
//! # Divergences, named
//!
//! * `on_pop_invoked` carries no `result` — FLUI's `Route::on_pop_invoked` is
//!   result-less today; the `WithResult` variant joins when a consumer needs
//!   the popped value.
//! * No `NavigationNotification` re-dispatch on registration
//!   (`routes.dart:2119-2120`) — FLUI has no `NavigationNotification`.
//! * Registration happens once, in `init_state` — FLUI routes cannot change
//!   over a widget's lifetime (no `GlobalKey` reparenting across routes), so
//!   Flutter's re-register-on-route-change `didChangeDependencies` dance
//!   (`pop_scope.dart:150-166`) collapses.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_view::element::ElementKind;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use parking_lot::Mutex;

/// Reports a pop attempt's outcome: `true` — the route is leaving; `false` —
/// the pop was refused (a veto, this scope's or a sibling's).
pub type PopInvokedCallback = Arc<dyn Fn(bool) + Send + Sync>;

// ============================================================================
// The registry (route side)
// ============================================================================

/// One mounted [`PopScope`]'s live state — Flutter's `PopEntry` (`routes.dart:2137`).
struct PopEntry {
    can_pop: AtomicBool,
    on_pop_invoked: Mutex<Option<PopInvokedCallback>>,
}

/// Every [`PopScope`] mounted inside one route. The route's `ModalInner` owns
/// one — Flutter's `ModalRoute._popEntries` (`routes.dart:1980`) — and the
/// route's `vetoes_pop` / `on_pop_invoked` consult it.
#[derive(Clone, Default)]
pub(crate) struct PopEntryRegistry {
    entries: Arc<Mutex<Vec<Arc<PopEntry>>>>,
}

impl PopEntryRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn register(&self, entry: Arc<PopEntry>) {
        self.entries.lock().push(entry);
    }

    fn deregister(&self, entry: &Arc<PopEntry>) {
        self.entries.lock().retain(|held| !Arc::ptr_eq(held, entry));
    }

    /// `ModalRoute.popDisposition`'s veto half (`routes.dart:2034-2038`): any
    /// entry with `can_pop = false`.
    pub(crate) fn any_vetoes(&self) -> bool {
        self.entries
            .lock()
            .iter()
            .any(|entry| !entry.can_pop.load(Ordering::Relaxed))
    }

    /// `ModalRoute.onPopInvokedWithResult`'s fan-out (`routes.dart:2045-2050`).
    pub(crate) fn notify_pop_invoked(&self, did_pop: bool) {
        // Clone out so a callback may mount/unmount scopes without deadlock.
        let entries = self.entries.lock().clone();
        for entry in &entries {
            if let Some(callback) = entry.on_pop_invoked.lock().clone() {
                callback(did_pop);
            }
        }
    }
}

impl std::fmt::Debug for PopEntryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopEntryRegistry")
            .field("entries", &self.entries.lock().len())
            .finish()
    }
}

/// Provides the enclosing route's [`PopEntryRegistry`] to the page subtree —
/// the `HeroScope` pattern. Never notifies: the registry handle is fixed for
/// the route's lifetime.
#[derive(Clone)]
pub(crate) struct PopEntryScope {
    registry: PopEntryRegistry,
    child: BoxedView,
}

impl PopEntryScope {
    pub(crate) fn new(registry: PopEntryRegistry, child: impl IntoView) -> Self {
        Self {
            registry,
            child: BoxedView(Box::new(child.into_view())),
        }
    }
}

impl std::fmt::Debug for PopEntryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopEntryScope")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl InheritedView for PopEntryScope {
    type Data = PopEntryRegistry;

    fn data(&self) -> &Self::Data {
        &self.registry
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        false
    }
}

impl_inherited_view!(PopEntryScope);

// ============================================================================
// The widget
// ============================================================================

/// Vetoes attempts by the **user** to dismiss the enclosing route, and reports
/// every pop attempt's outcome — Flutter's `PopScope` (`pop_scope.dart:83`).
///
/// While [`can_pop`](Self::can_pop) is `false`, `NavigatorHandle::maybe_pop`
/// (and anything routed through it) refuses and reports `handled`; the
/// enclosing route stays. A programmatic `pop()` is not blocked. In both
/// cases [`on_pop_invoked`](Self::on_pop_invoked) hears the outcome.
///
/// Outside any route, a `PopScope` is inert — there is nothing to veto.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = PopScope::new(Text::new("unsaved changes"))
///     .can_pop(false)
///     .on_pop_invoked(|did_pop| {
///         if !did_pop {
///             // show the "discard changes?" dialog
///         }
///     });
/// ```
#[derive(Clone)]
pub struct PopScope {
    child: BoxedView,
    can_pop: bool,
    on_pop_invoked: Option<PopInvokedCallback>,
}

impl PopScope {
    /// A scope that allows popping — `can_pop` defaults to `true`
    /// (`pop_scope.dart:145`), so a bare `PopScope` only observes.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            can_pop: true,
            on_pop_invoked: None,
        }
    }

    /// Whether the user may dismiss the enclosing route (`pop_scope.dart:142`).
    #[must_use]
    pub fn can_pop(mut self, can_pop: bool) -> Self {
        self.can_pop = can_pop;
        self
    }

    /// Called after every pop attempt on the enclosing route: `true` when it
    /// actually popped, `false` when a veto refused it — Flutter's
    /// `onPopInvokedWithResult` minus the result (`pop_scope.dart:106`).
    #[must_use]
    pub fn on_pop_invoked(mut self, callback: impl Fn(bool) + Send + Sync + 'static) -> Self {
        self.on_pop_invoked = Some(Arc::new(callback));
        self
    }
}

impl std::fmt::Debug for PopScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopScope")
            .field("can_pop", &self.can_pop)
            .finish_non_exhaustive()
    }
}

impl View for PopScope {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for PopScope {
    type State = PopScopeState;

    fn create_state(&self) -> Self::State {
        PopScopeState {
            entry: Arc::new(PopEntry {
                can_pop: AtomicBool::new(self.can_pop),
                on_pop_invoked: Mutex::new(self.on_pop_invoked.clone()),
            }),
            registry: None,
        }
    }
}

/// The state behind [`PopScope`]. `pub` only because `StatefulView::State`
/// requires it; not re-exported.
pub struct PopScopeState {
    entry: Arc<PopEntry>,
    registry: Option<PopEntryRegistry>,
}

impl std::fmt::Debug for PopScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopScopeState")
            .field("can_pop", &self.entry.can_pop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ViewState<PopScope> for PopScopeState {
    /// `ModalRoute.registerPopEntry` (`routes.dart:2117`), through the route's
    /// ambient registry. A `PopScope` outside any route finds none and stays
    /// inert.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(registry) = ctx.get::<PopEntryScope, _>(|scope| scope.registry.clone()) {
            registry.register(Arc::clone(&self.entry));
            self.registry = Some(registry);
        }
    }

    /// Keep the live entry current — Flutter re-reads `widget.canPop` through
    /// the entry's notifier (`pop_scope.dart:171-179`).
    fn did_update_view(&mut self, _old: &PopScope, new_view: &PopScope) {
        self.entry
            .can_pop
            .store(new_view.can_pop, Ordering::Relaxed);
        self.entry
            .on_pop_invoked
            .lock()
            .clone_from(&new_view.on_pop_invoked);
    }

    /// `unregisterPopEntry` (`routes.dart:2126`).
    fn dispose(&mut self) {
        if let Some(registry) = self.registry.take() {
            registry.deregister(&self.entry);
        }
    }

    fn build(&self, view: &PopScope, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

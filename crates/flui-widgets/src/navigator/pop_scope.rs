//! [`PopScope`] — veto back-navigation and observe pop attempts.
//!
//! A `can_pop = false` scope blocks **`maybe_pop` / back-navigation only**: a
//! programmatic `pop()` still pops — `can_pop` guards
//! the routes the *user* can leave, not the ones code can. Either way, every
//! registered scope hears the outcome through
//! [`on_pop_invoked`](PopScope::on_pop_invoked) with `did_pop` saying whether
//! the route actually left.
//!
//! # Scope of the surface
//!
//! * `on_pop_invoked` carries no `result` — `Route::on_pop_invoked` is
//!   result-less today; a result-carrying variant joins when a consumer needs
//!   the popped value.
//! * There is no navigation-notification re-dispatch on registration.
//! * Registration happens once, in `init_state` — routes cannot change
//!   over a widget's lifetime (no `GlobalKey` reparenting across routes), so
//!   there is no re-registration on route change.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_view::element::ElementKind;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use parking_lot::Mutex;

/// Reports a pop attempt's outcome: `true` — the route is leaving; `false` —
/// the pop was refused (a veto, this scope's or a sibling's).
pub type PopInvokedCallback = Rc<dyn Fn(&mut EventCx<'_>, bool)>;

type BoundPopCallback = Rc<dyn Fn(bool)>;

// ============================================================================
// The registry (route side)
// ============================================================================

/// One mounted [`PopScope`]'s live state.
struct PopEntry {
    can_pop: AtomicBool,
    on_pop_invoked: Mutex<Option<BoundPopCallback>>,
}

/// Every [`PopScope`] mounted inside one route. The route's `ModalInner` owns
/// one, and the
/// route's `vetoes_pop` / `on_pop_invoked` consult it.
#[derive(Clone, Default)]
pub(crate) struct PopEntryRegistry {
    entries: Arc<super::lifecycle::TerminalVec<Arc<PopEntry>>>,
}

impl PopEntryRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn register(&self, entry: Arc<PopEntry>) {
        self.entries.lock().push(entry);
    }

    fn deregister(&self, entry: &Arc<PopEntry>) {
        let mut entries = std::mem::take(&mut *self.entries.lock());
        entries.retain(|held| !Arc::ptr_eq(held, entry));
        let _prev = std::mem::replace(&mut *self.entries.lock(), entries);
    }

    /// The veto half of a pop: any entry with `can_pop = false`.
    pub(crate) fn any_vetoes(&self) -> bool {
        self.entries
            .lock()
            .iter()
            .any(|entry| !entry.can_pop.load(Ordering::Relaxed))
    }

    /// Fan a pop attempt's outcome out to every registered scope.
    pub(crate) fn notify_pop_invoked(&self, did_pop: bool) {
        // Clone out so a callback may mount/unmount scopes without deadlock.
        let entries = self.entries.lock().clone();
        for entry in &entries {
            let callback = entry.on_pop_invoked.lock().clone();
            if let Some(callback) = callback {
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
pub(crate) struct PopEntryScope {
    registry: crate::navigator::lifecycle::Terminal<PopEntryRegistry>,
    child: crate::navigator::lifecycle::Terminal<BoxedView>,
}

impl Clone for PopEntryScope {
    fn clone(&self) -> Self {
        Self {
            registry: crate::navigator::lifecycle::Terminal::new(self.registry.clone()),
            child: crate::navigator::lifecycle::Terminal::new(self.child.clone()),
        }
    }
}

impl Drop for PopEntryScope {
    fn drop(&mut self) {
        let registry = self.registry.withdraw();
        let child = self.child.withdraw();
        drop((registry, child));
    }
}

impl PopEntryScope {
    pub(crate) fn new(registry: PopEntryRegistry, child: impl IntoView) -> Self {
        Self {
            registry: crate::navigator::lifecycle::Terminal::new(registry),
            child: crate::navigator::lifecycle::Terminal::new(BoxedView(Box::new(
                child.into_view(),
            ))),
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
        &*self.child
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
/// every pop attempt's outcome.
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
///     .on_pop_invoked(|_cx, did_pop| {
///         if !did_pop {
///             // show the "discard changes?" dialog
///         }
///     });
/// ```
pub struct PopScope {
    child: crate::navigator::lifecycle::Terminal<BoxedView>,
    can_pop: bool,
    on_pop_invoked: Option<PopInvokedCallback>,
}

impl Clone for PopScope {
    fn clone(&self) -> Self {
        Self {
            child: crate::navigator::lifecycle::Terminal::new(self.child.clone()),
            can_pop: self.can_pop,
            on_pop_invoked: self.on_pop_invoked.clone(),
        }
    }
}

impl Drop for PopScope {
    fn drop(&mut self) {
        let child = self.child.withdraw();
        let callback = crate::navigator::lifecycle::Terminal::new(self.on_pop_invoked.take());
        drop((child, callback));
    }
}

impl PopScope {
    /// A scope that allows popping — `can_pop` defaults to `true`, so a bare
    /// `PopScope` only observes.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: crate::navigator::lifecycle::Terminal::new(BoxedView(Box::new(
                child.into_view(),
            ))),
            can_pop: true,
            on_pop_invoked: None,
        }
    }

    /// Whether the user may dismiss the enclosing route.
    #[must_use]
    pub fn can_pop(mut self, can_pop: bool) -> Self {
        self.can_pop = can_pop;
        self
    }

    /// Called after every pop attempt on the enclosing route: `true` when it
    /// actually popped, `false` when a veto refused it. Carries no popped value.
    #[must_use]
    pub fn on_pop_invoked<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, bool) -> R + 'static,
    ) -> Self {
        self.on_pop_invoked = Some(Rc::new(move |cx, did_pop| callback(cx, did_pop).report()));
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
            entry: crate::navigator::lifecycle::Terminal::new(Arc::new(PopEntry {
                can_pop: AtomicBool::new(self.can_pop),
                on_pop_invoked: Mutex::new(None),
            })),
            registry: None,
            callback: self.on_pop_invoked.clone(),
            writer: None,
        }
    }
}

/// The state behind [`PopScope`]. `pub` only because `StatefulView::State`
/// requires it; not re-exported.
pub struct PopScopeState {
    callback: Option<PopInvokedCallback>,
    writer: Option<WriterSource>,
    entry: crate::navigator::lifecycle::Terminal<Arc<PopEntry>>,
    registry: Option<PopEntryRegistry>,
}

impl Drop for PopScopeState {
    fn drop(&mut self) {
        let callback = crate::navigator::lifecycle::Terminal::new(self.callback.take());
        let writer = crate::navigator::lifecycle::Terminal::new(self.writer.take());
        let entry = self.entry.withdraw();
        let registry = crate::navigator::lifecycle::Terminal::new(self.registry.take());
        drop((callback, writer, entry, registry));
    }
}

impl std::fmt::Debug for PopScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopScopeState")
            .field("can_pop", &self.entry.can_pop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ViewState<PopScope> for PopScopeState {
    /// Registers with the route's ambient registry. A `PopScope` outside any route finds none and stays
    /// inert.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.writer = Some(ctx.writer_source());
        self.install_callback();
        if let Some(registry) = ctx.get::<PopEntryScope, _>(|scope| scope.registry.clone()) {
            registry.register(Arc::clone(&self.entry));
            self.registry = Some(registry);
        }
    }

    /// Keep the live entry current with the new `can_pop` and callback.
    fn did_update_view(&mut self, _old: &PopScope, new_view: &PopScope) {
        self.entry
            .can_pop
            .store(new_view.can_pop, Ordering::Relaxed);
        self.callback.clone_from(&new_view.on_pop_invoked);
        self.install_callback();
    }

    /// Unregister from the route's registry.
    fn dispose(&mut self) {
        if let Some(registry) = self.registry.take() {
            registry.deregister(&self.entry);
        }
    }

    fn build(&self, view: &PopScope, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

impl PopScopeState {
    fn install_callback(&self) {
        let writer = self
            .writer
            .clone()
            .expect("BUG: PopScope initialized before callback installation");
        let callback = self.callback.clone().map(|callback| {
            let callback = crate::navigator::lifecycle::Terminal::new(callback);
            let writer = crate::navigator::lifecycle::Terminal::new(writer);
            Rc::new(move |did_pop| writer.write(|cx| callback(cx, did_pop))) as Rc<dyn Fn(bool)>
        });
        let previous = std::mem::replace(&mut *self.entry.on_pop_invoked.lock(), callback);
        drop(previous);
    }
}

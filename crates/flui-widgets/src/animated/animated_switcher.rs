//! [`AnimatedSwitcher`] — cross-fades (or custom-transitions) between a
//! sequence of children keyed by [`View::can_update`].
//!
//! Flutter parity: `widgets/animated_switcher.dart` `AnimatedSwitcher`, tag
//! `3.44.0`. Structurally this widget is the odd one out among its
//! `animated/` siblings: `AnimatedContainer`/`AnimatedOpacity`/… hold ONE
//! persistent [`AnimationController`] retargeted in place
//! ([`crate::animated::implicitly_animated::ImplicitController`]).
//! `AnimatedSwitcher` instead owns a **set of entries**, each with its own
//! controller — a new child gets a fresh entry that animates in while the
//! previous entry (now "outgoing") animates out, and outgoing entries are
//! disposed once their reverse run dismisses. This mirrors the oracle's
//! `_ChildEntry` / `_currentEntry` / `_outgoingEntries` bookkeeping.
//!
//! # Why `build` needs interior mutability
//!
//! `ViewState::build` takes `&self` (unlike `did_update_view`, which takes
//! `&mut self`), but an outgoing entry's dismissal is discovered
//! asynchronously — from an [`AnimationController`] status-listener callback
//! that fires on a later frame, independent of any `did_update_view` call.
//! The listener itself only flips a `Send + Sync`-safe [`AtomicBool`] and
//! schedules a rebuild ([`RebuildHandle::schedule`]) — it never touches
//! `BoxedView`/`AnimationController`, which are not required to be `Send`
//! ([`View`] carries no such bound). The actual sweep (disposing the
//! dismissed entry and dropping it from the list) happens inside `build`,
//! reading that flag, through a `RefCell<Vec<ChildEntry>>` — the same
//! shared-mutable-state-behind-`&self` idiom `Overlay`/`Navigator` already use
//! (`parking_lot::Mutex` there; `RefCell` here since nothing here crosses a
//! thread boundary). This is the same "pull, don't push" shape
//! [`AnimatedSize`](crate::AnimatedSize)'s `completed_runs` counter uses for
//! its `on_end` callback.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, CurvedAnimation, Curves, Scheduler, Vsync,
    VsyncRegistration,
};
use flui_foundation::{ListenerId, ViewKey};
use flui_types::Alignment;
use flui_view::element::ElementKind;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, BuildContextExt, IntoView, RebuildHandle, StatelessView, ValueKey, View, ViewExt,
    ViewState,
};

use crate::animated::vsync_scope::VsyncScope;
use crate::{FadeTransition, Stack};

/// A custom transition for [`AnimatedSwitcher`]: wraps an incoming/outgoing
/// `child` with a widget driven by `animation` (`0.0` = fully switched out,
/// `1.0` = fully switched in).
///
/// Flutter parity: `AnimatedSwitcherTransitionBuilder`
/// (`animated_switcher.dart`, tag `3.44.0`).
pub type AnimatedSwitcherTransitionBuilder =
    Rc<dyn Fn(BoxedView, Arc<dyn Animation<f32>>) -> BoxedView>;

/// A custom layout for [`AnimatedSwitcher`]: arranges the incoming
/// `current_child` (if any) alongside the still-animating-out
/// `previous_children` (oldest first).
///
/// Flutter parity: `AnimatedSwitcherLayoutBuilder` (`animated_switcher.dart`,
/// tag `3.44.0`).
pub type AnimatedSwitcherLayoutBuilder = Rc<dyn Fn(Option<BoxedView>, Vec<BoxedView>) -> BoxedView>;

/// Cross-fades between children, keyed by [`View::can_update`] (Flutter's
/// `Widget.canUpdate`: same concrete type and same key).
///
/// Setting [`AnimatedSwitcher::child`] to a widget that is NOT
/// `can_update`-compatible with the previous one starts a transition: the old
/// child animates out along `switch_out_curve` while the new one animates in
/// along `switch_in_curve`, both over `duration` (or `reverse_duration` for
/// the outgoing run, if set). A `can_update`-compatible child instead updates
/// the existing entry in place — no transition restarts. Setting the SAME
/// key on a new child that is mid-transition-out (e.g. a value oscillating A
/// → B → A faster than `duration`) does not collapse into the old outgoing
/// entry; it starts its own fresh entry, exactly as `Widget.canUpdate` would
/// never unify two different `_ChildEntry`s.
///
/// The default `transition_builder` cross-fades via [`FadeTransition`]; the
/// default `layout_builder` overlaps every still-animating entry in a
/// [`Stack`], centered, oldest-to-newest with the incoming child painted
/// last (on top).
#[derive(Clone, StatefulView)]
pub struct AnimatedSwitcher {
    child: Option<BoxedView>,
    duration: Duration,
    reverse_duration: Option<Duration>,
    switch_in_curve: ArcCurve,
    switch_out_curve: ArcCurve,
    transition_builder: AnimatedSwitcherTransitionBuilder,
    layout_builder: AnimatedSwitcherLayoutBuilder,
}

thread_local! {
    // Canonical, thread-shared handles for the default builders — mirrors
    // `crate::animated::implicitly_animated::default_curve`'s `ArcCurve`
    // caching (same rationale, `thread_local!` here rather than a
    // `static OnceLock` because `Rc<dyn Fn>` is neither `Send` nor `Sync`).
    // Without this, every `AnimatedSwitcher::new()` would mint a FRESH `Rc`
    // allocation wrapping the same default function, so
    // `did_update_view`'s `Rc::ptr_eq` builder-changed check (which detects a
    // genuine `transition_builder` override) would report "changed" on
    // EVERY reconfigure even when the caller never touched it — needlessly
    // rebuilding every entry's cached transition on every rebuild.
    static DEFAULT_TRANSITION_BUILDER: AnimatedSwitcherTransitionBuilder =
        Rc::new(AnimatedSwitcher::default_transition_builder);
    static DEFAULT_LAYOUT_BUILDER: AnimatedSwitcherLayoutBuilder =
        Rc::new(AnimatedSwitcher::default_layout_builder);
}

impl AnimatedSwitcher {
    /// A switcher with no child yet, transitioning over `duration` with
    /// `Curves::Linear` in both directions (oracle default — deliberately
    /// NOT the `EaseInOut` default of the sibling implicit-animation
    /// widgets), the default fade transition, and the default centered-stack
    /// layout.
    pub fn new(duration: Duration) -> Self {
        Self {
            child: None,
            duration,
            reverse_duration: None,
            switch_in_curve: ArcCurve::new(Curves::Linear),
            switch_out_curve: ArcCurve::new(Curves::Linear),
            transition_builder: DEFAULT_TRANSITION_BUILDER.with(Clone::clone),
            layout_builder: DEFAULT_LAYOUT_BUILDER.with(Clone::clone),
        }
    }

    /// The child to display. Setting a `can_update`-incompatible child
    /// (different concrete type or key) starts a transition.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view().boxed());
        self
    }

    /// Overrides the outgoing-run duration; defaults to `duration`.
    #[must_use]
    pub fn reverse_duration(mut self, reverse_duration: Duration) -> Self {
        self.reverse_duration = Some(reverse_duration);
        self
    }

    /// Overrides the curve applied while a child transitions in.
    #[must_use]
    pub fn switch_in_curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.switch_in_curve = ArcCurve::new(curve);
        self
    }

    /// Overrides the curve applied while a child transitions out.
    #[must_use]
    pub fn switch_out_curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.switch_out_curve = ArcCurve::new(curve);
        self
    }

    /// Overrides how each entry is wrapped for transition; defaults to
    /// [`AnimatedSwitcher::default_transition_builder`].
    #[must_use]
    pub fn transition_builder(
        mut self,
        builder: impl Fn(BoxedView, Arc<dyn Animation<f32>>) -> BoxedView + 'static,
    ) -> Self {
        self.transition_builder = Rc::new(builder);
        self
    }

    /// Overrides how the current and outgoing entries are laid out together;
    /// defaults to [`AnimatedSwitcher::default_layout_builder`].
    #[must_use]
    pub fn layout_builder(
        mut self,
        builder: impl Fn(Option<BoxedView>, Vec<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        self.layout_builder = Rc::new(builder);
        self
    }

    /// The default `transition_builder`: cross-fades `child` via
    /// [`FadeTransition`].
    ///
    /// Flutter parity: `AnimatedSwitcher.defaultTransitionBuilder`
    /// (`animated_switcher.dart`, tag `3.44.0`) wraps in a `FadeTransition`
    /// additionally keyed by `child.key`. FLUI's `FadeTransition` has no
    /// `.key(...)` setter (a keyed `impl View` cannot come from
    /// `impl_animated_view!`'s generated block — see `flui-macros`'
    /// "Keyed widgets" doc), so that inner re-key is not reproduced; the
    /// entry's OWN stable identity (this builder's caller wraps the result
    /// in a per-entry key, mirroring the oracle's `KeyedSubtree.wrap(...,
    /// _childNumber)`) is what the corpus actually asserts on.
    pub fn default_transition_builder(
        child: BoxedView,
        animation: Arc<dyn Animation<f32>>,
    ) -> BoxedView {
        FadeTransition::new(animation, child).boxed()
    }

    /// The default `layout_builder`: a [`Stack`] centering every
    /// still-animating entry, oldest `previous_children` first, then
    /// `current_child` last (so it paints on top).
    ///
    /// Flutter parity: `AnimatedSwitcher.defaultLayoutBuilder`
    /// (`animated_switcher.dart`, tag `3.44.0`).
    pub fn default_layout_builder(
        current_child: Option<BoxedView>,
        previous_children: Vec<BoxedView>,
    ) -> BoxedView {
        let mut children = previous_children;
        children.extend(current_child);
        Stack::new(children).alignment(Alignment::CENTER).boxed()
    }
}

impl std::fmt::Debug for AnimatedSwitcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSwitcher")
            .field("duration", &self.duration)
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

/// Attaches a stable per-entry identity to an already-built transition widget
/// so the layout builder's dynamic `Vec<BoxedView>` — reconciled by
/// `flui-view`'s keyed-child machinery — recognizes the SAME entry across
/// rebuilds even though [`AnimatedSwitcherState::build`] constructs a fresh
/// `Vec` every time. Rust-native stand-in for Flutter's `KeyedSubtree`
/// (`widgets/basic.dart`), which the oracle's `_newEntry` /
/// `_updateTransitionForEntry` wrap every transition in for exactly this
/// reason — the key is the entry's `child_number`, never rebuilt once
/// assigned, so `_updateTransitionForEntry`-equivalent calls
/// ([`ChildEntry::update_transition`]) can swap the wrapped content without
/// losing the slot's element identity.
#[derive(Clone)]
struct KeyedEntry {
    key: ValueKey<u64>,
    child: BoxedView,
}

impl KeyedEntry {
    fn new(child_number: u64, child: BoxedView) -> Self {
        Self {
            key: ValueKey::new(child_number),
            child,
        }
    }
}

impl std::fmt::Debug for KeyedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedEntry")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl View for KeyedEntry {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for KeyedEntry {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// One child that is, now or in the past, the value [`AnimatedSwitcher`]'s
/// `child` was set to but is still transitioning. Flutter parity:
/// `_ChildEntry` (`animated_switcher.dart`, tag `3.44.0`).
struct ChildEntry {
    /// This entry's stable identity — assigned once at creation, carried
    /// unchanged by its [`KeyedEntry`] wrapper for the entry's whole life.
    child_number: u64,
    /// The transition's driver. Runs forward while incoming, reverse once
    /// demoted to outgoing.
    controller: AnimationController,
    /// `controller`, eased by `switch_in_curve` going forward and
    /// `switch_out_curve` going backward — what `transition_builder`
    /// actually animates against.
    curved: CurvedAnimation<ArcCurve>,
    /// The `Vsync` this entry registered with, kept alongside the
    /// registration so [`ChildEntry::dispose`] can unregister (mirrors
    /// `ImplicitController::dispose`).
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
    status_listener_id: Option<ListenerId>,
    /// Flipped by the status-listener callback when `controller` reaches
    /// [`AnimationStatus::Dismissed`] (a completed reverse run). Read — and
    /// acted on — by [`AnimatedSwitcherState::build`]'s sweep; see the
    /// module docs for why the listener cannot dispose the entry itself.
    dismissed: Arc<AtomicBool>,
    /// The child widget this entry was built from, used to detect a
    /// same-entry rebuild (`View::can_update`) and to re-run
    /// `transition_builder` on demand.
    widget_child: BoxedView,
    /// The cached, already-built (and stably keyed) transition widget.
    transition: BoxedView,
}

impl ChildEntry {
    /// A fresh entry wrapping `child`, at rest (`animate: false`, the very
    /// first entry created from `create_state`) or animating in
    /// (`animate: true`, every later swap). Builds the controller, curve,
    /// and initial transition, but does NOT register with a `Vsync` or
    /// attach the dismissal status-listener — [`ChildEntry::register`] does
    /// that once a [`BuildContext`] is available (`create_state` has none;
    /// see `AnimatedSwitcherState::init_state`).
    #[allow(clippy::too_many_arguments)] // one argument per oracle constructor parameter
    fn new(
        child: BoxedView,
        child_number: u64,
        duration: Duration,
        reverse_duration: Option<Duration>,
        switch_in_curve: ArcCurve,
        switch_out_curve: ArcCurve,
        transition_builder: &AnimatedSwitcherTransitionBuilder,
        animate: bool,
    ) -> Self {
        let controller = AnimationController::new(duration, Arc::new(Scheduler::new()));
        if let Some(reverse_duration) = reverse_duration {
            controller.set_reverse_duration(reverse_duration);
        }
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        let curved =
            CurvedAnimation::new(parent, switch_in_curve).with_reverse_curve(switch_out_curve);

        if animate {
            // Oracle: `controller.forward();` (`_addEntryForNewChild`,
            // `animated_switcher.dart`, tag `3.44.0`).
            let _ = controller.forward();
        } else {
            // Oracle: `controller.value = 1.0;` for the very first entry — sits
            // at rest, fully switched in, no motion.
            controller.set_value(1.0);
        }

        let transition = Self::build_transition(child_number, &child, &curved, transition_builder);

        Self {
            child_number,
            controller,
            curved,
            vsync: None,
            vsync_registration: None,
            status_listener_id: None,
            dismissed: Arc::new(AtomicBool::new(false)),
            widget_child: child,
            transition,
        }
    }

    /// Register with `vsync` (if any) and attach the dismissal
    /// status-listener, which flips [`ChildEntry::dismissed`] and schedules
    /// `rebuild` when `controller` reaches
    /// [`AnimationStatus::Dismissed`] — the oracle's
    /// `animation.addStatusListener` in `_newEntry`.
    fn register(&mut self, vsync: Option<Vsync>, rebuild: RebuildHandle) {
        if let Some(vsync) = &vsync {
            self.vsync_registration = Some(vsync.register(self.controller.clone()));
        }
        self.vsync = vsync;

        let dismissed = Arc::clone(&self.dismissed);
        self.status_listener_id =
            Some(self.controller.add_status_listener(Arc::new(move |status| {
                if status == AnimationStatus::Dismissed {
                    dismissed.store(true, Ordering::Release);
                    rebuild.schedule(flui_view::RebuildReason::AnimationTick);
                }
            })));
    }

    /// Re-run `transition_builder` over the current `widget_child`/`curved`,
    /// preserving this entry's key. Oracle: `_updateTransitionForEntry`
    /// (`animated_switcher.dart`, tag `3.44.0`) — called both when
    /// `transition_builder` itself changes and when a `can_update`-compatible
    /// child rebuilds the current entry in place.
    fn update_transition(&mut self, transition_builder: &AnimatedSwitcherTransitionBuilder) {
        self.transition = Self::build_transition(
            self.child_number,
            &self.widget_child,
            &self.curved,
            transition_builder,
        );
    }

    fn build_transition(
        child_number: u64,
        widget_child: &BoxedView,
        curved: &CurvedAnimation<ArcCurve>,
        transition_builder: &AnimatedSwitcherTransitionBuilder,
    ) -> BoxedView {
        let animation: Arc<dyn Animation<f32>> = Arc::new(curved.clone());
        let content = transition_builder(widget_child.clone(), animation);
        KeyedEntry::new(child_number, content).boxed()
    }

    /// Detach the status listener, unregister from `vsync`, and dispose the
    /// controller. Oracle: `dispose()` (`animated_switcher.dart`, tag
    /// `3.44.0`) disposes every entry's controller and animation.
    fn dispose(&mut self) {
        if let Some(id) = self.status_listener_id.take() {
            self.controller.remove_status_listener(id);
        }
        if let (Some(vsync), Some(registration)) =
            (self.vsync.take(), self.vsync_registration.take())
        {
            vsync.unregister(registration);
        }
        self.controller.dispose();
    }
}

impl std::fmt::Debug for ChildEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildEntry")
            .field("child_number", &self.child_number)
            .field("status", &self.controller.status())
            .field("dismissed", &self.dismissed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedSwitcher`]. See the module docs for why
/// `outgoing_entries` needs interior mutability.
pub struct AnimatedSwitcherState {
    current_entry: Option<ChildEntry>,
    outgoing_entries: RefCell<Vec<ChildEntry>>,
    /// Monotonically increasing entry counter — the source of each
    /// [`ChildEntry::child_number`]. Oracle: `_childNumber`
    /// (`animated_switcher.dart`, tag `3.44.0`).
    child_number: u64,
    /// Captured in `init_state` (unavailable in `create_state`, which has no
    /// `BuildContext`); reused for every later entry `did_update_view`
    /// creates. `None` only in the pre-`init_state` window.
    rebuild: Option<RebuildHandle>,
    vsync: Option<Vsync>,
}

impl std::fmt::Debug for AnimatedSwitcherState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSwitcherState")
            .field("has_current_entry", &self.current_entry.is_some())
            .field("outgoing_count", &self.outgoing_entries.borrow().len())
            .finish_non_exhaustive()
    }
}

impl StatefulView for AnimatedSwitcher {
    type State = AnimatedSwitcherState;

    fn create_state(&self) -> Self::State {
        // Oracle: `initState` calls `_addEntryForNewChild(animate: false)`
        // (`animated_switcher.dart`, tag `3.44.0`). FLUI splits controller
        // construction (here, no `BuildContext` yet) from vsync/listener
        // registration (`init_state`, below) — see `ChildEntry::new`'s doc.
        let current_entry = self.child.clone().map(|child| {
            ChildEntry::new(
                child,
                0,
                self.duration,
                self.reverse_duration,
                self.switch_in_curve.clone(),
                self.switch_out_curve.clone(),
                &self.transition_builder,
                false,
            )
        });
        AnimatedSwitcherState {
            current_entry,
            outgoing_entries: RefCell::new(Vec::new()),
            child_number: 0,
            rebuild: None,
            vsync: None,
        }
    }
}

impl AnimatedSwitcherState {
    /// Demote the current entry to outgoing (reversing it) and install a
    /// fresh incoming entry for `view.child`, or do nothing if `view` has no
    /// child. Oracle: `_addEntryForNewChild` (`animated_switcher.dart`, tag
    /// `3.44.0`).
    fn add_entry_for_new_child(&mut self, view: &AnimatedSwitcher, animate: bool) {
        debug_assert!(
            animate || self.current_entry.is_none(),
            "BUG: a non-animated entry replacement is only valid for the very first entry"
        );
        if let Some(old_entry) = self.current_entry.take() {
            debug_assert!(animate, "BUG: demoting a current entry always animates");
            let _ = old_entry.controller.reverse();
            self.outgoing_entries.get_mut().push(old_entry);
        }
        let Some(child) = view.child.clone() else {
            return;
        };
        let mut entry = ChildEntry::new(
            child,
            self.child_number,
            view.duration,
            view.reverse_duration,
            view.switch_in_curve.clone(),
            view.switch_out_curve.clone(),
            &view.transition_builder,
            animate,
        );
        if let Some(rebuild) = self.rebuild.clone() {
            entry.register(self.vsync.clone(), rebuild);
        }
        self.current_entry = Some(entry);
    }
}

impl ViewState<AnimatedSwitcher> for AnimatedSwitcherState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let rebuild = ctx.rebuild_handle();
        let vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        if let Some(entry) = self.current_entry.as_mut() {
            entry.register(vsync.clone(), rebuild.clone());
        }
        self.rebuild = Some(rebuild);
        self.vsync = vsync;
    }

    fn build(&self, view: &AnimatedSwitcher, _ctx: &dyn BuildContext) -> impl IntoView {
        // Sweep entries whose reverse run dismissed since the last build — see
        // the module docs for why this cannot happen inside the status
        // listener itself. Oracle: the `setState` inside
        // `animation.addStatusListener` in `_newEntry` removes the entry from
        // `_outgoingEntries` and disposes it.
        self.outgoing_entries.borrow_mut().retain_mut(|entry| {
            if entry.dismissed.load(Ordering::Acquire) {
                entry.dispose();
                false
            } else {
                true
            }
        });

        let current_child_number = self.current_entry.as_ref().map(|entry| entry.child_number);
        let current_transition = self
            .current_entry
            .as_ref()
            .map(|entry| entry.transition.clone());
        // Oracle: `_outgoingWidgets!.where((w) => w.key != _currentEntry?.transition.key)`
        // (`build`, `animated_switcher.dart`, tag `3.44.0`) — an outgoing entry
        // sharing the current entry's key is suppressed from
        // `previousChildren`; translated here as the same `child_number` never
        // appearing in both lists at once.
        let previous_transitions: Vec<BoxedView> = self
            .outgoing_entries
            .borrow()
            .iter()
            .filter(|entry| Some(entry.child_number) != current_child_number)
            .map(|entry| entry.transition.clone())
            .collect();

        (view.layout_builder)(current_transition, previous_transitions)
    }

    fn did_update_view(&mut self, old_view: &AnimatedSwitcher, new_view: &AnimatedSwitcher) {
        // Oracle: a `transitionBuilder` swap rebuilds every cached transition in
        // place, preserving each entry's key (`didUpdateWidget`,
        // `animated_switcher.dart`, tag `3.44.0`).
        if !Rc::ptr_eq(&old_view.transition_builder, &new_view.transition_builder) {
            for entry in self.outgoing_entries.get_mut() {
                entry.update_transition(&new_view.transition_builder);
            }
            if let Some(entry) = self.current_entry.as_mut() {
                entry.update_transition(&new_view.transition_builder);
            }
        }

        // Oracle: `hasNewChild != hasOldChild || (hasNewChild &&
        // !Widget.canUpdate(widget.child!, _currentEntry!.widgetChild))`.
        let needs_new_entry = match (new_view.child.as_ref(), self.current_entry.as_ref()) {
            (Some(new_child), Some(entry)) => !new_child.can_update(&entry.widget_child),
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };

        if needs_new_entry {
            self.child_number += 1;
            self.add_entry_for_new_child(new_view, true);
        } else if let (Some(new_child), Some(entry)) =
            (new_view.child.clone(), self.current_entry.as_mut())
        {
            // Same entry, updated in place — no transition restart.
            entry.widget_child = new_child;
            entry.update_transition(&new_view.transition_builder);
        }
    }

    fn dispose(&mut self) {
        if let Some(entry) = self.current_entry.as_mut() {
            entry.dispose();
        }
        for entry in self.outgoing_entries.get_mut() {
            entry.dispose();
        }
    }
}

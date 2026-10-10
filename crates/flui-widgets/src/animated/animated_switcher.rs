//! [`AnimatedSwitcher`] — cross-fades (or custom-transitions) between a
//! sequence of children keyed by [`View::can_update`].
//!
//! Structurally this widget is the odd one out among its
//! `animated/` siblings: property widgets own persistent trajectories retargeted
//! in place.
//! `AnimatedSwitcher` instead owns a **set of entries**, each with its own
//! controller — a new child gets a fresh entry that animates in while the
//! previous entry (now "outgoing") animates out, and outgoing entries are
//! disposed once their reverse run dismisses.
//!
//! # Why `build` needs interior mutability
//!
//! `ViewState::build` takes `&self` (unlike `did_update_view`, which takes
//! `&mut self`), but an outgoing entry's dismissal is discovered
//! from an [`AnimationController`] status-listener callback
//! that fires on a later frame, independent of any `did_update_view` call.
//! The owner-local listener marks the entry dismissed and schedules a rebuild
//! ([`RebuildHandle::schedule`]). The sweep disposes dismissed entries during
//! `build`, through a `RefCell<Vec<ChildEntry>>`, before producing the next
//! layout's transition list. This entry maintenance is separate from event
//! delivery: [`AnimatedSize`](crate::AnimatedSize) admits its completion effects
//! directly to the owner post-frame lane.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, CurvedAnimation, Curves, DrivenController,
    Vsync, VsyncUpdate,
};
use flui_foundation::ViewKey;
use flui_painting::Alignment;
use flui_view::element::ElementKind;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RebuildHandle, StatelessView, ValueKey, View, ViewExt, ViewState,
};

use crate::animated::vsync_scope::VsyncScope;
use crate::support::retirement::Terminal;
use crate::{FadeTransition, Stack};

/// A custom transition for [`AnimatedSwitcher`]: wraps an incoming/outgoing
/// `child` with a widget driven by `animation` (`0.0` = fully switched out,
/// `1.0` = fully switched in).
pub type AnimatedSwitcherTransitionBuilder =
    Rc<dyn Fn(BoxedView, std::rc::Rc<dyn Animation<f64>>) -> BoxedView>;

/// A custom layout for [`AnimatedSwitcher`]: arranges the incoming
/// `current_child` (if any) alongside the still-animating-out
/// `previous_children` (oldest first).
pub type AnimatedSwitcherLayoutBuilder = Rc<dyn Fn(Option<BoxedView>, Vec<BoxedView>) -> BoxedView>;

/// Cross-fades between children, keyed by [`View::can_update`]
/// (same concrete type and same key).
///
/// Setting [`AnimatedSwitcher::child`] to a widget that is NOT
/// `can_update`-compatible with the previous one starts a transition: the old
/// child animates out along `switch_out_curve` while the new one animates in
/// along `switch_in_curve`, both over `duration` (or `reverse_duration` for
/// the outgoing run, if set). A `can_update`-compatible child instead updates
/// the existing entry in place — no transition restarts. Setting the SAME
/// key on a new child that is mid-transition-out (e.g. a value oscillating A
/// → B → A faster than `duration`) does not collapse into the old outgoing
/// entry; it starts its own fresh entry, since `can_update` never unifies two
/// different entries.
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
    /// `Curves::Linear` in both directions (deliberately
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
        builder: impl Fn(BoxedView, std::rc::Rc<dyn Animation<f64>>) -> BoxedView + 'static,
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
    /// `FadeTransition` has no `.key(...)` setter (a keyed `impl View`
    /// cannot come from `impl_animated_view!`'s generated block — see
    /// `flui-macros`' "Keyed widgets" doc), so the child's own key is not
    /// re-applied here; the entry's OWN stable identity (this builder's
    /// caller wraps the result in a per-entry key) is what carries the slot.
    pub fn default_transition_builder(
        child: BoxedView,
        animation: std::rc::Rc<dyn Animation<f64>>,
    ) -> BoxedView {
        FadeTransition::new(animation, child).boxed()
    }

    /// The default `layout_builder`: a [`Stack`] centering every
    /// still-animating entry, oldest `previous_children` first, then
    /// `current_child` last (so it paints on top).
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
/// `Vec` every time. The key is the entry's
/// `child_number`, never rebuilt once assigned, so calls to
/// [`ChildEntry::update_transition`] can swap the wrapped content without
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
/// `child` was set to but is still transitioning.
struct ChildEntry {
    /// This entry's stable identity — assigned once at creation, carried
    /// unchanged by its [`KeyedEntry`] wrapper for the entry's whole life.
    child_number: u64,
    /// The transition's driver. Runs forward while incoming, reverse once
    /// demoted to outgoing.
    controller: DrivenController,
    /// `controller`, eased by `switch_in_curve` going forward and
    /// `switch_out_curve` going backward — what `transition_builder`
    /// actually animates against.
    curved: Terminal<CurvedAnimation<ArcCurve>>,
    status_subscription: Option<flui_animation::StatusSubscription>,
    /// Flipped by the status-listener callback when `controller` reaches
    /// [`AnimationStatus::Dismissed`] (a completed reverse run). Read — and
    /// acted on — by [`AnimatedSwitcherState::build`]'s sweep; see the
    /// module docs for why the listener cannot dispose the entry itself.
    dismissed: Rc<Cell<bool>>,
    /// The child widget this entry was built from, used to detect a
    /// same-entry rebuild (`View::can_update`) and to re-run
    /// `transition_builder` on demand.
    widget_child: Terminal<BoxedView>,
    /// The cached, already-built (and stably keyed) transition widget.
    transition: Terminal<Rc<BoxedView>>,
}

impl ChildEntry {
    /// A fresh entry wrapping `child`, at rest (`animate: false`, the very
    /// first entry created from `create_state`) or animating in
    /// (`animate: true`, every later swap). Builds the controller, curve,
    /// and initial transition, but does NOT register with a `Vsync` or
    /// attach the dismissal status-listener — [`ChildEntry::register`] does
    /// that once a [`BuildContext`] is available (`create_state` has none;
    /// see `AnimatedSwitcherState::init_state`).
    #[expect(clippy::too_many_arguments)] // one argument per entry parameter
    fn new(
        child: BoxedView,
        child_number: u64,
        duration: Duration,
        reverse_duration: Option<Duration>,
        switch_in_curve: ArcCurve,
        switch_out_curve: ArcCurve,
        transition_builder: &AnimatedSwitcherTransitionBuilder,
        animate: bool,
        vsync: Option<&Vsync>,
    ) -> Self {
        // No ticker: `Vsync` drives this controller once registered (see
        // `ChildEntry::register`).
        let child = Terminal::new(child);
        let controller = AnimationController::builder(duration).build_on(vsync);
        if let Some(reverse_duration) = reverse_duration {
            controller
                .controller()
                .set_reverse_duration(reverse_duration);
        }
        let parent: std::rc::Rc<dyn Animation<f64>> =
            std::rc::Rc::new(controller.controller().clone());
        let curved = Terminal::new(
            CurvedAnimation::new(parent, switch_in_curve).with_reverse_curve(switch_out_curve),
        );

        if animate {
            // A new entry animates in.
            let _ = controller.controller().forward();
        } else {
            // The very first entry sits
            // at rest, fully switched in, no motion.
            controller.controller().set_value(1.0);
        }

        let transition = Self::build_transition(child_number, &child, &curved, transition_builder);

        Self {
            child_number,
            controller,
            curved,
            status_subscription: None,
            dismissed: Rc::new(Cell::new(false)),
            widget_child: child,
            transition: Terminal::new(Rc::new(transition)),
        }
    }

    /// Register with `vsync` (if any) and attach the dismissal
    /// status-listener, which flips [`ChildEntry::dismissed`] and schedules
    /// `rebuild` when `controller` reaches
    /// [`AnimationStatus::Dismissed`].
    fn register(&mut self, vsync: Option<Vsync>, rebuild: RebuildHandle) {
        self.rebind(vsync.as_ref());

        let dismissed = Rc::clone(&self.dismissed);
        drop(self.status_subscription.take());
        self.status_subscription = Some(self.controller.controller().subscribe_status(
            std::rc::Rc::new(move |status| {
                if status == AnimationStatus::Dismissed {
                    dismissed.set(true);
                    rebuild.schedule(flui_view::RebuildReason::AnimationTick);
                }
            }),
        ));
    }

    /// Re-run `transition_builder` over the current `widget_child`/`curved`,
    /// preserving this entry's key. Called both when
    /// `transition_builder` itself changes and when a `can_update`-compatible
    /// child rebuilds the current entry in place.
    fn rebind(&mut self, vsync: Option<&Vsync>) {
        if let Err(error) = self.controller.rebind(vsync) {
            tracing::error!(%error, "AnimatedSwitcher lost its frame registry");
        }
    }

    fn update_transition(&mut self, transition_builder: &AnimatedSwitcherTransitionBuilder) {
        let transition = Self::build_transition(
            self.child_number,
            &self.widget_child,
            &self.curved,
            transition_builder,
        );
        let previous = std::mem::replace(&mut self.transition, Terminal::new(Rc::new(transition)));
        drop(previous);
    }

    fn build_transition(
        child_number: u64,
        widget_child: &BoxedView,
        curved: &CurvedAnimation<ArcCurve>,
        transition_builder: &AnimatedSwitcherTransitionBuilder,
    ) -> BoxedView {
        let animation: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(curved.clone());
        let content = transition_builder(widget_child.clone(), animation);
        KeyedEntry::new(child_number, content).boxed()
    }

    /// Detach the status listener, unregister from `vsync`, and dispose the
    /// controller.
    fn dispose(&mut self) {
        drop(self.status_subscription.take());
        self.controller.dispose();
    }
}

impl Drop for ChildEntry {
    fn drop(&mut self) {
        // Logical cancellation still runs during incoming unwind. Independent
        // authored values use terminal slots, so generated field cleanup cannot
        // run their destructors after the first failure. Vec cleanup consequently
        // withdraws every remaining driver without retiring its opaque captures.
        self.dispose();
    }
}

impl std::fmt::Debug for ChildEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildEntry")
            .field("child_number", &self.child_number)
            .field("status", &self.controller.controller().status())
            .field("dismissed", &self.dismissed.get())
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedSwitcher`]. See the module docs for why
/// `outgoing_entries` needs interior mutability.
pub struct AnimatedSwitcherState {
    current_entry: Option<ChildEntry>,
    outgoing_entries: RefCell<Vec<ChildEntry>>,
    /// Monotonically increasing entry counter — the source of each
    /// [`ChildEntry::child_number`].
    child_number: u64,
    /// Captured in `init_state` (unavailable in `create_state`, which has no
    /// `BuildContext`); reused for every later entry `did_update_view`
    /// creates. `None` only in the pre-`init_state` window.
    rebuild: Option<RebuildHandle>,
    vsync: Option<Vsync>,
}

impl std::fmt::Debug for AnimatedSwitcherState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outgoing_count = self.outgoing_entries.borrow().len();
        f.debug_struct("AnimatedSwitcherState")
            .field("has_current_entry", &self.current_entry.is_some())
            .field("outgoing_count", &outgoing_count)
            .finish_non_exhaustive()
    }
}

impl StatefulView for AnimatedSwitcher {
    type State = AnimatedSwitcherState;

    fn create_state(&self) -> Self::State {
        // The initial entry is added without animating. FLUI splits controller
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
                None,
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
    /// child.
    fn add_entry_for_new_child(&mut self, view: &AnimatedSwitcher, animate: bool) {
        debug_assert!(
            animate || self.current_entry.is_none(),
            "BUG: a non-animated entry replacement is only valid for the very first entry"
        );
        if let Some(old_entry) = self.current_entry.take() {
            debug_assert!(animate, "BUG: demoting a current entry always animates");
            let _ = old_entry.controller.controller().reverse();
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
            self.vsync.as_ref(),
        );
        if let Some(rebuild) = self.rebuild.clone() {
            entry.register(self.vsync.clone(), rebuild);
        }
        self.current_entry = Some(entry);
    }
}

impl ViewState<AnimatedSwitcher> for AnimatedSwitcherState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rebuild = ctx.rebuild_handle();
        let vsync = VsyncScope::maybe_of(ctx);
        if let Some(entry) = self.current_entry.as_mut() {
            entry.register(vsync.clone(), rebuild.clone());
        }
        self.rebuild = Some(rebuild);
        self.vsync = vsync;
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        if let Err(error) = VsyncUpdate::run(|update| {
            if let Some(entry) = self.current_entry.as_mut() {
                update.rebind_controller(&mut entry.controller, self.vsync.as_ref());
            }
            for entry in self.outgoing_entries.get_mut() {
                update.rebind_controller(&mut entry.controller, self.vsync.as_ref());
            }
        }) {
            tracing::error!(%error, "AnimatedSwitcher lost its frame registry");
        }
    }

    fn build(&self, view: &AnimatedSwitcher, _ctx: &dyn BuildContext) -> impl IntoView {
        // Sweep entries whose reverse run dismissed since the last build — see
        // the module docs for why this cannot happen inside the status
        // listener itself.
        let dismissed: Vec<_> = self
            .outgoing_entries
            .borrow_mut()
            .extract_if(.., |entry| entry.dismissed.get())
            .collect();
        drop(dismissed);

        let current_child_number = self.current_entry.as_ref().map(|entry| entry.child_number);
        let current_transition = self
            .current_entry
            .as_ref()
            .map(|entry| Rc::clone(&entry.transition));
        // An outgoing entry sharing the current entry's key is suppressed from
        // the previous children: the same `child_number` never appears in both
        // lists at once.
        let previous_transitions: Vec<_> = {
            let outgoing = self.outgoing_entries.borrow();
            outgoing
                .iter()
                .filter(|entry| Some(entry.child_number) != current_child_number)
                .map(|entry| Rc::clone(&entry.transition))
                .collect()
        };

        // Snapshot ownership under the borrow; authored View::clone runs only
        // after the entry collection is available again.
        let current_transition = current_transition.map(|transition| transition.as_ref().clone());
        let previous_transitions = previous_transitions
            .into_iter()
            .map(|transition| transition.as_ref().clone())
            .collect();
        (view.layout_builder)(current_transition, previous_transitions)
    }

    fn did_update_view(&mut self, old_view: &AnimatedSwitcher, new_view: &AnimatedSwitcher) {
        // A `transition_builder` swap rebuilds every cached transition in
        // place, preserving each entry's key.
        if !Rc::ptr_eq(&old_view.transition_builder, &new_view.transition_builder) {
            for entry in self.outgoing_entries.get_mut() {
                entry.update_transition(&new_view.transition_builder);
            }
            if let Some(entry) = self.current_entry.as_mut() {
                entry.update_transition(&new_view.transition_builder);
            }
        }

        // A new entry is needed when a child appeared or disappeared, or when
        // the new child cannot update the current entry's child in place.
        let needs_new_entry = match (new_view.child.as_ref(), self.current_entry.as_ref()) {
            (Some(new_child), Some(entry)) => !new_child.can_update(&*entry.widget_child),
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
            let previous = std::mem::replace(&mut entry.widget_child, Terminal::new(new_child));
            drop(previous);
            entry.update_transition(&new_view.transition_builder);
        }
    }

    fn dispose(&mut self) {
        let current = self.current_entry.take();
        let outgoing = std::mem::take(self.outgoing_entries.get_mut());
        drop(current);
        drop(outgoing);
    }
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[derive(Clone)]
    struct Capture {
        drops: Rc<Cell<usize>>,
        armed: Rc<Cell<bool>>,
        fails: bool,
        label: &'static str,
    }

    impl View for Capture {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl StatelessView for Capture {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            crate::SizedBox::new(10.0, 10.0)
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            if self.armed.get() {
                self.drops.set(self.drops.get() + 1);
                assert!(!self.fails, "{}", self.label);
            }
        }
    }

    // A mounted actor contains build failures. The private entry seam isolates
    // physical destruction, including an incoming unwind and independent tails.
    fn exercise(child_failure: bool, transition_failure: bool, incoming: bool) {
        let vsync = Vsync::new();
        let armed = Rc::new(Cell::new(false));
        let drops: [Rc<Cell<usize>>; 4] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
        let mut entries = Vec::new();
        let mut futures = Vec::new();
        for index in 0..2 {
            let transition = Capture {
                drops: drops[index * 2 + 1].clone(),
                armed: armed.clone(),
                fails: transition_failure,
                label: "switcher transition retirement",
            };
            let builder: AnimatedSwitcherTransitionBuilder =
                Rc::new(move |_child, _animation| transition.clone().boxed());
            let entry = ChildEntry::new(
                Capture {
                    drops: drops[index * 2].clone(),
                    armed: armed.clone(),
                    fails: child_failure,
                    label: "switcher child retirement",
                }
                .boxed(),
                index as u64,
                Duration::from_secs(1),
                None,
                ArcCurve::new(Curves::Linear),
                ArcCurve::new(Curves::Linear),
                &builder,
                true,
                Some(&vsync),
            );
            futures.push(
                entry
                    .controller
                    .controller()
                    .forward()
                    .expect("live entry starts"),
            );
            entries.push(entry);
        }
        armed.set(true);
        let outcome = catch_unwind(AssertUnwindSafe(move || {
            let _entries = entries;
            assert!(!incoming, "switcher incoming failure");
        }));
        let expected_failure = if incoming {
            Some("switcher incoming failure")
        } else if child_failure {
            Some("switcher child retirement")
        } else if transition_failure {
            Some("switcher transition retirement")
        } else {
            None
        };
        match expected_failure {
            Some(expected) => {
                let failure = outcome.expect_err("authored retirement must fail");
                assert_eq!(
                    flui_foundation::panic::payload_text(failure.as_ref()),
                    Some(expected)
                );
            }
            None => assert!(outcome.is_ok()),
        }
        let expected_drops = if incoming {
            [0, 0, 0, 0]
        } else if child_failure {
            [1, 0, 0, 0]
        } else if transition_failure {
            [1, 1, 0, 0]
        } else {
            [1, 1, 1, 1]
        };
        assert_eq!(drops.each_ref().map(|count| count.get()), expected_drops);
        assert!(
            vsync.is_empty(),
            "every driver releases its seat, including the tail"
        );
        assert!(
            futures
                .iter()
                .all(flui_animation::AnimationRunFuture::is_canceled)
        );
    }

    fn healthy() {
        exercise(false, false, false);
    }
    fn child_failure() {
        exercise(true, false, false);
        healthy();
    }
    fn transition_failure() {
        exercise(false, true, false);
        healthy();
    }
    fn competing() {
        exercise(true, true, false);
        healthy();
    }
    fn incoming() {
        exercise(true, true, true);
        healthy();
    }

    crate::support::child_process::child_test! {
        fn switcher_entry_retirement_preserves_logical_cleanup() {
            crate::support::test_cases::run_cases("switcher entry retirement", &[
                ("healthy", healthy),
                ("child_failure", child_failure),
                ("transition_failure", transition_failure),
                ("competing", competing),
                ("incoming", incoming),
            ]);
        }
    }
}

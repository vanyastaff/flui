//! Platform accessibility wiring of a presentation: the bridge's three directions,
//! the owner-thread reconcile of semantics enablement and the dispatch of an
//! accessibility action.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use flui_platform::PlatformWindow;
use flui_rendering::pipeline::PipelineCell;
use flui_semantics::platform::PlatformAccessibility;
use flui_semantics::{SemanticsActionError, SemanticsActionRequest};

use super::{PresentationLifecycle, PresentationState};
use crate::semantics_host::SemanticsHost;

impl PresentationState {
    /// Wire this window's platform accessibility bridge, when one exists,
    /// into all three directions of the semantics seam:
    ///
    /// - **Out** — every `SemanticsOwner` this pipeline creates publishes
    ///   its translated tree to the platform. The callback holds the bridge
    ///   `Weak`, so a flush racing window teardown degrades to a drop,
    ///   never a call into a dead adapter.
    /// - **Activation** — assistive technology attaching or detaching
    ///   toggles this presentation's [`SemanticsHost`] flag and wakes the
    ///   loop; the frame pump's reconcile (`UiRuntime::draw_frame_entered`)
    ///   then drives `PipelineOwner::set_semantics_enabled`, so tree
    ///   assembly starts and stops on the OWNER thread — the listener runs
    ///   on the adapter's own thread and touches only `Send + Sync` state.
    /// - **In** — action requests marshal through the UI runtime inbox as
    ///   [`HostInput::Accessibility`](crate::HostInput::Accessibility) stamped
    ///   for this exact presentation and resolve at the next Idle drain. Requests FLUI cannot route (a
    ///   zero node id, an action with no counterpart, a full inbox) are
    ///   traced drops, since screen readers may act on a stale snapshot. Typed action payloads
    ///   (`accesskit::ActionData`) translate with the whole request through
    ///   [`semantics_action_request_for`](flui_semantics::semantics_action_request_for). Numeric setters remain distinct from
    ///   text edits, and expand/collapse preserve their explicit direction.
    ///   The current owner validates payloads before invoking a handler.
    ///
    /// A window without the capability (`bridge` is `None`) wires nothing:
    /// the pipeline keeps its documented publish-nowhere placeholder.
    pub(super) fn wire_platform_accessibility(
        window: &Rc<dyn PlatformWindow>,
        bridge: Option<&Arc<dyn PlatformAccessibility>>,
        pipeline: &PipelineCell,
        semantics: &SemanticsHost,
        wake: &Arc<dyn Fn() + Send + Sync>,
        command_sender: crate::ui_runtime::UiCommandSender,
    ) {
        let Some(bridge) = bridge else {
            return;
        };

        let publish_bridge = Arc::downgrade(bridge);
        pipeline.with_mut(|owner| {
            owner.set_semantics_update_callback(Rc::new(
                move |update: &flui_semantics::TreeUpdate| {
                    if let Some(bridge) = publish_bridge.upgrade() {
                        bridge.publish(update.clone());
                    }
                },
            ));
            // A bridge with a native notice of a new screen is told each route
            // change; any other leaves the owner to announce it (ADR-0204 §5).
            let sink = (bridge.route_change_support()
                == flui_semantics::platform::RouteChangeSupport::Native)
                .then(|| {
                    let bridge = Arc::downgrade(bridge);
                    Rc::new(move |change: flui_semantics::platform::RouteChange| {
                        if let Some(bridge) = bridge.upgrade() {
                            bridge.route_changed(change);
                        }
                    }) as flui_semantics::platform::RouteChangeSink
                });
            owner.set_semantics_route_change_sink(sink);
        });

        let enabled_flag = semantics.platform_semantics_enabled_handle();
        let republish_flag = semantics.full_republish_handle();
        let wake = Arc::clone(wake);
        let awaken_window = window.redraw_requester();
        let detached = command_sender.clone();
        bridge.set_activation_listener(Arc::new(move |active| {
            enabled_flag.store(active, Ordering::Relaxed);
            if active {
                // A (re)attached assistive technology's state is unknown —
                // it may have forgotten everything — and flushes publish
                // incrementally, so it must be answered with a
                // self-contained full tree, not the next diff.
                republish_flag.store(true, Ordering::Relaxed);
            } else if let Err(error) = detached.send_assistive_focus(None) {
                // Its cursor left with it, whether or not the adapter said so.
                tracing::warn!(
                    ?error,
                    "dropping the end of the assistive cursor: the ui_runtime inbox is full or gone"
                );
            }
            // The flag alone changes nothing until a frame runs: wake the
            // loop and poke this window so the reconcile actually happens.
            // The adapter calls from its own thread, so the window is asked
            // through its requester (ADR-0254 §5).
            wake();
            awaken_window.request_redraw();
        }));
        // Assistive technology may have attached before this window existed
        // — the transition the listener waits for has already happened.
        if bridge.is_active() {
            semantics.set_platform_semantics_enabled(true);
        }

        let cursor = command_sender.clone();
        bridge.set_assistive_focus_listener(Arc::new(move |node| {
            // Transported like an action, so the two apply in the order the
            // platform reported them.
            if let Err(error) = cursor.send_assistive_focus(node) {
                tracing::warn!(
                    ?error,
                    "dropping an assistive cursor move: the ui_runtime inbox is full or gone"
                );
            }
        }));
        bridge.set_action_listener(Arc::new(move |request| {
            // The adapter's thread only transports the request: it reaches
            // the owner as `HostInput::Accessibility` and is decoded there
            // (ADR-0207 §1).
            if let Err(error) = command_sender.send_accessibility(request) {
                tracing::warn!(
                    ?error,
                    "dropping accessibility action: the ui_runtime inbox is full or gone"
                );
            }
        }));
    }

    /// Reconcile platform-driven semantics enablement onto this
    /// presentation's pipeline — the owner-thread half of the activation
    /// seam.
    ///
    /// The platform's activation listener may only flip the
    /// [`SemanticsHost`] flag and wake the loop (it runs on the adapter's
    /// thread); this is where the flag becomes pipeline state. Called at
    /// each segment start in `UiRuntime::draw_frame_entered`, BEFORE dirty
    /// sampling, because enabling seeds the root as needing semantics —
    /// exactly the pending work the segment should then observe. A no-op
    /// whenever flag and pipeline already agree, which is every frame but
    /// the two transitions.
    pub(crate) fn reconcile_semantics_enablement(&self) {
        let wanted = self.semantics.semantics_enabled();
        // Consumed unconditionally: a request that arrives alongside a
        // deactivation must not linger and fire on some later re-enable.
        let full_republish = self.semantics.take_full_republish_request();
        self.pipeline.with_mut(|owner| {
            if owner.semantics_enabled() != wanted {
                owner.set_semantics_enabled(wanted);
            }
            if full_republish && wanted {
                // Activation with an owner already alive (a screen reader
                // restarting without an intervening deactivation, or one
                // attaching after a handle enabled semantics first): the
                // adapter must be re-answered with a self-contained tree.
                // On the enable transition just above this is a no-op-shaped
                // reinforcement — the fresh owner's first flush is full
                // anyway.
                owner.request_semantics_full_publish();
            }
        });
        // What a view keeps only for a collected tree (a `Semantics` action
        // route) follows the pipeline in this frame's build.
        self.widgets.set_collects_semantics(wanted);
    }

    /// Resolve an accessibility action through this presentation's exact
    /// semantics owner, then invoke it after releasing the pipeline
    /// checkout.
    ///
    /// `debug_assert!(is_free())` makes the Idle-commit contract this
    /// dispatch site depends on an explicit, production-checked invariant:
    /// `UiRuntime::drain_commands` (the sole caller) only runs at a frame
    /// boundary, so nothing should still hold the pipeline checked out by
    /// the time a semantics-action handler runs. The `flui-testing` test
    /// `an_action_sent_off_thread_is_applied_by_the_next_harness_pump` runs
    /// through this assert.
    pub(crate) fn dispatch_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), SemanticsActionError> {
        if self.closing_requested.get()
            || matches!(
                self.lifecycle.get(),
                PresentationLifecycle::Closing | PresentationLifecycle::Closed
            )
        {
            return Err(SemanticsActionError::PresentationClosed);
        }
        let invocation = self
            .pipeline
            .with(|pipeline| pipeline.resolve_semantics_action(request))?;
        debug_assert!(
            self.pipeline.is_free(),
            "BUG: the pipeline must be free before invoking a semantics-action handler — \
             drain_commands runs only at a frame boundary, so nothing should still hold it \
             checked out here"
        );
        invocation.invoke();
        Ok(())
    }
}

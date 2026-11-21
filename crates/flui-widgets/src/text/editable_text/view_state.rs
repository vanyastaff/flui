//! The lifecycle of an [`EditableTextState`]: mounting, updates, dependencies,
//! the build of the field and disposal.

use std::{cell::Cell, rc::Rc};

use flui_foundation::geometry::Bounds;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{Listenable, TextDirection};
use flui_interaction::pointer::HitTestBehavior;
use flui_interaction::text_input::{ClipboardCapability, TextInputClient};
use flui_platform::text::store::OwnerCalls;
use flui_view::environment::MediaQuery;
use flui_view::prelude::*;

use super::field_semantics::FieldSemanticsActions;
use super::field_view::{FieldAppearance, FieldMount, build_field_view};
use super::{
    CaretHandle, CursorAreaLoop, EditableText, EditableTextState, FieldCommands, HandleParts,
    ImeFocusTransition, LongPressSelection, claim_primary,
};
use crate::AnimatedBuilder;
use crate::text::primary_selection::PrimarySelectionOwner;
use crate::text::selection_area::handles::Look;
use crate::text::selection_style::DefaultSelectionStyle;
use crate::text::text_store::{EditableTextStore, FieldParts};
use crate::{DefaultSelectionControls, DotHandles};

impl ViewState<EditableText> for EditableTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // Resolve the focus parent first, but attach only after every
        // focus/IME listener is installed below. An external node may carry a
        // focus request queued before mount; `attach_node` fulfills it
        // synchronously, and no listener may miss that edge.
        let parent = crate::__private::enclosing_focus_parent(ctx);
        self.parent = Some(Rc::clone(&parent));
        let (rect_provider, rect_provider_registration) =
            crate::__private::install_rect_provider(&self.focus_node, &self.anchor, ctx);
        self.rect_provider = Some(rect_provider);
        self.rect_provider_registration = Some(rect_provider_registration);

        // 1b. The field's text store (ADR-0090), built before the key
        //     handler so a key edit can run the grants queued ahead of it.
        //     `text_input_handle()` and `pipeline_owner()` are frame
        //     capabilities — acquired here, in `init_state`, never in `build`
        //     (see `LifecycleContext::text_input_handle`'s doc) — and stored
        //     so the focus-listener closure below and `dispose` reach them.
        self.ime_handle = ctx.text_input_handle();
        self.pipeline_owner = ctx.pipeline_owner();
        // The clipboard first: the store performs a platform's edit commands
        // through the field's clipboard actions.
        self.clipboard = ctx.capability::<ClipboardCapability>().ok();
        self.primary = Rc::new(PrimarySelectionOwner::acquire(ctx));
        self.text_store = Some(EditableTextStore::new(FieldParts {
            controller: Rc::clone(&self.controller),
            handle: self.ime_handle.clone(),
            pipeline: self.pipeline_owner.clone(),
            inner_anchor: self.inner_anchor.clone(),
            press_probe: Rc::clone(&self.press_probe),
            obscure: Rc::clone(&self.obscure),
            text_input: Rc::clone(&self.text_input),
            obscuring_character: Rc::clone(&self.obscuring_character),
            edits: self.edit_observer(),
            commands: FieldCommands {
                clipboard: self.clipboard_action(),
                select_all: self.select_all_action(),
            },
        }));

        // 2. Install the key handler on the node itself. It only fires when
        //    this node is on the primary-focus dispatch path. Gated on
        //    `can_request_focus` (kept in sync with `enabled` in
        //    `did_update_view`) so a stray dispatch to an already-focused
        //    field that has since been disabled is a no-op.
        self.key_handler_registration = Some(
            self.focus_node
                .register_on_key_event(self.key_handler(&self.focus_node)),
        );

        // 2b. The clipboard actions, recorded on the node beside the key
        //     handler: a clipboard chord the handler leaves unconsumed
        //     reaches the `Shortcuts` above, which resolves it here.
        self.record_action_chain(ctx);

        // 3. Forward controller change events into the rebuild notifier so the
        //    inner AnimatedBuilder rebuilds on every keystroke, and claim the
        //    selection they leave.
        let listener = self.controller_listener();
        self.controller_listener_id = Some(self.controller.borrow().listen(listener));
        // A selection the controller already held is claimed now: no change
        // will report it.
        claim_primary(
            &self.primary,
            &Rc::downgrade(&self.controller),
            &self.obscure,
        );

        // 4. Forward FocusManager focus-change events into the rebuild notifier
        //    so the caret appears / disappears immediately when this field
        //    gains or loses focus. Removed by id in `dispose`.
        let rebuild_notifier_for_focus = self.rebuild_notifier.clone();
        let focus_node_for_rebuild = Rc::clone(&self.observed_focus_node);
        self.focus_listening = Some(self.focus_manager.listen(move |previous, current| {
            let focus_node = focus_node_for_rebuild.borrow();
            // Only rebuild when this node's focus state actually changed.
            let was_focused = previous
                .as_ref()
                .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
            let now_focused = current
                .as_ref()
                .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
            if was_focused != now_focused {
                rebuild_notifier_for_focus.notify_listeners();
            }
        }));

        // 5. Attach/detach the IME client on this field's own focus
        //    transitions, through the handle acquired in 1b.
        //    `post_frame_handle()` is acquired here for the same
        //    reason — the IME cursor-area loop (ADR-0030) it drives is
        //    started/stopped by the same closure.
        self.post_frame_handle = ctx.post_frame_handle();
        let ime_handle_for_focus = self.ime_handle.clone();
        let post_frame_handle_for_focus = self.post_frame_handle.clone();
        let pipeline_owner_for_focus = self.pipeline_owner.clone();
        let inner_anchor_for_focus = self.inner_anchor.clone();
        let store_for_ime = self
            .text_store
            .clone()
            .expect("BUG: init_state builds the text store before the IME listener");
        let ime_token_for_focus = Rc::clone(&self.ime_token);
        let commit_for_focus = self.composition_commit();
        let cursor_area_alive_for_focus = Rc::clone(&self.cursor_area_alive);
        let platform_selection = Rc::clone(&self.platform_selection);
        let handles_for_platform = Rc::downgrade(&self.touch_handles);
        let ime_focus_transition: ImeFocusTransition = Rc::new(move |now_focused| {
            let Some(handle) = &ime_handle_for_focus else {
                return;
            };
            if now_focused {
                // Fresh per-attach state (ADR-0030): `last_sent` resets
                // so a brand-new IME session always gets its first rect
                // even at an unchanged caret position, and `alive` is a
                // NEW flag so a stale queued firing from a previous
                // attach (see the field's `cursor_area_alive` doc) can
                // never resurrect this session or run alongside it.
                let last_sent: Rc<Cell<Option<Bounds<f64>>>> = Rc::new(Cell::new(None));
                let alive = Rc::new(Cell::new(true));
                let _prev = cursor_area_alive_for_focus
                    .borrow_mut()
                    .replace(Rc::clone(&alive));

                let last_sent_for_session = Rc::clone(&last_sent);
                let store: Rc<dyn flui_platform::text::store::TextStore> = store_for_ime.clone(); // the presentation holds this field through the erased text-store contract.
                // The backend may have restarted the IME session (`Enabled`
                // re-fires on that restart) — clearing `last_sent`
                // guarantees the new session gets a fresh rect instead of
                // the dedupe cache silently suppressing it.
                let client = TextInputClient::new(store)
                    .on_session_start(move || last_sent_for_session.set(None));
                let token = match handle.attach(client) {
                    Ok(token) => token,
                    Err(error) => {
                        alive.set(false);
                        let _prev = cursor_area_alive_for_focus.borrow_mut().take();
                        tracing::warn!(?error, "IME client could not attach to its presentation");
                        return;
                    }
                };
                *ime_token_for_focus.borrow_mut() = Some(token);
                // A host that draws the selection UI takes it over now: what
                // the field showed of its own (the handles and toolbar a long
                // press left as it focused the field) goes; its failure is
                // resumed once the cursor-area loop below has started.
                let mut failure = FirstPanic::new();
                platform_selection.follow(Some(handle.selection_ui(token)));
                if platform_selection.is_active()
                    && let Some(handles) = handles_for_platform.upgrade()
                {
                    handles.platform_took_over(&mut failure);
                }

                if let Some(post_frame) = post_frame_handle_for_focus.clone() {
                    CursorAreaLoop {
                        post_frame,
                        pipeline_owner: pipeline_owner_for_focus.clone(),
                        inner_anchor: inner_anchor_for_focus.clone(),
                        text_input: handle.clone(),
                        store: Rc::downgrade(&store_for_ime),
                        alive,
                        last_sent,
                        last_placement: Rc::new(Cell::new(
                            super::cursor_area::SeenPlacement::Unread,
                        )),
                    }
                    .start();
                } else {
                    tracing::warn!(
                        "IME cursor-area tracking not started: no post-frame handle \
                         installed (the platform candidate window will not follow \
                         the caret)"
                    );
                }
                failure.resume();
            } else {
                // The field draws its own selection UI again once it no
                // longer holds the input.
                platform_selection.follow(None);
                // The composition is committed while the token still names
                // this field's client, then the client detaches; the detach
                // runs though the commit's owner code failed (ADR-0142
                // items 4 and 8).
                let mut calls = OwnerCalls::new();
                commit_for_focus.run(&mut calls);
                // The token is taken in a statement of its own: a borrow
                // held across the detach (which runs other code) would be
                // refused by a reentrant focus change.
                let token = ime_token_for_focus.borrow_mut().take();
                if let Some(token) = token
                    && let Some(Err(error)) = calls.run(|| handle.detach(token))
                {
                    calls.run(|| {
                        tracing::trace!(
                            ?error,
                            "IME detach reached a presentation that was already closing"
                        );
                    });
                }
                let alive = cursor_area_alive_for_focus.borrow_mut().take();
                if let Some(alive) = alive {
                    alive.set(false);
                }
                calls.resume();
            }
        });
        self.ime_focus_transition = Some(Rc::clone(&ime_focus_transition));
        let focus_node_for_ime = Rc::clone(&self.observed_focus_node);
        let handles_for_focus = Rc::downgrade(&self.touch_handles);
        let drag_for_focus = Rc::downgrade(&self.selection_drag);
        self.ime_focus_listening = Some(self.focus_manager.listen(move |previous, current| {
            let focus_node = focus_node_for_ime.borrow();
            let was_focused = previous
                .as_ref()
                .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
            let now_focused = current
                .as_ref()
                .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
            drop(focus_node);
            if was_focused == now_focused {
                return;
            }
            // The focus leaving takes a touch's handles and the contact
            // still down, whose release would show them again; the input
            // method still hears of it though that fails.
            let mut failure = FirstPanic::new();
            if !now_focused && let Some(drag) = drag_for_focus.upgrade() {
                drag.set(None);
            }
            if !now_focused && let Some(handles) = handles_for_focus.upgrade() {
                handles.abandon(&mut failure);
            }
            failure.contain(|| ime_focus_transition(now_focused));
            failure.resume();
        }));

        self.touch_handles.mount(HandleParts {
            pipeline: self.pipeline_owner.clone(),
            inner_anchor: self.inner_anchor.clone(),
            controller: Rc::clone(&self.controller),
            obscure: Rc::clone(&self.obscure),
            obscuring_character: Rc::clone(&self.obscuring_character),
            repaint: self.rebuild_notifier.clone(),
            commit: self.composition_commit(),
            post_frame: self.post_frame_handle.clone(),
            platform_selection: Rc::clone(&self.platform_selection),
        });
        self.follow_handle_scope(ctx);

        // Attach last. Besides normal pre-mount `request_focus`, this also
        // covers a route scope's pending first-focus intent: either path may
        // synchronously focus this node during attachment, after both the
        // caret rebuild and IME listeners are ready.
        let attachment = parent
            .attach_node(&self.focus_node)
            .expect("BUG: EditableText could not attach its explicit focus node");
        let previous = self.focus_attachment.replace(Some(Rc::new(attachment)));
        drop(previous);
        self.try_autofocus();
    }

    fn did_update_view(&mut self, _old_view: &EditableText, new_view: &EditableText) {
        self.accepts_semantics_actions.set(new_view.enabled);
        // A disabled field shows no handles and finishes no long press, and a
        // replaced controller's selection is not the one shown handles were
        // for.
        let replaced = !self
            .controller
            .borrow()
            .is_same_controller(&new_view.controller);
        if !new_view.enabled || replaced {
            let mut failure = FirstPanic::new();
            self.touch_handles.abandon(&mut failure);
            failure.resume();
        }
        if !new_view.enabled {
            self.selection_drag.set(None);
        }
        // Cheap and unconditional: a closure has no identity worth comparing,
        // so every rebuild just installs whatever `on_submitted` the latest
        // view carries — read through this cell at dispatch time by the key
        // handler installed once in `init_state`.
        self.on_submitted
            .borrow_mut()
            .clone_from(&new_view.on_submitted);
        self.on_changed
            .borrow_mut()
            .clone_from(&new_view.on_changed);
        // The platform hears of changes through observer code; every change
        // this update makes still lands though one of those calls failed,
        // and the first failure is resumed once the update is complete.
        let mut calls = OwnerCalls::new();
        let was_obscured = self.obscure.replace(new_view.obscure_text);
        self.obscuring_character.set(new_view.obscuring_character);
        // An obscured field holds no primary selection; the claim is
        // reconciled below, once the controller is the replacement's too, so
        // no backend sees the new flag with the old controller's text.
        let mut reclaim = was_obscured != new_view.obscure_text;
        let was_text_input = self.text_input.replace(new_view.text_input);
        if (was_obscured != new_view.obscure_text || was_text_input != new_view.text_input)
            && let Some(store) = &self.text_store
        {
            calls.run(|| store.status_changed());
        }

        // A parent rebuilding with a DIFFERENT controller retargets the
        // mounted field onto it, rather than the field silently going on
        // driving the one it was born with. The reference does the same in
        // `didUpdateWidget`: drop the listener from the old, add it to the
        // new, and resynchronise.
        //
        // Everything that reaches the controller — the key handler, the text
        // store, `build` — reads through `self.controller`'s cell at
        // use time, so writing the cell retargets all of them at once. Only
        // the change LISTENER has to move by hand, because it is registered on
        // the controller rather than read from it.
        if !self
            .controller
            .borrow()
            .is_same_controller(&new_view.controller)
        {
            self.selection_drag.set(None);
            // The subscription withdraws through its own channel — the old
            // controller's registrations cannot be detached by the new one's.
            drop(self.controller_listener_id.take());
            // The replaced controller retires inside the update's scope: it
            // may hold the last handle to listeners the application added.
            let replaced = std::mem::replace(
                &mut *self.controller.borrow_mut(),
                new_view.controller.clone(),
            );
            calls.retire(replaced);
            let listener = self.controller_listener();
            self.controller_listener_id = Some(self.controller.borrow().listen(listener));
            // The visible text is the replacement's now, and nothing else
            // will say so: the old controller's notifications are gone and the
            // new one has not changed since it was handed over. Its selection
            // is the one the primary selection holds now.
            calls.run(|| self.rebuild_notifier.notify_listeners());
            reclaim = true;
            // The input method's document is the replacement's too.
            if let Some(store) = &self.text_store {
                calls.run(|| store.controller_changed());
            }
        }
        if reclaim {
            let controller = Rc::downgrade(&self.controller);
            calls.run(|| claim_primary(&self.primary, &controller, &self.obscure));
        }

        if !Rc::ptr_eq(&self.focus_node, &new_view.focus_node) {
            let replacement = Rc::clone(&new_view.focus_node);
            replacement.set_can_request_focus(new_view.enabled);
            let replacement_key_handler_registration =
                replacement.register_on_key_event(self.key_handler(&replacement));
            let replacement_rect_provider_registration = self
                .rect_provider
                .as_ref()
                .map(|provider| replacement.register_rect_provider(Rc::clone(provider)));
            let replacement_action_chain_registration = self
                .focus_record
                .as_ref()
                .map(|record| record.register_on(&replacement));

            // Keep observing the old node while the transaction reports an
            // exact-primary loss, so the current IME session is detached.
            // A queued request on the replacement can be fulfilled within
            // the same transaction; reconcile that gain explicitly after
            // switching the observed identity.
            let attachment = self
                .focus_attachment
                .borrow()
                .clone()
                .expect("BUG: a mounted EditableText must retain its FocusAttachment");
            // A rejected replacement leaves the current authority in place.
            // Once replacement commits, the old generation is already stale
            // before focus listeners run, so semantic reentry is refused until
            // the returned replacement authority is installed below.
            //
            // The focus listeners the replacement notifies are application
            // code, run inside this update's scope. One that panics unwinds
            // out of the notifications after the replacement committed,
            // taking the returned handle and the notifications after it
            // (this field's own focus listener among them) with it: the
            // field re-adopts the replacement in place for a handle, then
            // reconciles its IME session with the focus the listeners left.
            //
            // A node the focus tree refuses (one attached to another field)
            // is a usage error, raised as the update's failure behind any
            // earlier one in its scope, so the update still completes: the
            // field keeps its node and handle, and the replacement's
            // registrations go with this block. It is never re-adopted.
            let outcome = calls.run(|| attachment.replace_node(&replacement));
            if let Some(Err(rejection)) = outcome {
                calls.run(|| {
                    panic!(
                        "BUG: EditableText could not atomically replace its focus node: {rejection}"
                    )
                });
            } else {
                let replaced = outcome.and_then(Result::ok);
                let notified = replaced.is_some();
                let replacement_attachment = replaced.or_else(|| {
                    let parent = self
                        .parent
                        .clone()
                        .expect("BUG: a mounted EditableText holds its focus parent");
                    calls
                        .run(|| parent.adopt_node(&replacement))
                        .map(|adopted| {
                            adopted.expect(
                                "BUG: EditableText could not adopt its replacement focus node",
                            )
                        })
                });

                self.key_handler_registration.take();
                self.rect_provider_registration.take();
                self.action_chain_registration.take();
                self.focus_node = replacement;
                self.key_handler_registration = Some(replacement_key_handler_registration);
                self.rect_provider_registration = replacement_rect_provider_registration;
                self.action_chain_registration = replacement_action_chain_registration;
                let observed = std::mem::replace(
                    &mut *self.observed_focus_node.borrow_mut(),
                    Rc::clone(&self.focus_node),
                );
                calls.retire(observed);
                // With no handle (the re-adoption failed too) the stale one
                // stays: it refuses semantic reentry, as the replacement's would
                // until installed.
                if let Some(replacement_attachment) = replacement_attachment {
                    let previous = self
                        .focus_attachment
                        .replace(Some(Rc::new(replacement_attachment)));
                    drop(previous);
                }

                if self.focus_node.has_primary_focus() {
                    calls.run(|| self.rebuild_notifier.notify_listeners());
                    if let Some(transition) = &self.ime_focus_transition {
                        calls.run(|| transition(true));
                    }
                } else if !notified && let Some(transition) = &self.ime_focus_transition {
                    // The notifications were cut short before this field heard
                    // its old node lose focus: its IME session ends here.
                    calls.run(|| transition(false));
                }
            }
        }

        // A field disabled while focused must not keep the caret and keyboard
        // input, so the field must unfocus when `enabled` flips false
        // mid-focus. `FocusNode::set_can_request_focus`
        // itself releases primary focus on a true-to-false change, so this
        // call alone covers the unfocus — no separate `has_primary_focus` check needed.
        //
        calls.run(|| self.focus_node.set_can_request_focus(new_view.enabled));
        // Last, behind every other change: the request runs focus listeners.
        self.autofocus = new_view.autofocus;
        calls.run(|| self.try_autofocus());
        calls.resume();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let parent = crate::__private::enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &parent))
        {
            let attachment = self
                .focus_attachment
                .borrow()
                .clone()
                .expect("BUG: a mounted EditableText must retain its FocusAttachment");
            attachment
                .reparent(&parent)
                .expect("BUG: EditableText could not reparent within its presentation");
            self.parent = Some(parent);
        }
        self.record_action_chain(ctx);
        self.follow_handle_scope(ctx);
    }

    fn build(&self, view: &EditableText, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = Rc::clone(&self.controller);
        let focus_node = Rc::clone(&self.focus_node);
        let semantics_actions = FieldSemanticsActions {
            focus_node: Rc::clone(&self.observed_focus_node),
            attachment: Rc::clone(&self.focus_attachment),
            enabled: Rc::clone(&self.accepts_semantics_actions),
            edits: self.edit_observer(),
            store: self.text_store.clone(),
            commit: self.composition_commit(),
        };
        let enabled = view.enabled;
        let appearance = FieldAppearance {
            text_sizing: MediaQuery::text_sizing_of(ctx),
            font_weight_adjustment: MediaQuery::font_weight_adjustment_of(ctx).unwrap_or(0),
            caret_height: view.caret_height,
            caret_color: view.caret_color,
            selection_style: view
                .selection_style
                .or_else(|| DefaultSelectionStyle::maybe_of(ctx))
                .unwrap_or_default(),
            text_style: view.text_style.clone(),
            obscure_text: view.obscure_text,
            obscuring_character: view.obscuring_character,
            semantics: view.field_semantics.clone(),
            platform_selection: self.platform_selection.is_active(),
        };
        let inner_anchor = self.inner_anchor.clone();
        let press_probe = Rc::clone(&self.press_probe);
        // The handles look as the area's do: given, a default's, else dots
        // in the opaque active selection colour.
        let look = view
            .handles
            .clone()
            .or_else(|| DefaultSelectionControls::handles_of(ctx))
            .map_or_else(
                || {
                    Look::Dots(DotHandles::new(
                        appearance.selection_style.active.with_alpha(255),
                    ))
                },
                Look::Given,
            );
        let platform = flui_view::environment::PlatformConvention::of(ctx);
        self.platform.set(platform);
        self.direction
            .set(crate::Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));
        let caret_handle = view
            .caret_handle
            .or_else(|| DefaultSelectionControls::caret_handle_of(ctx))
            .unwrap_or_else(|| CaretHandle::for_platform(platform));
        self.touch_handles.set_look(look, caret_handle);
        (self.touch_handles).set_long_press(
            view.long_press
                .or_else(|| DefaultSelectionControls::long_press_of(ctx))
                .unwrap_or_else(|| LongPressSelection::for_platform(platform)),
        );
        (self.touch_handles).set_magnifier(DefaultSelectionControls::magnifier_of(
            ctx,
            view.magnifier.as_ref(),
        ));
        let origin =
            crate::PopupOrigin::new(self.inner_anchor.clone(), self.pipeline_owner.clone());
        let chain = self.action_chain.clone();
        // The platform's edit menu stands in for the field's own while the
        // platform draws the selection UI: no toolbar, no menu key, no
        // secondary tap opens the field's.
        let menu = if appearance.platform_selection {
            let mut hidden = view.menu.clone();
            hidden.hide();
            hidden
        } else {
            view.menu.clone()
        };
        (self.touch_handles).menu_configured(ctx, &menu, chain, &origin);
        let touch_handles = Rc::clone(&self.touch_handles);

        // While the platform draws the selection UI, its gestures place the
        // caret and make the selection inside the field: the field's own
        // long press, taps and drags stay out of the arena and the contact.
        // The listener stays opaque, so a touch on the field still reaches
        // nothing behind it.
        let gestures = enabled && !appearance.platform_selection;
        // OUTSIDE the inner `AnchoredBox`, not inside it: the IME cursor-area
        // loop reaches the editable by taking `inner_anchor`'s FIRST child,
        // so anything inserted between the two would break that walk.
        let field = crate::interaction::Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .recognizer_when(&self.long_press.attachment, move |_down| gestures)
            .child(crate::__private::AnchoredBox::new(
                self.anchor.clone(),
                AnimatedBuilder::new(std::rc::Rc::new(self.rebuild_notifier.clone()), move || {
                    build_field_view(
                        &controller.borrow(),
                        &focus_node,
                        enabled,
                        &appearance,
                        FieldMount {
                            inner_anchor: inner_anchor.clone(),
                            press_probe: Rc::clone(&press_probe),
                        },
                        &semantics_actions,
                        &touch_handles,
                    )
                }),
            ));

        // Shared with `wrap_double_tap_word_select` below: a double-tap that
        // lands on this same contact must be able to silence the drag this
        // anchor otherwise starts for it — see that method's doc.
        let drag_anchor = Rc::clone(&self.selection_drag);
        let field = if gestures {
            self.install_pointer_handlers(field, view, Rc::clone(&drag_anchor))
        } else {
            field
        };
        self.wrap_double_tap_word_select(field, view, gestures, drag_anchor)
    }

    fn dispose(&mut self) {
        self.accepts_semantics_actions.set(false);
        // Owner code runs below (the controller's listeners, a detach that
        // reaches the presentation): every later step still runs, and the
        // first failure is resumed once dispose is complete.
        let mut calls = OwnerCalls::new();
        // No edit from here on reaches the owner: the field is going away
        // (ADR-0142 item 4).
        self.reports_edits.set(false);
        // The touch handles leave the overlay, and the long press its arena.
        self.long_press.dispose();
        let handles = Rc::clone(&self.touch_handles);
        calls.run(|| {
            let mut failure = FirstPanic::new();
            handles.retire(&mut failure);
            failure.resume();
        });
        calls.run(|| self.touch_handles.menu().close());
        // This field's own rebuild listener goes first: the commit below
        // must not schedule a build of a subtree that is being unmounted.
        if let Some(subscription) = self.controller_listener_id.take() {
            calls.run(|| drop(subscription));
        }
        // A field unmounted while it holds the input (a route the program
        // changed) keeps the composition it shows as committed text: the
        // composing range ends in place and the controller's listeners hear
        // of it, before the store detaches below and drops whatever the input
        // method still had queued. A field that blurred first was committed
        // then, and holds no client now. Read in a statement of its own: the
        // commit runs owner code.
        let holds_input = self.ime_token.borrow().is_some();
        if holds_input {
            let controller = self.controller.borrow().clone();
            calls.run(|| controller.end_composition());
        }
        // A selection that is gone is no longer offered to other clients.
        calls.run(|| self.primary.release());
        let attachment = self.focus_attachment.borrow_mut().take();
        self.selection_drag.set(None);
        let owns_attachment = attachment
            .as_ref()
            .is_some_and(|attachment| attachment.is_attached());
        if owns_attachment {
            self.rect_provider_registration.take();
            self.key_handler_registration.take();
            self.action_chain_registration.take();
        } else {
            if let Some(registration) = self.rect_provider_registration.take() {
                registration.relinquish();
            }
            if let Some(registration) = self.key_handler_registration.take() {
                registration.relinquish();
            }
            if let Some(registration) = self.action_chain_registration.take() {
                registration.relinquish();
            }
        }
        self.action_chain = None;
        self.enclosing_action_chain = None;
        // Withdraw the focus-change listener registered in init_state: the
        // owned subscription detaches on its drop.
        drop(self.focus_listening.take());
        // Withdraw the IME focus-change listener registered in init_state.
        drop(self.ime_focus_listening.take());
        self.ime_focus_transition = None;

        // Detach the IME client if this field still has one attached — the
        // ADR-0030 detach-on-dispose contract. A field unmounted while
        // focused is not guaranteed a focus-loss notification when its
        // attachment is detached below, so this is the one path that unconditionally
        // closes the IME session on unmount. Harmless no-op if the field
        // already blurred (and so already detached) before unmounting.
        let token = self.ime_token.borrow_mut().take();
        if let Some(token) = token
            && let Some(handle) = &self.ime_handle
            && let Some(Err(error)) = calls.run(|| handle.detach(token))
        {
            calls.run(|| {
                tracing::trace!(
                    ?error,
                    "IME dispose detach reached a presentation that was already closing"
                );
            });
        }
        // A platform that still holds the store (or a grant queued in it)
        // must not reach the controller of a field that is gone.
        if let Some(store) = self.text_store.take() {
            let _ = calls.run(|| store.detach());
            calls.retire(store);
        }

        // Stop the IME cursor-area loop (ADR-0030) if one is running — the
        // same unconditional-on-unmount contract as the IME token detach
        // just above, and independent of it: a field unmounted while
        // focused is not guaranteed a blur notification, so this is the one
        // path that always flips the current attach's alive flag false,
        // whether or not the field ever blurred first.
        let alive = self.cursor_area_alive.borrow_mut().take();
        if let Some(alive) = alive {
            alive.set(false);
        }

        // Detach through the generation-checked lifecycle authority.
        if let Some(attachment) = attachment {
            let _ = calls.run(|| attachment.detach());
        }
        self.parent = None;
        calls.resume();

        // Deliberately NOT disposed here: `self.rebuild_notifier` is also
        // held by the `AnimatedBuilder` this state's own `build()` output
        // wraps around (`Arc::new(self.rebuild_notifier.clone())`), and that
        // child element's own `on_unmount` calls `remove_listener` on its
        // clone as part of the SAME unmount sweep. `ViewState::dispose`
        // (this method) runs before that child unmounts.
        //
        // `ChangeNotifier::remove_listener` is a safe no-op after dispose
        // (see its doc), so
        // calling `dispose()` here first would no longer panic against the
        // child's later `remove_listener` call. It is still left undone: an
        // explicit `dispose()` would mark the shared notifier disposed while
        // the child's subscription is still live, which is unnecessary
        // churn for no behavioral gain here — letting the notifier's `Arc`
        // refcount reach zero naturally (once every clone — this state's
        // and the child element's — is gone) is sufficient, since nothing
        // reads its disposed-flag on this path.
    }
}

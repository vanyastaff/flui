//! Downstream acceptance payload for the exact shared application tree.
//! No model/controller mutation or forced root rebuild to hide missing wakes.
//! Root replacement is used only for the explicit unmount scenario.
//! Native input, pixels and eager builder counts remain separate gates.

#[cfg(test)]
mod notes_flow {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;
    use std::{
        cell::RefCell,
        future::Future,
        pin::Pin,
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    use flui::foundation::RenderId;
    use flui::interaction::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
    use flui::testing::a11y::{A11yTree, Action, ActionRequest, Role, TreeId};
    use flui::testing::widgets::{LaidOut, lay_out, offset, settle_lazy, tight};

    const WIDTH: f64 = 640.0;
    const HEIGHT: f64 = 720.0;

    fn mount() -> LaidOut {
        lay_out(
            super::super::tree::NotesApp::default(),
            tight(WIDTH, HEIGHT),
        )
    }

    fn frames(laid: &mut LaidOut) {
        // Drive notifications, including lazy residency, without marking the
        // root dirty. A missing production wake must leave stale output.
        // PageRoute has a 300ms entrance even for its jump-cut presentation.
        // Before completion both old and new overlay entries are legitimately
        // onstage. Advance the virtual clock past it, rather than pretending
        // the covered page is inactive or forcing the root to rebuild.
        for _ in 0..8 {
            laid.pump_for(Duration::from_millis(50));
        }
        settle_lazy(laid);
    }

    fn published_tree(laid: &mut LaidOut) -> impl Fn() -> A11yTree + use<> {
        laid.enable_semantics();
        frames(laid);
        let delivered = Arc::new(Mutex::new(
            laid.a11y_tree().expect("semantics enabled").raw().clone(),
        ));
        let sink = Arc::clone(&delivered);
        // The headless platform bridge discards packets. Observe the real
        // owner's flush callback through its public composition-root API.
        laid.pipeline_owner().with_mut(|owner| {
            owner.set_semantics_update_callback(Arc::new(move |update| {
                let ids: std::collections::HashSet<_> =
                    update.nodes.iter().map(|(id, _)| *id).collect();
                assert_eq!(
                    ids.len(),
                    update.nodes.len(),
                    "published packet repeats an identity"
                );
                let mut current = sink.lock().expect("publication mirror lock");
                if let Some(tree) = &update.tree {
                    current.tree = Some(tree.clone());
                }
                for (id, node) in &update.nodes {
                    if let Some((_, previous)) = current.nodes.iter_mut().find(|(key, _)| key == id)
                    {
                        *previous = node.clone();
                    } else {
                        current.nodes.push((*id, node.clone()));
                    }
                }
                current.focus = update.focus;
                current.tree_id = update.tree_id;
            }));
        });
        // Query only nodes connected by the delivered parent's current child
        // links. Historical disconnected payloads cannot count as visible.
        move || A11yTree::new(delivered.lock().expect("publication mirror lock").clone())
    }

    fn settings_semantics(laid: &LaidOut, published: &impl Fn() -> A11yTree, compact: bool) {
        for (source, tree) in [
            ("assembled", laid.a11y_tree().expect("semantics enabled")),
            ("delivered", published()),
        ] {
            assert_eq!(
                tree.nodes()
                    .filter(|node| node.label() == Some("FLUI Notes"))
                    .count(),
                1,
                "{source}: only the active Notes header is reachable: {}",
                tree.describe()
            );
            let back = tree.find_by_label("Back").expect("one active Back action");
            assert!(back.supports_action(Action::Click));
            assert!(
                tree.find_by_label(&format!("Compact rows: {compact}"))
                    .is_ok()
            );
            assert!(
                tree.find_all(Role::Form).is_empty(),
                "{source}: covered Note form absent"
            );
            assert!(
                tree.find_all(Role::TextInput).is_empty(),
                "{source}: covered Edit absent"
            );
            assert!(
                tree.find_all_by_label("Save note").is_empty(),
                "{source}: covered Save absent"
            );
        }
    }

    fn queued_click(laid: &LaidOut, published: &impl Fn() -> A11yTree, label: &str) {
        let tree = published();
        let node = tree
            .find_by_label(label)
            .expect("one published action target");
        assert!(
            node.supports_action(Action::Click),
            "{label:?} supports Click"
        );
        click_node(laid, node.id());
    }

    fn click_node(laid: &LaidOut, target: flui::testing::a11y::NodeId) {
        // Targets one published node by identity, for when two onstage pages
        // publish the same label during a transition.
        let listener = laid
            .accessibility_action_listener()
            .expect("real platform action listener");
        listener(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        });
    }

    fn tick_until(
        laid: &mut LaidOut,
        published: &impl Fn() -> A11yTree,
        ready: impl Fn(&A11yTree) -> bool,
        what: &str,
    ) {
        // Zero-time frames: the 300ms transition stays underway.
        for _ in 0..8 {
            laid.tick();
            if ready(&published()) {
                return;
            }
        }
        panic!("{what} was not published during zero-time transition frames");
    }

    fn entrance_label(laid: &mut LaidOut, published: &impl Fn() -> A11yTree, label: &str) {
        // Drain actual queued platform work at the earliest real frames,
        // without advancing the 300ms entrance or forcing a root rebuild.
        for _ in 0..8 {
            laid.tick();
            let assembled = laid.a11y_tree().expect("semantics enabled");
            if assembled.find_by_label(label).is_ok() && published().find_by_label(label).is_ok() {
                return;
            }
        }
        panic!("queued {label:?} did not publish during zero-time entrance frames");
    }

    fn onstage(laid: &LaidOut, id: RenderId) -> bool {
        // Retained overlay pages may keep their old coordinates. Diagnostics
        // are a query aid here, never the behavior asserted by a scenario.
        laid.pipeline_owner().with(|owner| {
            let tree = owner.render_tree();
            let mut child = id;
            while let Some(parent) = tree.parent(child) {
                if let Some(node) = owner.debug_node_diagnostics(parent) {
                    if node.name() == Some("RenderOffstage")
                        && node.get_property("offstage").is_some()
                    {
                        return false;
                    }
                    if node.name() == Some("RenderTheater") {
                        let skip = node
                            .get_property("skip_count")
                            .expect("theater diagnostic skip count")
                            .parse::<usize>()
                            .expect("numeric skip count");
                        let slot = tree
                            .get(parent)
                            .expect("live parent")
                            .children()
                            .iter()
                            .position(|candidate| *candidate == child)
                            .expect("child has its reported parent");
                        if slot < skip {
                            return false;
                        }
                    }
                }
                child = parent;
            }
            true
        })
    }

    fn active_text(laid: &LaidOut, text: &str) -> Vec<RenderId> {
        laid.find_all_text(text)
            .into_iter()
            .filter(|id| onstage(laid, *id))
            .collect()
    }

    fn rendered_text(laid: &LaidOut, text: &str) -> RenderId {
        assert!(
            painted_text(laid, text),
            "{text:?} must be in the submitted scene"
        );
        let matches = active_text(laid, text);
        assert_eq!(matches.len(), 1, "expected one active {text:?}");
        let id = matches[0];
        let size = laid.size(id);
        let position = laid.absolute_offset(id);
        assert!(size.width > 0.0 && size.height > 0.0, "{text:?} laid out");
        assert!(position.dx >= 0.0 && position.dy >= 0.0);
        assert!(
            position.dx < WIDTH && position.dy < HEIGHT,
            "{text:?} in window"
        );
        id
    }

    fn painted_text(laid: &LaidOut, text: &str) -> bool {
        let tree = laid.layer_tree().expect("a committed scene");
        let commands: Vec<flui::testing::rendering::DrawCommandSummary> =
            flui::testing::rendering::collect_commands(tree);
        commands.iter().any(|command| {
            command.kind == flui::testing::rendering::DrawKind::Text
                && command.line.contains(&format!(" {text:?} "))
        })
    }

    fn tap_text(laid: &mut LaidOut, text: &str) {
        let id = rendered_text(laid, text);
        let position = laid.absolute_offset(id);
        let size = laid.size(id);
        let x = position.dx + size.width / 2.0;
        let y = position.dy + size.height / 2.0;
        let path = laid.hit_test_pointer(offset(x, y));
        let button_hit = laid.pipeline_owner().with(|owner| {
            let mut ancestor = Some(id);
            while let Some(current) = ancestor {
                if path
                    .path()
                    .iter()
                    .any(|entry| entry.target == current && entry.pointer_target.is_some())
                {
                    return true;
                }
                ancestor = owner.render_tree().parent(current);
            }
            false
        });
        assert!(
            button_hit,
            "the actual {text:?} control must receive this pointer"
        );
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
        frames(laid);
    }

    fn ready() -> LaidOut {
        let mut laid = mount();
        frames(&mut laid);
        rendered_text(&laid, "Load failed");
        tap_text(&mut laid, "Retry");
        rendered_text(&laid, "Note 0");
        assert!(active_text(&laid, "Load failed").is_empty());
        laid
    }

    fn dispatch(laid: &LaidOut, key: Key, modifiers: Modifiers) {
        let description = format!("{key:?} with {modifiers:?}");
        assert!(
            laid.focus_manager().dispatch_key_event(&KeyEvent {
                state: KeyState::Down,
                key,
                modifiers,
                ..KeyEvent::default()
            }),
            "focused input must consume {description}"
        );
    }

    fn field(laid: &LaidOut) -> RenderId {
        let fields: Vec<_> = laid
            .find_all_by_render_type("RenderEditable")
            .into_iter()
            .filter(|id| onstage(laid, *id))
            .collect();
        assert_eq!(fields.len(), 1, "one onstage editable field");
        fields[0]
    }

    fn field_text(laid: &LaidOut) -> String {
        let field = field(laid);
        laid.render_property(field, "text")
            .expect("editable text diagnostics")
    }

    fn focus_field(laid: &mut LaidOut) {
        let id = field(laid);
        let position = laid.absolute_offset(id);
        let size = laid.size(id);
        assert!(size.width > 0.0 && size.height > 0.0);
        laid.dispatch_pointer_down(position.dx + 2.0, position.dy + size.height / 2.0);
        laid.dispatch_pointer_up(position.dx + 2.0, position.dy + size.height / 2.0);
        frames(laid);
        assert!(laid.focus_manager().primary_focus().is_some());
    }

    fn replace_by_keyboard(laid: &mut LaidOut, text: &str) {
        focus_field(laid);
        let command = if cfg!(any(target_os = "macos", target_os = "ios")) {
            Modifiers::META
        } else {
            Modifiers::CONTROL
        };
        dispatch(laid, Key::Character("a".to_owned()), command);
        dispatch(laid, Key::Named(NamedKey::Backspace), Modifiers::empty());
        for ch in text.chars() {
            dispatch(laid, Key::Character(ch.to_string()), Modifiers::empty());
        }
        frames(laid);
        assert_eq!(
            field_text(laid),
            text,
            "rendered edit follows dispatched keys"
        );
        if !text.is_empty() {
            assert!(
                painted_text(laid, text),
                "keyboard edit reaches the painted field"
            );
        }
    }

    fn invalid_save_preserves_home_and_valid_keyboard_edit_updates_it() {
        let mut laid = ready();
        tap_text(&mut laid, "Note 0");
        rendered_text(&laid, "Editing note 0");
        replace_by_keyboard(&mut laid, "");
        tap_text(&mut laid, "Save note");
        rendered_text(&laid, "Enter a title");
        rendered_text(&laid, "Fix the title");
        tap_text(&mut laid, "Back");
        rendered_text(&laid, "Note 0");
        tap_text(&mut laid, "Note 0");
        replace_by_keyboard(&mut laid, "Community note");
        // The field's successor must be the usable Save action. Go backwards
        // and forwards once to prove traversal, then activate through Enter.
        dispatch(&laid, Key::Named(NamedKey::Tab), Modifiers::empty());
        dispatch(&laid, Key::Named(NamedKey::Tab), Modifiers::SHIFT);
        dispatch(&laid, Key::Named(NamedKey::Tab), Modifiers::empty());
        dispatch(&laid, Key::Named(NamedKey::Enter), Modifiers::empty());
        frames(&mut laid);
        rendered_text(&laid, "Saved note 0");
        assert!(active_text(&laid, "Enter a title").is_empty());
        tap_text(&mut laid, "Back");
        rendered_text(&laid, "Community note");
        assert!(active_text(&laid, "Note 0").is_empty());
        assert!(
            !painted_text(&laid, "Note 0"),
            "old title is absent from active paint"
        );
    }

    fn draft_survives_settings_and_density_changes_actual_row_geometry() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        let first = rendered_text(&laid, "Note 0");
        let second = rendered_text(&laid, "Note 1");
        assert_eq!(
            laid.absolute_offset(second).dy - laid.absolute_offset(first).dy,
            48.0
        );
        tap_text(&mut laid, "Note 0");
        replace_by_keyboard(&mut laid, "Unsaved community draft");
        for tree in [laid.a11y_tree().expect("semantics enabled"), published()] {
            let field = tree
                .find(Role::TextInput)
                .expect("lower Edit was published");
            assert_eq!(field.value(), Some("Unsaved community draft"));
            assert!(
                tree.find_by_label("Save note")
                    .expect("lower Save published")
                    .supports_action(Action::Click)
            );
        }
        let save = published()
            .find_by_label("Save note")
            .expect("published Save")
            .id();
        tap_text(&mut laid, "Settings");
        rendered_text(&laid, "Compact rows: false");
        settings_semantics(&laid, &published, false);
        assert!(
            laid.invoke_semantics_action(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: save,
                data: None,
            })
            .is_err(),
            "covered Save must lose action admission"
        );
        tap_text(&mut laid, "Toggle compact rows");
        rendered_text(&laid, "Compact rows: true");
        settings_semantics(&laid, &published, true);
        tap_text(&mut laid, "Back");
        assert_eq!(field_text(&laid), "Unsaved community draft");
        for tree in [laid.a11y_tree().expect("semantics enabled"), published()] {
            assert_eq!(
                tree.find(Role::TextInput).expect("restored Edit").value(),
                Some("Unsaved community draft")
            );
            assert!(
                tree.find_by_label("Save note")
                    .expect("restored Save")
                    .supports_action(Action::Click)
            );
        }
        let back = published()
            .find_by_label("Back")
            .expect("restored Back")
            .id();
        laid.invoke_semantics_action(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: back,
            data: None,
        })
        .expect("restored Note action remains usable");
        frames(&mut laid);
        let first = rendered_text(&laid, "Note 0");
        let second = rendered_text(&laid, "Note 1");
        assert_eq!(
            laid.absolute_offset(second).dy - laid.absolute_offset(first).dy,
            32.0
        );
        tap_text(&mut laid, "Note 0");
        assert_eq!(field_text(&laid), "Unsaved community draft");
    }

    fn queued_settings_toggle_during_entrance_settles_to_one_active_page() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        tap_text(&mut laid, "Note 0");
        replace_by_keyboard(&mut laid, "Entrance draft");
        assert_eq!(
            published()
                .find(Role::TextInput)
                .expect("Note published before push")
                .value(),
            Some("Entrance draft")
        );
        queued_click(&laid, &published, "Settings");
        entrance_label(&mut laid, &published, "Compact rows: false");
        // The lower Note is legitimately onstage while the incoming route
        // animates. This premise distinguishes the early-toggle sequence
        // from the settled navigation scenario above.
        assert_eq!(
            published()
                .find(Role::TextInput)
                .expect("Note still onstage during entrance")
                .value(),
            Some("Entrance draft")
        );
        queued_click(&laid, &published, "Toggle compact rows");
        entrance_label(&mut laid, &published, "Compact rows: true");
        frames(&mut laid);
        settings_semantics(&laid, &published, true);
        queued_click(&laid, &published, "Back");
        frames(&mut laid);
        for tree in [laid.a11y_tree().expect("semantics enabled"), published()] {
            assert_eq!(
                tree.find(Role::TextInput)
                    .expect("Note restored after queued Back")
                    .value(),
                Some("Entrance draft")
            );
            assert!(
                tree.find_by_label("Save note")
                    .expect("restored Save")
                    .supports_action(Action::Click)
            );
            assert!(
                tree.find_all_by_label("Compact rows: true").is_empty(),
                "Settings disconnected after Back"
            );
            assert_eq!(
                tree.nodes()
                    .filter(|node| node.label() == Some("FLUI Notes"))
                    .count(),
                1,
                "one restored Notes header"
            );
            assert!(
                tree.find_by_label("Back")
                    .expect("one restored Back")
                    .supports_action(Action::Click)
            );
        }
        assert_eq!(field_text(&laid), "Entrance draft");
    }

    fn settings_activated_again_during_entrance_stacks_one_page() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        queued_click(&laid, &published, "Settings");
        entrance_label(&mut laid, &published, "Compact rows: false");
        // Home is still onstage during the entrance and still publishes its
        // Settings action, so an assistive client can activate it again.
        queued_click(&laid, &published, "Settings");
        frames(&mut laid);
        tap_text(&mut laid, "Back");
        frames(&mut laid);
        assert!(
            active_text(&laid, "Compact rows: false").is_empty(),
            "one Back leaves the only Settings page"
        );
        rendered_text(&laid, "Note 0");
    }

    fn another_note_activated_during_entrance_is_ignored() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        queued_click(&laid, &published, "Note 0");
        // Wait for the first entrance frames: the editor's field is published
        // while Home is still onstage.
        let mut entering = false;
        for _ in 0..8 {
            laid.tick();
            if published().find(Role::TextInput).is_ok() {
                entering = true;
                break;
            }
        }
        assert!(entering, "the Note editor enters after a queued activation");
        // Home's rows stay published while the Note animates in.
        queued_click(&laid, &published, "Note 1");
        frames(&mut laid);
        rendered_text(&laid, "Editing note 0");
        tap_text(&mut laid, "Back");
        frames(&mut laid);
        assert!(
            active_text(&laid, "Editing note 0").is_empty()
                && active_text(&laid, "Editing note 1").is_empty(),
            "one Back returns from the only Note page"
        );
        rendered_text(&laid, "Note 0");
    }

    fn departing_settings_back_pops_only_its_own_page() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        tap_text(&mut laid, "Note 0");
        rendered_text(&laid, "Editing note 0");
        tap_text(&mut laid, "Settings");
        rendered_text(&laid, "Compact rows: false");
        let back = published()
            .find_by_label("Back")
            .expect("one settled Settings Back")
            .id();
        click_node(&laid, back);
        // The exit is underway once the Note beneath publishes its field
        // again while the departing Settings still publishes its Back.
        tick_until(
            &mut laid,
            &published,
            |tree| {
                tree.find(Role::TextInput).is_ok()
                    && tree.find_by_label("Compact rows: false").is_ok()
                    && tree.nodes().any(|node| node.id() == back)
            },
            "the Settings exit over the Note",
        );
        click_node(&laid, back);
        frames(&mut laid);
        rendered_text(&laid, "Editing note 0");
        assert!(
            active_text(&laid, "Compact rows: false").is_empty(),
            "Settings left"
        );
    }

    fn departing_settings_toggle_keeps_the_preference() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        tap_text(&mut laid, "Settings");
        rendered_text(&laid, "Compact rows: false");
        let toggle = published()
            .find_by_label("Toggle compact rows")
            .expect("published toggle")
            .id();
        queued_click(&laid, &published, "Back");
        // The exit is underway once Home's rows publish again while the
        // departing Settings still publishes its toggle.
        tick_until(
            &mut laid,
            &published,
            |tree| {
                tree.find_by_label("Note 1").is_ok() && tree.nodes().any(|node| node.id() == toggle)
            },
            "the Settings exit over Home",
        );
        click_node(&laid, toggle);
        frames(&mut laid);
        let first = rendered_text(&laid, "Note 0");
        let second = rendered_text(&laid, "Note 1");
        assert_eq!(
            laid.absolute_offset(second).dy - laid.absolute_offset(first).dy,
            48.0,
            "rows keep their regular density"
        );
    }

    fn departing_editor_save_does_not_write_into_home() {
        let mut laid = ready();
        let published = published_tree(&mut laid);
        tap_text(&mut laid, "Note 0");
        replace_by_keyboard(&mut laid, "Abandoned draft");
        let save = published()
            .find_by_label("Save note")
            .expect("published Save")
            .id();
        queued_click(&laid, &published, "Back");
        // The exit is underway once Home's rows publish again while the
        // departing editor still publishes its Save.
        tick_until(
            &mut laid,
            &published,
            |tree| {
                tree.find_by_label("Note 1").is_ok() && tree.nodes().any(|node| node.id() == save)
            },
            "the Note exit over Home",
        );
        click_node(&laid, save);
        frames(&mut laid);
        rendered_text(&laid, "Note 0");
        assert!(
            active_text(&laid, "Saved note 0").is_empty(),
            "the departing editor did not save"
        );
        assert!(
            !painted_text(&laid, "Abandoned draft"),
            "the abandoned draft is not a Home title"
        );
    }

    fn back_is_offered_only_where_it_leaves_a_page() {
        let mut laid = ready();
        assert!(
            active_text(&laid, "Back").is_empty(),
            "Home is the root page; Back is not offered there"
        );
        tap_text(&mut laid, "Settings");
        tap_text(&mut laid, "Back");
        rendered_text(&laid, "Note 0");
        assert!(
            active_text(&laid, "Back").is_empty(),
            "Back is gone again on Home"
        );
    }

    fn settings_pressed_twice_leaves_with_one_back() {
        let mut laid = ready();
        tap_text(&mut laid, "Settings");
        rendered_text(&laid, "Compact rows: false");
        // Press Settings again wherever the Settings page still offers it.
        let again = active_text(&laid, "Settings");
        if !again.is_empty() {
            tap_text(&mut laid, "Settings");
        }
        tap_text(&mut laid, "Back");
        assert!(
            active_text(&laid, "Compact rows: false").is_empty(),
            "one Back leaves Settings"
        );
        rendered_text(&laid, "Note 0");
        assert!(again.is_empty(), "Settings is not offered on its own page");
    }

    fn pointer_drag_changes_lazy_band_and_route_roundtrip_preserves_position() {
        let mut laid = ready();
        assert!(
            laid.find_all_text("Note 100").is_empty(),
            "distant row not resident initially"
        );
        assert!(
            active_text(&laid, "Note 15").into_iter().all(|id| {
                let y = laid.absolute_offset(id).dy;
                y < 0.0 || y >= HEIGHT
            }),
            "Note 15 is outside the initial visible band"
        );
        let first = rendered_text(&laid, "Note 0");
        let start = laid.absolute_offset(first).dy + 420.0;
        assert!(start < HEIGHT);
        laid.dispatch_pointer_down(200.0, start);
        laid.dispatch_pointer_move_after(200.0, start - 20.0, Duration::from_millis(20));
        laid.dispatch_pointer_move_after(200.0, start - 300.0, Duration::from_millis(30));
        // This row pins drag and retained position, not ballistic progress.
        // Hold the final point long enough for the velocity sample window to
        // expire, so a subsequent page transition cannot keep changing offset.
        laid.dispatch_pointer_move_after(200.0, start - 300.0, Duration::from_millis(300));
        laid.dispatch_pointer_up(200.0, start - 300.0);
        frames(&mut laid);
        let row = rendered_text(&laid, "Note 15");
        let before = laid.absolute_offset(row).dy;
        tap_text(&mut laid, "Note 15");
        rendered_text(&laid, "Editing note 15");
        tap_text(&mut laid, "Back");
        let row = rendered_text(&laid, "Note 15");
        assert_eq!(
            laid.absolute_offset(row).dy,
            before,
            "nonzero offset survives navigation"
        );
        assert!(laid.find_all_text("Note 100").is_empty());
    }

    #[derive(Default)]
    struct Completion {
        state: Mutex<CompletionState>,
        retired: AtomicBool,
    }

    #[derive(Default)]
    struct CompletionState {
        result: Option<Result<(), &'static str>>,
        waker: Option<Waker>,
    }

    impl Completion {
        fn finish(&self, result: Result<(), &'static str>) {
            let wake = {
                let mut state = self.state.lock().expect("completion state");
                state.result = Some(result);
                state.waker.take()
            };
            if let Some(wake) = wake {
                wake.wake();
            }
        }
    }

    struct Readiness(Arc<Completion>);

    impl Future for Readiness {
        type Output = Result<(), &'static str>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let wake = cx.waker().clone();
            let (result, old_wake) = {
                let mut state = self.0.state.lock().expect("completion state");
                let result = state.result.take();
                let old_wake = state.waker.replace(wake);
                (result, old_wake)
            };
            drop(old_wake);
            if let Some(result) = result {
                Poll::Ready(result)
            } else {
                Poll::Pending
            }
        }
    }

    impl Drop for Readiness {
        fn drop(&mut self) {
            self.0.retired.store(true, Ordering::SeqCst);
        }
    }

    fn loading_retry_replacement_and_unmount_retire_old_service_work() {
        let requests = Rc::new(RefCell::new(Vec::<Arc<Completion>>::new()));
        let recorded = requests.clone();
        let loader = Rc::new(move |_attempt| {
            let completion = Arc::new(Completion::default());
            recorded.borrow_mut().push(completion.clone());
            Box::pin(Readiness(completion)) as flui::widgets::BoxedResultFuture<(), &'static str>
        });
        let app = super::super::tree::NotesApp::with_loader(loader);
        let mut laid = lay_out(app, tight(WIDTH, HEIGHT));
        frames(&mut laid);
        rendered_text(&laid, "Loading notes");
        let first = requests.borrow()[0].clone();
        first.finish(Err("held service failure"));
        frames(&mut laid);
        rendered_text(&laid, "Load failed");
        tap_text(&mut laid, "Retry");
        rendered_text(&laid, "Loading notes");
        let replaced = requests.borrow().last().expect("retry request").clone();
        tap_text(&mut laid, "Reload notes");
        assert!(
            replaced.retired.load(Ordering::SeqCst),
            "key replacement drops old accepted future"
        );
        let newest = requests.borrow().last().expect("reload request").clone();
        assert!(!Arc::ptr_eq(&newest, &replaced));
        replaced.finish(Err("stale error"));
        frames(&mut laid);
        rendered_text(&laid, "Loading notes");
        assert!(!painted_text(&laid, "Load failed"));
        newest.finish(Ok(()));
        frames(&mut laid);
        rendered_text(&laid, "Note 0");
        tap_text(&mut laid, "Reload notes");
        rendered_text(&laid, "Loading notes");
        let pending = requests.borrow().last().expect("pending reload").clone();
        laid.pump_widget(flui::widgets::Text::new("Closed notes"));
        frames(&mut laid);
        assert!(
            pending.retired.load(Ordering::SeqCst),
            "unmount retires accepted pending future"
        );
        pending.finish(Ok(()));
        frames(&mut laid);
        rendered_text(&laid, "Closed notes");
        assert!(!painted_text(&laid, "Loading notes"));
        assert!(!painted_text(&laid, "Note 0"));
    }

    #[test]
    fn notes_public_input_flow_matrix() {
        let cases: [(&str, fn()); 12] = [
            (
                "departing_settings_toggle_keeps_the_preference",
                departing_settings_toggle_keeps_the_preference,
            ),
            (
                "departing_settings_back_pops_only_its_own_page",
                departing_settings_back_pops_only_its_own_page,
            ),
            (
                "departing_editor_save_does_not_write_into_home",
                departing_editor_save_does_not_write_into_home,
            ),
            (
                "loading_retry_replacement_and_unmount_retire_old_service_work",
                loading_retry_replacement_and_unmount_retire_old_service_work,
            ),
            (
                "invalid_save_preserves_home_and_valid_keyboard_edit_updates_it",
                invalid_save_preserves_home_and_valid_keyboard_edit_updates_it,
            ),
            (
                "draft_survives_settings_and_density_changes_actual_row_geometry",
                draft_survives_settings_and_density_changes_actual_row_geometry,
            ),
            (
                "queued_settings_toggle_during_entrance_settles_to_one_active_page",
                queued_settings_toggle_during_entrance_settles_to_one_active_page,
            ),
            (
                "settings_pressed_twice_leaves_with_one_back",
                settings_pressed_twice_leaves_with_one_back,
            ),
            (
                "back_is_offered_only_where_it_leaves_a_page",
                back_is_offered_only_where_it_leaves_a_page,
            ),
            (
                "another_note_activated_during_entrance_is_ignored",
                another_note_activated_during_entrance_is_ignored,
            ),
            (
                "settings_activated_again_during_entrance_stacks_one_page",
                settings_activated_again_during_entrance_stacks_one_page,
            ),
            (
                "pointer_drag_changes_lazy_band_and_route_roundtrip_preserves_position",
                pointer_drag_changes_lazy_band_and_route_roundtrip_preserves_position,
            ),
        ];
        let mut failures = Vec::new();
        for (name, case) in cases {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic");
                failures.push(format!("{name}: {message}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

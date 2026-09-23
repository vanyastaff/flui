use super::*;

#[derive(Clone)]
struct DependencyPanicView {
    hook_calls: Arc<AtomicUsize>,
}

struct DependencyPanicState {
    hook_calls: Arc<AtomicUsize>,
}

impl crate::StatefulView for DependencyPanicView {
    type State = DependencyPanicState;

    fn create_state(&self) -> Self::State {
        DependencyPanicState {
            hook_calls: Arc::clone(&self.hook_calls),
        }
    }
}

impl crate::ViewState<DependencyPanicView> for DependencyPanicState {
    fn build(
        &self,
        _view: &DependencyPanicView,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        TestView
    }

    fn did_change_dependencies(&mut self, _ctx: &dyn crate::LifecycleContext) {
        self.hook_calls.fetch_add(1, Ordering::Relaxed);
        panic!("dependency hook panic");
    }
}

impl View for DependencyPanicView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
}

#[derive(Clone)]
struct InitPanicView {
    failed_id: Arc<parking_lot::Mutex<Option<ElementId>>>,
}

struct InitPanicState {
    failed_id: Arc<parking_lot::Mutex<Option<ElementId>>>,
}

impl crate::StatefulView for InitPanicView {
    type State = InitPanicState;

    fn create_state(&self) -> Self::State {
        InitPanicState {
            failed_id: Arc::clone(&self.failed_id),
        }
    }
}

impl crate::ViewState<InitPanicView> for InitPanicState {
    fn init_state(&mut self, ctx: &dyn crate::LifecycleContext) {
        *self.failed_id.lock() = Some(ctx.element_id());
        panic!("scoped descendant init_state panic");
    }

    fn build(&self, _view: &InitPanicView, _ctx: &dyn crate::BuildContext) -> impl crate::IntoView {
        TestView
    }
}

impl View for InitPanicView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
}

#[test]
fn scoped_child_dependency_panic_is_replaced_without_consuming_foreign_work() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
    settle_initial_builds(&mut tree, &mut owner);
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let scope = tree.insert(
        &ReparentingLayoutHost {
            children: vec![
                DependencyPanicView {
                    hook_calls: Arc::clone(&hook_calls),
                }
                .boxed(),
            ],
        },
        root,
        0,
        &mut owner.element_owner_mut(),
    );
    tree.mark_needs_build(scope);
    owner.schedule_build_for(scope, 1, RebuildReason::InitialMount);
    settle_initial_builds(&mut tree, &mut owner);
    let panicking = tree.get(scope).expect("scope stays live").child_ids()[0];
    let foreign = insert_child(&mut tree, &mut owner, root, 1);
    settle_initial_builds(&mut tree, &mut owner);
    let _cell = owner.register_layout_builder_for_test(RenderId::new(71), scope);

    tree.mark_needs_build(panicking);
    owner.schedule_build_for(panicking, 2, RebuildReason::DependencyChange);
    owner.pending_dependency_changes.insert(panicking);
    tree.mark_needs_build(foreign);
    owner.schedule_build_for(foreign, 1, RebuildReason::StateChange);

    let foreign_reasons = owner
        .pending_rebuild_reasons(foreign)
        .expect("foreign work is queued");
    let live_scopes = owner.live_layout_scope_ids(&tree);
    owner.repartition_dirty(&tree, &live_scopes);
    owner.activate_build_target(BuildScopeTarget::LayoutBuilder(scope));
    BuildOwner::reset_layout_scope_classifications();
    let result = owner.drain_prepared_build_target(
        &mut tree,
        BuildScopeTarget::LayoutBuilder(scope),
        Some(&live_scopes),
    );

    assert_eq!(hook_calls.load(Ordering::Relaxed), 1);
    assert!(result.any_rebuilt);
    assert!(!result.target_rebuilt);
    assert_eq!(BuildOwner::layout_scope_classifications(), 4);
    let replacement = tree.get(scope).expect("scope stays live").child_ids()[0];
    assert_ne!(replacement, panicking);
    assert_eq!(
        tree.get(replacement)
            .expect("replacement stays live")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>(),
    );
    assert!(tree.get(panicking).is_none());
    assert_eq!(owner.pending_rebuild_reasons(panicking), None);
    assert!(!owner.pending_dependency_changes.contains(&panicking));
    assert_eq!(owner.pending_rebuild_reasons(replacement), None);
    assert_eq!(
        owner.pending_rebuild_reasons(foreign),
        Some(foreign_reasons)
    );
    assert_eq!(owner.dirty_count(), 1, "only foreign work remains queued");
    let recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].hook, LifecycleHook::DidChangeDependencies);
    assert!(matches!(
        recovered[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
        } if element == panicking
    ));
    #[cfg(debug_assertions)]
    {
        assert!(!owner.is_building());
        assert_eq!(owner.scope_depth(), 0);
    }
}

#[cfg(debug_assertions)]
#[test]
fn scoped_unbounded_duplicate_global_key_panic_restores_partitioned_work_and_flags() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
    settle_initial_builds(&mut tree, &mut owner);
    let scope = tree.insert(
        &ReparentingLayoutHost { children: vec![] },
        root,
        0,
        &mut owner.element_owner_mut(),
    );
    let panicking = tree.insert(
        &ReparentingLayoutHost { children: vec![] },
        scope,
        0,
        &mut owner.element_owner_mut(),
    );
    let current = insert_child(&mut tree, &mut owner, scope, 1);
    let foreign = insert_child(&mut tree, &mut owner, root, 1);
    settle_initial_builds(&mut tree, &mut owner);
    let _cell = owner.register_layout_builder_for_test(RenderId::new(72), scope);
    let duplicate_key = crate::GlobalKey::<()>::new();
    tree.update(
        panicking,
        &ReparentingLayoutHost {
            children: vec![
                KeyedView {
                    key: duplicate_key.clone(),
                }
                .boxed(),
                KeyedView { key: duplicate_key }.boxed(),
            ],
        },
        &mut owner.element_owner_mut(),
    );
    for (element, depth, reason) in [
        (panicking, 2, RebuildReason::ParentUpdate),
        (current, 2, RebuildReason::DependencyChange),
        (foreign, 1, RebuildReason::StateChange),
    ] {
        tree.mark_needs_build(element);
        owner.schedule_build_for(element, depth, reason);
    }
    let current_reasons = owner
        .pending_rebuild_reasons(current)
        .expect("current-scope work is queued");
    let panicking_reasons = owner
        .pending_rebuild_reasons(panicking)
        .expect("the framework-panicking element is queued");
    let foreign_reasons = owner
        .pending_rebuild_reasons(foreign)
        .expect("foreign work is queued");

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(scope));
    }))
    .expect_err("duplicate GlobalKey framework panic remains unbounded");
    assert!(
        payload_text(&*panic)
            .is_some_and(|message| message.contains("duplicate GlobalKey children"))
    );
    assert_eq!(
        owner.pending_rebuild_reasons(panicking),
        Some(panicking_reasons)
    );
    assert_eq!(
        owner.pending_rebuild_reasons(current),
        Some(current_reasons)
    );
    assert_eq!(
        owner.pending_rebuild_reasons(foreign),
        Some(foreign_reasons)
    );
    assert!(owner.dirty_elements.is_empty());
    let queues = owner
        .build_scope_queues
        .as_ref()
        .expect("the unwind repartitions every retained entry");
    assert_eq!(queues.root.len(), 1);
    assert_eq!(queues.isolated.get(&scope).map(BinaryHeap::len), Some(2));
    assert_eq!(owner.dirty_count(), 3);
    assert!(!owner.is_building());
    assert_eq!(owner.scope_depth(), 0);
}

#[test]
fn production_layout_builder_contains_a_stateful_descendant_init_panic() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let failed_id = Arc::new(parking_lot::Mutex::new(None));
    let child = InitPanicView {
        failed_id: Arc::clone(&failed_id),
    };
    let host = ReparentingLayoutHost {
        children: vec![LayoutBuilder::new(move |_ctx, _constraints| child.clone()).boxed()],
    };
    let root = tree.mount_root_with_pipeline_owner(
        &host,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    tree.mark_needs_build(root);
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(owner.layout_builder_count(), 1);
    let (scope, cell) = owner.layout_builder_registry.with_entries(|entries| {
        let entry = entries.values().next().expect("live LayoutBuilder entry");
        (entry.element, Arc::clone(&entry.cell))
    });
    cell.as_any()
        .downcast_ref::<flui_objects::LayoutConstraintsCell>()
        .expect("LayoutBuilder owns a constraints cell")
        .publish(flui_rendering::constraints::BoxConstraints::tight_for(
            Some(flui_types::geometry::px(100.0)),
            Some(flui_types::geometry::px(100.0)),
        ));

    assert!(owner.service_layout_builders(&mut tree, &pipeline));

    let failed = failed_id.lock().expect("init_state records the failed id");
    let replacement = tree.get(scope).expect("scope root stays live").child_ids()[0];
    assert!(tree.get(failed).is_none());
    assert_eq!(
        tree.get(replacement)
            .expect("replacement stays live")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>(),
    );
    let recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].hook, LifecycleHook::InitState);
    assert!(matches!(
        recovered[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
        } if element == failed
    ));
    assert_eq!(owner.layout_builder_count(), 1);
    assert!(tree.contains(scope));
    assert_eq!(owner.pending_rebuild_reasons(replacement), None);
    assert_eq!(owner.dirty_count(), 0);
    assert!(owner.take_recovered_panics().is_empty());
    assert!(!owner.service_layout_builders(&mut tree, &pipeline));
    assert_eq!(
        tree.get(scope).expect("scope root stays live").child_ids(),
        &[replacement]
    );
}

#[test]
fn root_init_state_unwind_leaves_no_hook_recording_marker() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let root = tree.mount_root(
        &InitPanicView {
            failed_id: Arc::new(parking_lot::Mutex::new(None)),
        },
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.build_scope(&mut tree);
    }));
    assert!(outcome.is_err());
    assert!(owner.lifecycle_panic_handoff.take().is_disarmed());
    assert!(owner.take_recovered_panics().is_empty());
}

// Included in FutureBuilder's test module to reuse its production-tree harness.

fn completion_counting_factory(
    completer: &Completer,
    completed: Arc<AtomicUsize>,
) -> FutureFactory<Payload, Boom> {
    let factory = completer.factory();
    Rc::new(move || {
        let future = factory();
        let completed = Arc::clone(&completed);
        Box::pin(async move {
            let result = future.await;
            completed.fetch_add(1, Ordering::Relaxed);
            result
        })
    })
}

#[test]
fn future_builder_remount_cancels_old_producer_before_its_completion_effects() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let old = Completer::new();
    let old_completed = Arc::new(AtomicUsize::new(0));
    let first = FutureBuilder::keyed(
        Some(1_u32),
        completion_counting_factory(&old, Arc::clone(&old_completed)),
        recording_builder(Arc::clone(&log)),
    );
    let mut harness = Harness::mount(&first);
    harness
        .tree
        .remove(harness.root, &mut harness.owner.element_owner_mut());
    assert_eq!(harness.scheduler.pending_task_count(), 0);

    // A removed route recreated in the same presentation may use the same key.
    // Neither a new graph nor a different request key supplies isolation here.
    let current = Completer::new();
    let replacement = FutureBuilder::keyed(
        Some(1_u32),
        current.factory(),
        recording_builder(Arc::clone(&log)),
    );
    harness.root = harness
        .tree
        .mount_root(&replacement, &mut harness.owner.element_owner_mut());
    harness.owner.schedule_build_for(
        harness.root,
        0,
        crate::RebuildReason::InitialMount,
    );
    harness.owner.build_scope(&mut harness.tree);
    assert_eq!(last(&log).state, ConnectionState::Waiting);
    let builds = seen(&log).len();

    old.complete(Ok(Payload(111)));
    harness.frame();
    assert_eq!(old_completed.load(Ordering::Relaxed), 0);
    assert_eq!(seen(&log).len(), builds);
    assert_eq!(harness.scheduler.pending_task_count(), 1);

    current.complete(Ok(Payload(222)));
    harness.frame();
    assert_eq!(last(&log).data, Some(222));
    assert_eq!(last(&log).state, ConnectionState::Done);
    assert_eq!(harness.scheduler.pending_task_count(), 0);
}

#[test]
fn future_builder_replaced_key_cannot_overwrite_newer_completed_subscription() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let old = Completer::new();
    let old_completed = Arc::new(AtomicUsize::new(0));
    let first = FutureBuilder::keyed(
        Some(1_u32),
        completion_counting_factory(&old, Arc::clone(&old_completed)),
        recording_builder(Arc::clone(&log)),
    );
    let mut harness = Harness::mount(&first);
    let current = Completer::new();
    let replacement = FutureBuilder::keyed(
        Some(2_u32),
        current.factory(),
        recording_builder(Arc::clone(&log)),
    );
    harness.update(&replacement);
    current.complete(Ok(Payload(222)));
    harness.frame();
    assert_eq!(last(&log).data, Some(222));
    let builds = seen(&log).len();

    // This is FutureBuilder's explicit key-replacement policy, not a universal
    // latest-response rule imposed on independent application tasks.
    old.complete(Ok(Payload(111)));
    harness.frame();
    assert_eq!(old_completed.load(Ordering::Relaxed), 0);
    assert_eq!(seen(&log).len(), builds);
    assert_eq!(last(&log).data, Some(222));
    assert_eq!(last(&log).state, ConnectionState::Done);
    assert_eq!(harness.scheduler.pending_task_count(), 0);
}

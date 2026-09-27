use super::*;

/// Oracle: a LAYER-TREE COMPARE (`format!("{:?}", ...)` on the
/// produced `LayerTree`), never a rebuild/flush count — B's own
/// render output, not merely whether B ran, must be byte-for-byte
/// unaffected by A closing.
#[test]
fn closing_presentation_a_leaves_sibling_layer_tree_identical() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    // Mount independent content on both presentations.
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("A mounts");
    realm
        .enter(|realm| {
            realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .widgets()
                .attach_root_widget(&flui_widgets::SizedBox::new(30.0, 30.0))
        })
        .expect("B mounts");

    let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
    let b_layer_tree_before = realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B installed");
        match UiRealm::draw_frame_for_presentation(b, constraints) {
            Ok(FramePaintOutcome::Painted(scene)) => format!("{:?}", scene.tree()),
            Ok(FramePaintOutcome::Idle) => panic!("B's first frame must paint, got Idle"),
            Ok(FramePaintOutcome::Errored) => {
                panic!("draw_frame_for_presentation never returns Ok(Errored)")
            }
            Err(error) => panic!("B's first frame failed: {error}"),
        }
    });

    assert!(
        realm.close_presentation_entered(a_id),
        "A must have been installed and removable"
    );

    let b_layer_tree_after = realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B still installed");
        // The first draw already settled B (nothing left dirty), so
        // an unconditional second draw would correctly report Idle
        // regardless of A -- that would prove nothing about B's
        // CONTENT. Force B's root render node dirty directly so this
        // second draw genuinely re-produces its layer tree from
        // scratch, the same content as the first draw, to compare
        // against it (a rebuild alone is not enough: reconciling an
        // unchanged `SizedBox` config is legitimately a no-op that
        // marks nothing dirty).
        b.pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
        match UiRealm::draw_frame_for_presentation(b, constraints) {
            Ok(FramePaintOutcome::Painted(scene)) => format!("{:?}", scene.tree()),
            Ok(FramePaintOutcome::Idle) => {
                panic!("B's post-A-close frame must still paint, got Idle")
            }
            Ok(FramePaintOutcome::Errored) => {
                panic!("draw_frame_for_presentation never returns Ok(Errored)")
            }
            Err(error) => panic!("B's post-A-close frame failed: {error}"),
        }
    });

    assert_eq!(
        b_layer_tree_before, b_layer_tree_after,
        "B's own layer tree must be byte-for-byte identical before and \
         after A closes -- nothing about B's content changed, so \
         nothing about its render output may either"
    );
}

/// A `RenderInvalidationHandle` obtained from presentation A's pipeline BEFORE
/// A closes, and still held afterward, must fail closed
/// (`DirtySendError::OwnerGone`) rather than panic when used — the
/// underlying `PipelineOwner`'s dirty-request channel receiver drops
/// along with A's `PresentationState`, so the handle's sender side
/// simply finds nobody listening.
#[test]
fn dropped_presentations_surviving_pipeline_handles_fail_closed() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();

    let render_id = realm.presentations.primary().pipeline().with_mut(|owner| {
        owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        ))
    });
    let render_invalidation_handle = realm
        .presentations
        .primary()
        .pipeline()
        .with(|owner| owner.render_invalidation_handle(render_id))
        .expect("a freshly inserted node has a live repaint handle");

    assert!(
        render_invalidation_handle.mark_needs_layout().is_ok(),
        "precondition: the handle works while A is still alive"
    );

    assert!(realm.close_presentation_entered(a_id), "A was installed");

    assert!(
        matches!(
            render_invalidation_handle.mark_needs_layout(),
            Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
        ),
        "a RenderInvalidationHandle surviving its presentation's teardown must \
         fail closed, not panic and not silently succeed"
    );
}

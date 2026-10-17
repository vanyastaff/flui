use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_layer::TextureRegistry;
use flui_painting::paint::Image;

fn frame(shade: u8) -> Image {
    Image::from_rgba8(1, 1, vec![shade, shade, shade, 255])
}

fn counting_registry() -> (TextureRegistry, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let registry = TextureRegistry::new(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    (registry, wakes)
}

/// Ids are distinct and a dropped registration's id is never issued again.
pub(crate) fn ids_are_never_reissued() {
    let (registry, _) = counting_registry();
    let first = registry.register().expect("id");
    let first_id = first.id();
    drop(first);
    let second = registry.register().expect("id");
    assert_ne!(second.id(), first_id);
    assert_eq!(
        registry.revision(first_id),
        None,
        "a retired id is not live"
    );
}

/// The backend sees only the newest frame per texture, once, and every frame
/// wakes the presentation.
pub(crate) fn the_backend_takes_the_newest_frame_once() {
    let (registry, wakes) = counting_registry();
    let texture = registry.register().expect("id");
    texture.submit_frame(frame(10));
    texture.submit_frame(frame(20));
    assert_eq!(registry.revision(texture.id()), Some(2));
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    let updates = registry.take_updates();
    assert_eq!(updates.frames.len(), 1);
    assert_eq!(updates.frames[0].0, texture.id());
    assert_eq!(updates.frames[0].1.data(), [20, 20, 20, 255]);
    assert!(
        registry.take_updates().is_empty(),
        "a frame is applied once"
    );
}

/// Dropping a registration retires its id for the backend and wakes it.
pub(crate) fn dropping_a_registration_retires_it() {
    let (registry, wakes) = counting_registry();
    let texture = registry.register().expect("id");
    let id = texture.id();
    texture.submit_frame(frame(1));
    drop(texture);
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    let updates = registry.take_updates();
    assert!(
        updates.frames.is_empty(),
        "a retired texture's frame is not drawn"
    );
    assert_eq!(updates.retired, [id]);
}

/// Listeners run after each frame, may reach the registry again, and stop
/// when their guard drops.
pub(crate) fn listeners_run_reentrantly_until_removed() {
    let (registry, _) = counting_registry();
    let texture = registry.register().expect("id");
    let id = texture.id();
    let seen = Arc::new(AtomicUsize::new(0));
    let observer = registry.clone();
    let record = Arc::clone(&seen);
    let guard = registry
        .listen(id, move || {
            let revision = observer.revision(id).expect("live");
            record.store(usize::try_from(revision).expect("small"), Ordering::SeqCst);
        })
        .expect("live id");
    texture.submit_frame(frame(1));
    texture.submit_frame(frame(2));
    assert_eq!(seen.load(Ordering::SeqCst), 2);
    drop(guard);
    texture.submit_frame(frame(3));
    assert_eq!(
        seen.load(Ordering::SeqCst),
        2,
        "a removed listener is not called"
    );
    drop(texture);
    assert!(
        registry.listen(id, || {}).is_none(),
        "a retired id takes no listener"
    );
}

/// A panicking listener neither skips the listeners after it nor the wake:
/// the frame stays deliverable, the first panic is the one resumed, and the
/// next frame is delivered the same way.
pub(crate) fn a_panicking_listener_keeps_the_frame_deliverable() {
    let (registry, wakes) = counting_registry();
    let texture = registry.register().expect("id");
    let id = texture.id();
    let later = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&later);
    let _first = registry
        .listen(id, || std::panic::panic_any("first listener"))
        .expect("live id");
    let _counting = registry
        .listen(id, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .expect("live id");
    let _second = registry
        .listen(id, || std::panic::panic_any("second listener"))
        .expect("live id");
    for (shade, delivered) in [(1_u8, 1_usize), (2, 2)] {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            texture.submit_frame(frame(shade));
        }))
        .expect_err("the listener's panic propagates");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"first listener"),
            "the first panic is authoritative"
        );
        assert_eq!(
            later.load(Ordering::SeqCst),
            delivered,
            "later listeners run"
        );
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            delivered,
            "the presentation wakes"
        );
        let updates = registry.take_updates();
        assert_eq!(updates.frames.len(), 1);
        assert_eq!(updates.frames[0].1.data(), [shade, shade, shade, 255]);
    }
}

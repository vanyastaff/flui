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
    assert_eq!(registry.take_updates().retired, [first_id]);
}

/// The backend sees only the newest frame per texture, once, and every frame
/// wakes the presentation; the registry holds updates until they are taken.
pub(crate) fn the_backend_takes_the_newest_frame_once() {
    let (registry, wakes) = counting_registry();
    let texture = registry.register().expect("id");
    assert!(!registry.has_updates(), "a registration alone is no update");
    texture.submit_frame(frame(10));
    texture.submit_frame(frame(20));
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert!(registry.has_updates());
    let updates = registry.take_updates();
    assert_eq!(updates.frames.len(), 1);
    assert_eq!(updates.frames[0].0, texture.id());
    assert_eq!(updates.frames[0].1.data(), [20, 20, 20, 255]);
    assert!(!registry.has_updates(), "the frame was taken");
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
    assert!(registry.has_updates(), "a retirement is an update");
    let updates = registry.take_updates();
    assert!(
        updates.frames.is_empty(),
        "a retired texture's frame is not drawn"
    );
    assert_eq!(updates.retired, [id]);
    assert!(!registry.has_updates());
}

/// The wake runs with the registry unlocked, so the host's wake may read it
/// again, as a runtime that checks for updates at once does.
pub(crate) fn the_wake_may_reach_the_registry() {
    let seen = Arc::new(AtomicUsize::new(0));
    let slot: Arc<std::sync::OnceLock<TextureRegistry>> = Arc::default();
    let registry = {
        let (seen, slot) = (Arc::clone(&seen), Arc::clone(&slot));
        TextureRegistry::new(Arc::new(move || {
            let registry = slot.get().expect("set before the first frame");
            if registry.has_updates() {
                seen.fetch_add(1, Ordering::SeqCst);
            }
        }))
    };
    slot.set(registry.clone()).expect("set once");
    let texture = registry.register().expect("id");
    texture.submit_frame(frame(1));
    drop(texture);
    assert_eq!(
        seen.load(Ordering::SeqCst),
        2,
        "the frame and the retirement are pending when the wake runs"
    );
}

/// A wake that panics leaves the frame pending: it is delivered by whatever
/// frame the presentation draws next, and the next frame wakes again.
pub(crate) fn a_panicking_wake_keeps_the_frame_pending() {
    let registry = TextureRegistry::new(Arc::new(|| std::panic::panic_any("wake")));
    let texture = registry.register().expect("id");
    for shade in [1_u8, 2] {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            texture.submit_frame(frame(shade));
        }))
        .expect_err("the wake's panic propagates");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"wake"));
        assert!(registry.has_updates(), "the frame is pending");
        let updates = registry.take_updates();
        assert_eq!(updates.frames.len(), 1);
        assert_eq!(updates.frames[0].1.data(), [shade, shade, shade, 255]);
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(texture)))
        .expect_err("the retirement's wake panics too");
    assert!(registry.has_updates(), "the retirement is pending");
    assert_eq!(registry.take_updates().retired.len(), 1);
}

/// Two updates merged apply as both would in turn: a newer frame replaces
/// the older one of its texture, a texture retired later loses its older
/// frame, and every retirement reaches the backend.
pub(crate) fn merged_updates_keep_the_newest_frame_and_every_retirement() {
    let (registry, _) = counting_registry();
    let kept = registry.register().expect("id");
    let replaced = registry.register().expect("id");
    let retired_early = registry.register().expect("id");
    let retired_late = registry.register().expect("id");
    let (kept_id, replaced_id) = (kept.id(), replaced.id());
    let (early_id, late_id) = (retired_early.id(), retired_late.id());
    kept.submit_frame(frame(1));
    replaced.submit_frame(frame(2));
    retired_late.submit_frame(frame(3));
    drop(retired_early);
    let mut older = registry.take_updates();
    replaced.submit_frame(frame(4));
    drop(retired_late);
    let newer = registry.take_updates();
    older.merge(newer);

    let mut frames: Vec<_> = older
        .frames
        .iter()
        .map(|(id, image)| (*id, image.data()[0]))
        .collect();
    frames.sort_by_key(|(id, _)| id.get());
    assert_eq!(frames, [(kept_id, 1), (replaced_id, 4)]);
    let mut retired = older.retired.clone();
    retired.sort_by_key(|id| id.get());
    assert_eq!(retired, [early_id, late_id]);
}

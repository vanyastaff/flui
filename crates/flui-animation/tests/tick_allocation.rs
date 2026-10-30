//! Running frames and retirement of idle registry owners must not allocate.
//!
//! One `Vsync::tick_all` advances a live scalar controller, four-component
//! insets and a seven-component nested geometry/color value, each with four
//! value listeners. Observer conversion and scalar keyframe evaluation are
//! included in the measured window with a counting `#[global_allocator]`. That
//! allocator is process-wide, which is why this file is a test target of its
//! own rather than a module of `tests/main.rs`.
//!
//! Each measured claim includes a positive allocation control. Counters are
//! per-thread, so concurrent lifecycle cases and libtest's harness thread cannot
//! add allocations to another case's measured window. The retirement case has
//! no active run or user callbacks; admission remains outside its window.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use std::time::Duration;

use flui_animation::{
    AnimatedValue, Animation, AnimationController, Keyframes, LinearStop, MotionClock, MotionSpec,
    PiecewiseLinear, ReverseAnimation, SharedCurve, SpringCurve, SpringDescription, Tolerance,
    TwoWayConverter, Vsync, curves,
};
use flui_foundation::Listenable;
use flui_foundation::geometry::{EdgeInsets, Offset};
use flui_painting::styling::Color;

#[derive(Clone, TwoWayConverter)]
struct Appearance {
    position: Offset<f64>,
    color: Color,
}

#[derive(Clone, TwoWayConverter)]
struct CardMotion(Appearance, f64);

// `Cell<usize>` in a const-initialised thread-local has no drop glue and no
// lazy init, so reading it cannot itself allocate or run during TLS teardown.
thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOC_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Add to a counter without allocating or panicking during TLS teardown.
fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>, by: usize) {
    let _ = counter.try_with(|c| c.set(c.get().wrapping_add(by)));
}

/// Read a counter; `0` if this thread's TLS is already gone.
fn read(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
    counter.try_with(Cell::get).unwrap_or(0)
}

struct CountingAllocator;

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. `realloc`/`alloc_zeroed` keep
// `GlobalAlloc`'s defaults, which decompose into `self.alloc` + copy +
// `self.dealloc`: still `System`, and still counted through `alloc` (an
// in-place `System::realloc` growth would count as an allocation — never a
// miss).
#[expect(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(&ALLOC_COUNT, 1);
        bump(&ALLOC_BYTES, layout.size());
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// A run no frame in this test can finish.
const NEVER_ENDING: Duration = Duration::from_secs(u32::MAX as u64);

/// One frame at 60 Hz, in seconds.
const FRAME: f64 = 1.0 / 60.0;

/// Idle owner teardown has no user run or callback to prepare. Admission is
/// measured separately as a positive control; observers keep the allocation
/// alive while actual disposal must withdraw the registry and refuse new runs.
#[test]
fn retiring_idle_registry_owners_allocates_nothing() {
    for count in [1, 1_000, 10_000] {
        let admission_before = read(&ALLOC_COUNT);
        let registry = Vsync::new();
        let mut observers = Vec::with_capacity(count);
        let owners: Vec<_> = (0..count)
            .map(|_| {
                let owner =
                    AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
                observers.push(owner.controller().clone());
                owner
            })
            .collect();
        assert_eq!(registry.len(), count);
        assert!(
            read(&ALLOC_COUNT) > admission_before,
            "admission must exercise the same counting allocator"
        );
        let calls_before = read(&ALLOC_COUNT);
        let bytes_before = read(&ALLOC_BYTES);
        for mut owner in owners {
            black_box(&mut owner).dispose();
        }
        let calls = read(&ALLOC_COUNT) - calls_before;
        let bytes = read(&ALLOC_BYTES) - bytes_before;
        assert!(registry.is_empty(), "every retired seat is withdrawn");
        for observer in observers {
            assert!(
                matches!(
                    observer.forward(),
                    Err(flui_animation::AnimationError::Disposed)
                ),
                "an observing clone cannot restart the retired owner"
            );
        }
        assert_eq!(
            calls, 0,
            "retiring {count} idle registry owners allocated {calls} times ({bytes} bytes)"
        );
    }
}

/// 1. **The claim:** after warmup, every `tick_all` that advances a running
///    controller and notifies its four value listeners allocates nothing.
/// 2. **The negative control:** starting a run on the same controller does
///    allocate (its `AnimationRunFuture`), so a counter that counts nothing cannot
///    pass claim 1. It runs after the measured loop, so it cannot taint it.
#[test]
fn a_steady_state_frame_allocates_nothing() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(NEVER_ENDING).build_on(Some(&vsync));
    let controller = owner.controller();
    for _ in 0..4 {
        controller.add_listener(std::rc::Rc::new(|| {
            black_box(());
        }));
    }
    controller
        .subscribe_status(std::rc::Rc::new(|status| {
            black_box(status);
        }))
        .detach();
    let _run = controller.forward().expect("forward on a live controller");
    let mut vector = AnimatedValue::new(
        EdgeInsets::ZERO,
        MotionSpec::Curve {
            duration: Duration::from_secs(3600),
            curve: SharedCurve::new(curves::LINEAR),
        },
        Some(&vsync),
    )
    .expect("finite vector with a live registry");
    let stream = vector.animation();
    for _ in 0..4 {
        stream.add_listener(std::rc::Rc::new(|| {
            black_box(());
        }));
    }
    let _vector_run = vector
        .animate_to(EdgeInsets::new(10.0, 20.0, 30.0, 40.0))
        .expect("finite vector target");

    // Seven components exceed the component run's inline segment storage.
    // Admission may allocate; sampling and rebuilding this value may not.
    let mut nested = AnimatedValue::new(
        CardMotion(
            Appearance {
                position: Offset::ZERO,
                color: Color::rgb(255, 0, 0),
            },
            1.0,
        ),
        MotionSpec::Curve {
            duration: Duration::from_secs(3600),
            curve: SharedCurve::new(curves::LINEAR),
        },
        Some(&vsync),
    )
    .expect("finite nested value with a live registry");
    let nested_stream = nested.animation();
    let nested_deliveries = std::rc::Rc::new(Cell::new(0usize));
    for _ in 0..4 {
        let delivered = nested_deliveries.clone();
        nested_stream.add_listener(std::rc::Rc::new(move || {
            delivered.set(delivered.get() + 1);
        }));
    }
    let _nested_run = nested
        .animate_to(CardMotion(
            Appearance {
                position: Offset::new(10.0, 20.0),
                color: Color::rgba(255, 0, 0, 0),
            },
            2.0,
        ))
        .expect("finite nested target");
    let track = Keyframes::builder(0.0, Duration::from_secs(3600))
        .cubic(1.0, Duration::from_mins(30))
        .cubic(0.0, Duration::from_mins(30))
        .build()
        .expect("finite scalar keyframe track");
    let eased_tracks = [
        Keyframes::builder(0.0, Duration::from_secs(3600))
            .to(
                1.0,
                Duration::from_secs(3600),
                PiecewiseLinear::new(&[
                    LinearStop::new(0.0),
                    LinearStop::new(0.25),
                    LinearStop::new(1.0),
                ])
                .expect("normalized stops"),
            )
            .build()
            .expect("finite piecewise track"),
        Keyframes::builder(0.0, Duration::from_secs(3600))
            .to(
                1.0,
                Duration::from_secs(3600),
                SpringCurve::new(
                    SpringDescription::new(1.0, 100.0, 20.0).expect("critical spring"),
                    Tolerance::DEFAULT,
                )
                .expect("finite spring duration"),
            )
            .build()
            .expect("finite spring track"),
    ];

    // Warmup: the first frame anchors the run; later ones settle any
    // one-time lazy initialisation outside the measured path.
    let mut now = 0.0_f64;
    let mut clock = MotionClock::new();
    for _ in 0..16 {
        vsync.tick_all(&clock.frame(Duration::from_secs_f64(now)));
        now += FRAME;
    }

    const FRAMES: usize = 10_000;
    let mut frames_with_allocation = 0usize;
    let mut total_calls = 0usize;
    let mut total_bytes = 0usize;
    let mut worst_frame_bytes = 0usize;
    let start_value = controller.value();
    let start_insets = vector.value();
    let start_nested = nested_stream.value();
    let deliveries_before = nested_deliveries.get();
    let start_track = track.value_at(Duration::from_secs_f64(now));
    let start_eased = eased_tracks
        .each_ref()
        .map(|track| track.value_at(Duration::from_secs_f64(now)));
    for _ in 0..FRAMES {
        let calls_before = read(&ALLOC_COUNT);
        let bytes_before = read(&ALLOC_BYTES);

        vsync.tick_all(&clock.frame(Duration::from_secs_f64(now)));
        now += FRAME;

        black_box(stream.value());
        black_box(nested_stream.value());
        black_box(track.value_at(Duration::from_secs_f64(now)));
        for track in &eased_tracks {
            black_box(track.value_at(Duration::from_secs_f64(now)));
        }
        let calls = read(&ALLOC_COUNT) - calls_before;
        let bytes = read(&ALLOC_BYTES) - bytes_before;
        total_calls += calls;
        total_bytes += bytes;
        if calls != 0 {
            frames_with_allocation += 1;
            worst_frame_bytes = worst_frame_bytes.max(bytes);
        }
    }

    for (track, start) in eased_tracks.iter().zip(start_eased) {
        assert!(
            track.value_at(Duration::from_secs_f64(now)) > start,
            "authored easing advances throughout the measured window"
        );
    }
    assert!(
        controller.status().is_directional() && controller.value() > start_value,
        "the measured frames must advance a live run, or they priced an early return"
    );
    let final_insets = vector.value();
    assert!(
        !vector.is_settled()
            && final_insets.top > start_insets.top
            && final_insets.right > start_insets.right
            && final_insets.bottom > start_insets.bottom
            && final_insets.left > start_insets.left,
        "the measured vector must advance every component of its live run"
    );
    let final_nested = nested_stream.value();
    assert!(
        !nested.is_settled()
            && final_nested.0.position.dx > start_nested.0.position.dx
            && final_nested.0.position.dy > start_nested.0.position.dy
            && final_nested.0.color.opacity() < start_nested.0.color.opacity()
            && final_nested.1 > start_nested.1,
        "the nested geometry, color and scalar must advance during measurement"
    );
    assert_eq!(nested_deliveries.get() - deliveries_before, 4 * FRAMES);
    assert!(track.value_at(Duration::from_secs_f64(now)) > start_track);
    eprintln!(
        "tick_allocation: {FRAMES} frames -- {total_calls} allocating calls, {total_bytes} \
         bytes, {worst_frame_bytes} bytes on the worst frame, {frames_with_allocation} frames \
         allocated at least once"
    );
    assert_eq!(
        frames_with_allocation, 0,
        "{frames_with_allocation} of {FRAMES} steady-state frames allocated (worst: \
         {worst_frame_bytes} bytes); advancing a live run and notifying up to four value \
         listeners must not allocate"
    );

    let calls_before_start = read(&ALLOC_COUNT);
    let restarted = controller.forward_from(Some(0.0));
    let calls_after_start = read(&ALLOC_COUNT);
    assert!(restarted.is_ok());
    assert!(
        calls_after_start > calls_before_start,
        "starting a run must allocate its future; if it does not, the counter is not \
         counting and the zero-allocation claim above proves nothing"
    );

    // Wrapper composition must preserve the same frame-path contract. Each
    // channel has only one listener, so this measures relay depth rather than
    // a listener snapshot exceeding its inline capacity.
    for depth in [1, 5, 32] {
        let vsync = Vsync::new();
        let owner = AnimationController::builder(NEVER_ENDING).build_on(Some(&vsync));
        let controller = owner.controller();
        let mut leaf: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(controller.clone());
        for _ in 0..depth {
            leaf = std::rc::Rc::new(ReverseAnimation::new(leaf));
        }
        let delivered = std::rc::Rc::new(Cell::new(0usize));
        let observed = delivered.clone();
        leaf.add_listener(std::rc::Rc::new(move || observed.set(observed.get() + 1)));
        let _run = controller.forward().expect("forward on a live controller");
        let mut clock = flui_animation::MotionClock::new();
        let mut now = 0.0;
        for _ in 0..16 {
            vsync.tick_all(&clock.frame(Duration::from_secs_f64(now)));
            now += FRAME;
        }
        let start_value = leaf.value();
        let deliveries_before = delivered.get();
        let calls_before = read(&ALLOC_COUNT);
        let bytes_before = read(&ALLOC_BYTES);
        for _ in 0..FRAMES {
            vsync.tick_all(&clock.frame(Duration::from_secs_f64(now)));
            now += FRAME;
        }
        let calls = read(&ALLOC_COUNT) - calls_before;
        let bytes = read(&ALLOC_BYTES) - bytes_before;
        assert_eq!(delivered.get() - deliveries_before, FRAMES);
        assert!(controller.status().is_directional() && leaf.value() != start_value);
        assert_eq!(
            calls, 0,
            "{depth} animation relays allocated {calls} times ({bytes} bytes)"
        );
    }

    // Real subtree registries must have the same steady-state contract as a
    // flat registry. Keep both seats alive: otherwise this prices detached work.
    let root = Vsync::new();
    let child = Vsync::new();
    let grandchild = Vsync::new();
    let _child_seat = root.attach_child(&child).expect("fresh child registry");
    let _grandchild_seat = child
        .attach_child(&grandchild)
        .expect("fresh grandchild registry");
    let owner = AnimationController::builder(NEVER_ENDING).build_on(Some(&grandchild));
    let controller = owner.controller();
    let delivered = std::rc::Rc::new(Cell::new(0usize));
    let observed = delivered.clone();
    controller.add_listener(std::rc::Rc::new(move || observed.set(observed.get() + 1)));
    let _run = controller.forward().expect("live nested run");
    let mut clock = MotionClock::new();
    let mut now = 0.0;
    for _ in 0..16 {
        root.tick_all(&clock.frame(Duration::from_secs_f64(now)));
        assert!(root.has_running());
        now += FRAME;
    }
    let start_value = controller.value();
    let deliveries_before = delivered.get();
    let calls_before = read(&ALLOC_COUNT);
    let bytes_before = read(&ALLOC_BYTES);
    for _ in 0..FRAMES {
        root.tick_all(&clock.frame(Duration::from_secs_f64(now)));
        black_box(root.has_running());
        now += FRAME;
    }
    let calls = read(&ALLOC_COUNT) - calls_before;
    let bytes = read(&ALLOC_BYTES) - bytes_before;
    assert!(root.has_running() && controller.value() > start_value);
    assert_eq!(delivered.get() - deliveries_before, FRAMES);
    eprintln!(
        "nested registry allocation: {FRAMES} frames -- {calls} allocating calls, {bytes} bytes"
    );
    assert_eq!(
        calls, 0,
        "advancing and querying a live nested registry allocated {calls} times ({bytes} bytes)"
    );
}

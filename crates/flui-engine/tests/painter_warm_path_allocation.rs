//! A warm painter path's recording allocations: cached path draws may grow
//! the recording arenas geometrically, but must not allocate a temporary
//! vertex buffer for every draw.
//!
//! A dedicated integration-test binary (never the crate's `--lib` unit
//! tests) specifically so installing a counting `#[global_allocator]` here
//! cannot affect any other test binary in this workspace — each `tests/*.rs`
//! file compiles to its own process, and `#[global_allocator]` is
//! process-wide.

#![cfg(feature = "testing")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

// Per-thread, not process-global: a `GlobalAlloc` sees every thread's
// allocations, so process-wide counters charge any other thread's work to the
// measured window. libtest runs a test on a spawned thread while its own
// harness thread lives on, which made the measurement load-sensitive and
// intermittently wrong. `Cell<usize>` in a const-initialised thread-local has
// no drop glue and no lazy init, so reading it cannot itself allocate or run
// during TLS teardown.
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

// SAFETY: `alloc` and `dealloc` forward their arguments unchanged to `System`,
// the platform default allocator, so a pointer is always freed by the
// allocator that produced it. `realloc` and `alloc_zeroed` are deliberately
// NOT overridden: `GlobalAlloc`'s defaults decompose them into `self.alloc` +
// copy + `self.dealloc`, which lands in `System` too and, as a side effect,
// counts an in-place `System::realloc` growth as an alloc — conservative for
// this harness, never a miss. Overriding either one later MUST keep the
// counter bumps, or every `Vec` growth becomes invisible and the
// zero-allocation assertion silently turns into a false pass. (The workspace's
// other counting allocator, in `flui-interaction`'s pointer-route test,
// overrides all four and forwards each to `System` — do not reconcile the two
// by copying its shape without carrying the counters across.)
//
// The counters are per-thread and the measured draws run on the test thread,
// so the post-loop read observes every bump of the measured region.
// `System` itself upholds `GlobalAlloc`'s contract; this wrapper adds no new
// invariant for the caller to uphold.
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

/// Warm cached paths may grow the recording arenas geometrically, but must
/// not allocate a temporary vertex buffer for every draw. This uses the real
/// public recorder, with encoding checked outside the measured region.
#[test]
fn warm_path_recording_has_no_per_draw_vertex_allocation() {
    use std::sync::Arc;

    use flui_engine::{WgpuPainter, wgpu};
    use flui_foundation::geometry::Point;
    use flui_painting::{Paint, paint::path::Path, styling::Color};

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        apply_limit_buckets: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) => {
            assert!(
                std::env::var_os("FLUI_REQUIRE_GPU").is_none(),
                "FLUI_REQUIRE_GPU is set, but the warm path allocation test has no GPU adapter: {error}"
            );
            eprintln!("warm path allocation test skipped: no GPU adapter ({error})");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Warm path allocation test"),
        ..Default::default()
    }))
    .expect("allocation test device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Warm path allocation target"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut painter =
        WgpuPainter::with_shared_device(Arc::clone(&device), queue, format, (128, 128));
    let mut path = Path::new();
    for i in 0..128 {
        let angle = f64::from(i) * std::f64::consts::TAU / 128.0;
        let point = Point::new(64.0 + 48.0 * angle.cos(), 64.0 + 48.0 * angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
    let paint = Paint::stroke(Color::RED, 2.0).with_anti_alias(false);
    painter.begin_frame().expect("allocation frame begins");
    const DRAWS: usize = 128;
    for _ in 0..DRAWS {
        painter.draw_path(&path, &paint);
    }
    let before_calls = read(&ALLOC_COUNT);
    let before_bytes = read(&ALLOC_BYTES);
    for _ in 0..DRAWS {
        painter.draw_path(&path, &paint);
    }
    let calls = read(&ALLOC_COUNT) - before_calls;
    let bytes = read(&ALLOC_BYTES) - before_bytes;
    eprintln!("warm path recording: {DRAWS} cached draws, {calls} allocations, {bytes} bytes");
    // Allow a small number of geometric arena expansions. A per-path
    // temporary Vec alone would require at least DRAWS allocations.
    assert!(
        calls <= 8,
        "warm draws allocated {calls} times for {DRAWS} paths"
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("all measured path draws must encode, without a latched budget refusal");
    drop(encoder);
    painter.finish_frame();
}

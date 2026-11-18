//! ADR-0306 §5: commands under one transform entry share its reference, so
//! the dispatcher reuses the installed matrix by address and compares values
//! only for a command under another entry.

use super::*;
use flui_painting::{Canvas, paint::Paint, styling::Color};

#[test]
fn commands_under_one_entry_reuse_the_installed_transform_by_address() {
    let (device, queue) = test_device_and_queue();
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
    );
    let paint = Paint::fill(Color::RED);
    let mut canvas = Canvas::new();
    canvas.translate(2.0, 3.0);
    for x in 0..3 {
        canvas.draw_rect(Rect::from_xywh(f64::from(x), 0.0, 1.0, 1.0), &paint);
    }
    canvas.translate(1.0, 0.0);
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 1.0, 1.0), &paint);
    let list = canvas.finish();

    let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(&mut painter);
    crate::dispatch::dispatch_commands(&list, &mut dispatcher);
    // The second and third rectangles name the first one's entry; the fourth
    // names another entry and is compared by value once.
    assert_eq!(dispatcher.value_comparisons(), 1);
}

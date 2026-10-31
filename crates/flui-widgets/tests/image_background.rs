//! `Image::memory` and `Image::file` decode off the owner thread: once per
//! key, on the UI runtime's background lanes, and on the owner thread only
//! for a host without them.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{LaidOut, lay_out, loose, size};
use flui_painting::paint::Image as PixelImage;
use flui_widgets::{DecodeJob, Image, ImageCacheKey, ImageProvider, ImageProviderError};

/// A 5×3 PNG.
const FIVE_BY_THREE: &[u8] = include_bytes!("fixtures/tiny.png");
/// A 4×2 PNG.
const FOUR_BY_TWO: &[u8] = include_bytes!("fixtures/tiny_4x2.png");

/// Pumps until the background work is done and its answer rebuilt the
/// widget, within a bound.
fn settle(laid: &mut LaidOut) {
    for _ in 0..8 {
        laid.tick();
        if laid.pending_background() == 0 {
            laid.tick();
            return;
        }
    }
    panic!("background work did not settle");
}

/// The bytes decode on a background lane, not inside the build that mounts
/// the widget; a rebuild with equal bytes in a new buffer decodes nothing
/// again and keeps the frame.
///
/// Red-check: decode `Image::memory` in `build` (the frame shows at mount and
/// no work is queued).
fn memory_image_decodes_in_the_background_once_per_bytes() {
    let mut laid = lay_out(Image::memory(FIVE_BY_THREE), loose(100.0));
    assert!(
        !laid.image_has_image(laid.current_root()),
        "the mounting build shows the placeholder"
    );
    assert_eq!(laid.pending_background(), 1, "the decode is queued");
    settle(&mut laid);
    assert_eq!(laid.size(laid.current_root()), size(5.0, 3.0));

    laid.pump_widget(Image::memory(FIVE_BY_THREE.to_vec()));
    assert_eq!(laid.pending_background(), 0, "equal bytes are the same key");
    assert!(laid.image_has_image(laid.current_root()));
    assert_eq!(laid.size(laid.current_root()), size(5.0, 3.0));
}

/// A file is read and decoded in the background and shows once both answer.
fn file_image_reads_and_decodes_in_the_background() {
    let directory = std::env::temp_dir().join(format!(
        "flui-image-background-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join("image.png");
    std::fs::write(&path, FOUR_BY_TWO).expect("fixture writes");

    let mut laid = lay_out(Image::file(&path), loose(100.0));
    assert!(!laid.image_has_image(laid.current_root()));
    assert_eq!(laid.pending_background(), 1, "the read is queued");
    settle(&mut laid);
    assert_eq!(laid.size(laid.current_root()), size(4.0, 2.0));
    let _ = std::fs::remove_dir_all(directory);
}

/// A rebuild that changes the bytes while the first decode waits cancels it:
/// only the second image ever shows.
fn a_new_key_shows_only_the_new_bytes() {
    let mut laid = lay_out(Image::memory(FIVE_BY_THREE), loose(100.0));
    laid.hold_background();
    laid.pump_widget(Image::memory(FOUR_BY_TWO));
    laid.release_background();
    settle(&mut laid);
    assert_eq!(laid.size(laid.current_root()), size(4.0, 2.0));
}

/// Counts the decode jobs it hands out, and decodes a solid 2×2 image.
#[derive(Debug, Clone, Default)]
struct CountedDecode {
    jobs: Arc<AtomicUsize>,
    ran: Arc<AtomicUsize>,
}

impl ImageProvider for CountedDecode {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Ok(PixelImage::from_rgba8(2, 2, vec![255; 16]))
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::custom("counted"))
    }

    fn decode_job(&self) -> Option<DecodeJob> {
        self.jobs.fetch_add(1, Ordering::SeqCst);
        let ran = Arc::clone(&self.ran);
        Some(DecodeJob::decode(move || {
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(PixelImage::from_rgba8(2, 2, vec![255; 16]))
        }))
    }
}

/// On a host without background lanes, a decode job runs on the owner
/// thread, and the image still shows.
///
/// Red-check: skip the job when the capability is missing (nothing runs).
fn without_background_lanes_a_decode_job_runs_on_the_owner_thread() {
    use flui_runtime::capability::CapabilityRegistry;
    use flui_testing::{HeadlessHost, HeadlessWindow};

    // A registry that provides nothing, background lanes included.
    let registry = CapabilityRegistry::builder()
        .build()
        .expect("an empty registry");
    let mut host = HeadlessHost::with_capabilities(HeadlessWindow::new(10, 10), registry);
    // The host's own lanes are held, so a job that runs ran on the owner
    // thread.
    host.hold_background();
    let provider = CountedDecode::default();
    host.attach(&Image::new(provider.clone()))
        .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(provider.jobs.load(Ordering::SeqCst), 1);
    assert_eq!(
        provider.ran.load(Ordering::SeqCst),
        1,
        "the job ran on the owner thread"
    );
    assert_eq!(host.pending_background(), 0);
}

/// `Image::memory` and `Image::file` decode once per key on the background
/// lanes, a new key cancels the waiting decode, and a host without lanes
/// runs the decode on the owner thread.
#[test]
fn image_decodes_in_the_background() {
    memory_image_decodes_in_the_background_once_per_bytes();
    file_image_reads_and_decodes_in_the_background();
    a_new_key_shows_only_the_new_bytes();
    without_background_lanes_a_decode_job_runs_on_the_owner_thread();
}

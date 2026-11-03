//! Where and when image decodes run, observed through real decoder hooks. A
//! target of its own because image hook registrations are global and cannot
//! be removed.
//!
//! Each test registers a format whose files are an eight-byte signature
//! followed by the committed PNG fixture, so its hook sees exactly its own
//! decodes. Loads run on a native host's default lanes; network images are
//! scripted, never fetched.

#[allow(
    dead_code,
    reason = "shared with the main test binary, which uses the queued lanes"
)]
mod lanes;

use std::ffi::OsString;
use std::io::{Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use flui_assets::{AssetRegistryBuilder, ImageSource};
use flui_platform::testing::{ScriptedHttp, ScriptedResponse};
use lanes::{ThreadLanes, block_on};

const PNG: &[u8] = include_bytes!("fixtures/tiny.png");
const BUDGET: Duration = Duration::from_secs(5);

/// Registers `signature` as an image format whose decode first calls `before`.
fn register_signed_png(
    signature: &'static [u8; 8],
    extension: &str,
    before: impl Fn() -> std::io::Result<()> + Send + Sync + 'static,
) {
    let extension = OsString::from(extension);
    assert!(image::hooks::register_decoding_hook(
        extension.clone(),
        Box::new(move |mut reader| {
            before().map_err(image::ImageError::IoError)?;
            reader.seek(SeekFrom::Start(signature.len() as u64))?;
            image::codecs::png::PngDecoder::new(reader)
                .map(|decoder| Box::new(decoder) as Box<dyn image::ImageDecoder + '_>)
        }),
    ));
    image::hooks::register_format_detection_hook(extension, signature, None);
}

/// Files of the signed format in a directory removed on drop.
struct SignedFiles(PathBuf);

impl SignedFiles {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "flui-assets-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("test directory is created");
        Self(directory)
    }

    fn write(&self, signature: &[u8; 8], name: &str) -> String {
        let path = self.0.join(name);
        let mut bytes = signature.to_vec();
        bytes.extend_from_slice(PNG);
        std::fs::write(&path, bytes).expect("signed fixture is written");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for SignedFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime starts")
}

/// A decode that waits for a task on the IO lane's only worker completes:
/// the decode runs on the compute lane, so the IO lane is free to run that
/// task. Decoding on the IO lane would wait for itself.
#[test]
fn decodes_leave_the_io_lane_free() {
    const SIGNATURE: &[u8; 8] = b"FLUIWRK1";
    let lanes = ThreadLanes::new();
    let worker = lanes.io_handle();
    register_signed_png(SIGNATURE, "flui-worker-probe", move || {
        let (tx, rx) = std::sync::mpsc::channel();
        worker.spawn(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| std::io::Error::other("the IO lane's worker is blocked"))
    });
    let files = SignedFiles::new("worker-probe");
    let path = files.write(SIGNATURE, "probe.img");
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();

    let image = block_on(registry.load_image(path, lanes.services()))
        .expect("the decode reaches a free IO worker");
    assert_eq!((image.width(), image.height()), (4, 2));
}

/// Decodes the gated format has run, and the gate they wait behind.
#[derive(Default)]
struct Gate {
    decodes: AtomicUsize,
    open: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn pass(&self) -> std::io::Result<()> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        let open = self.open.lock().expect("gate lock");
        let (_open, timeout) = self
            .opened
            .wait_timeout_while(open, Duration::from_secs(5), |open| !*open)
            .expect("gate lock");
        if timeout.timed_out() {
            return Err(std::io::Error::other("the gate never opened"));
        }
        Ok(())
    }

    fn open(&self) {
        *self.open.lock().expect("gate lock") = true;
        self.opened.notify_all();
    }

    fn wait_for_decodes(&self, count: usize) {
        let deadline = Instant::now() + BUDGET;
        while self.decodes.load(Ordering::SeqCst) < count {
            assert!(Instant::now() < deadline, "{count} decodes never started");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// With the only decode slot taken, loads abandoned while they wait never
/// decode, and a live load queued after them is the next to decode.
#[test]
fn abandoned_loads_never_decode_and_do_not_delay_a_live_one() {
    const SIGNATURE: &[u8; 8] = b"FLUIGAT1";
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    register_signed_png(SIGNATURE, "flui-gated", move || hook_gate.pass());
    let files = SignedFiles::new("gated");
    let lanes = ThreadLanes::new();
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();

    let blocker = registry.load_image(files.write(SIGNATURE, "blocker.img"), lanes.services());
    gate.wait_for_decodes(1);
    let stale: Vec<_> = (0..5)
        .map(|index| {
            registry.load_image(
                files.write(SIGNATURE, &format!("stale-{index}.img")),
                lanes.services(),
            )
        })
        .collect();
    // Let the stale reads finish so they wait for the decode slot.
    std::thread::sleep(Duration::from_millis(100));
    drop(stale);
    let live_path = files.write(SIGNATURE, "live.img");
    let live = registry.load_image(live_path.clone(), lanes.services());
    gate.open();

    let runtime = current_thread();
    for load in [blocker, live] {
        let image = runtime
            .block_on(async { tokio::time::timeout(BUDGET, load).await })
            .expect("the load completes")
            .expect("the image decodes");
        assert_eq!((image.width(), image.height()), (4, 2));
    }
    assert_eq!(
        gate.decodes.load(Ordering::SeqCst),
        2,
        "only the blocker and the live load decode; the abandoned ones never do"
    );
    assert!(
        registry
            .cached_image(&ImageSource::Asset(live_path.into()))
            .is_some(),
        "the live image is kept"
    );
}

/// With one decode thread a registry lets four loads past admission: while a
/// decode holds its load slot, only three more requests go out, and the next
/// one starts once that decode returns.
#[test]
fn loads_past_admission_are_bounded_by_count() {
    const BLOCKER: &[u8; 8] = b"FLUIBND0";
    const SETTLE: Duration = Duration::from_millis(300);
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    register_signed_png(BLOCKER, "flui-bound-blocker", move || hook_gate.pass());
    let http = ScriptedHttp::new();
    let urls: Vec<String> = (0..5)
        .map(|index| format!("https://example.test/{index}.img"))
        .collect();
    for url in &urls {
        http.hold(url.clone());
    }
    let requests = || http.requests().len();
    let files = SignedFiles::new("bound");
    let lanes = ThreadLanes::new();
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();

    let blocker = registry.load_image(files.write(BLOCKER, "blocker.img"), lanes.services());
    gate.wait_for_decodes(1);
    let held: Vec<_> = urls
        .iter()
        .map(|url| {
            registry.load_network_image(url.as_str(), lanes.services().with_http(http.handle()))
        })
        .collect();
    std::thread::sleep(SETTLE);
    assert_eq!(
        requests(),
        3,
        "the blocker and three requests hold the four load slots"
    );

    gate.open();
    let runtime = current_thread();
    runtime
        .block_on(async { tokio::time::timeout(BUDGET, blocker).await })
        .expect("the blocker completes")
        .expect("the blocker decodes");
    let deadline = Instant::now() + BUDGET;
    while requests() < 4 {
        assert!(
            Instant::now() < deadline,
            "the slot the blocker freed starts the next request"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    std::thread::sleep(SETTLE);
    assert_eq!(requests(), 4, "one freed slot, one request");
    drop(held);
}

/// With one decode thread, a decode slot goes to the earliest-admitted load
/// whose bytes are ready. With the decoder free, a file load decodes while a
/// network load admitted before it still waits for its response: the decoder
/// does not idle behind a slow fetch. With both waiting behind a running
/// decode, the earlier-admitted network load goes first, although the file's
/// bytes were ready long before the response arrived.
#[test]
fn decode_slots_go_to_the_earliest_admitted_ready_load() {
    const BLOCKER: &[u8; 8] = b"FLUIORD0";
    const NETWORK: &[u8; 8] = b"FLUIORDN";
    const FILE: &[u8; 8] = b"FLUIORDF";
    let gate = Arc::new(Gate::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    let hook_gate = Arc::clone(&gate);
    register_signed_png(BLOCKER, "flui-order-blocker", move || hook_gate.pass());
    for (signature, extension, name) in [
        (NETWORK, "flui-order-network", "network"),
        (FILE, "flui-order-file", "file"),
    ] {
        let order = Arc::clone(&order);
        register_signed_png(signature, extension, move || {
            order.lock().expect("order log").push(name);
            Ok(())
        });
    }
    let files = SignedFiles::new("order");
    let lanes = ThreadLanes::new();
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();
    let http = ScriptedHttp::new();
    let mut response = NETWORK.to_vec();
    response.extend_from_slice(PNG);
    let runtime = current_thread();
    let finish = |load: flui_assets::ImageLoad| {
        let image = runtime
            .block_on(async { tokio::time::timeout(BUDGET, load).await })
            .expect("the load completes")
            .expect("the image decodes");
        assert_eq!((image.width(), image.height()), (4, 2));
    };

    // A ready load does not wait for an earlier one still fetching.
    let slow = "https://example.test/slow.img";
    http.hold(slow);
    http.respond(slow, ScriptedResponse::ok(response.clone()));
    let fetching = registry.load_network_image(slow, lanes.services().with_http(http.handle()));
    arrive(&http, slow);
    finish(registry.load_image(files.write(FILE, "ready.img"), lanes.services()));
    assert_eq!(
        *order.lock().expect("order log"),
        ["file"],
        "the ready file decodes while the earlier network load still fetches"
    );
    http.release(slow);
    finish(fetching);
    order.lock().expect("order log").clear();

    // With both ready and waiting, the earlier admission wins.
    let held = "https://example.test/held.img";
    http.hold(held);
    http.respond(held, ScriptedResponse::ok(response));

    let blocker = registry.load_image(files.write(BLOCKER, "blocker.img"), lanes.services());
    gate.wait_for_decodes(1);
    let network = registry.load_network_image(held, lanes.services().with_http(http.handle()));
    arrive(&http, held);
    let file = registry.load_image(files.write(FILE, "file.img"), lanes.services());
    // The file's read finishes and it waits for the slot before the response
    // is even sent; then the network load's bytes arrive and it waits too.
    std::thread::sleep(Duration::from_millis(100));
    http.release(held);
    std::thread::sleep(Duration::from_millis(200));
    gate.open();

    for load in [blocker, network, file] {
        finish(load);
    }
    assert_eq!(
        *order.lock().expect("order log"),
        ["network", "file"],
        "the freed slot goes to the waiting load admitted first"
    );
}

/// Waits until `http` has seen the request for `url`.
fn arrive(http: &ScriptedHttp, url: &str) {
    let deadline = Instant::now() + BUDGET;
    while http.requests_to(url) == 0 {
        assert!(Instant::now() < deadline, "the request for {url} arrives");
        std::thread::sleep(Duration::from_millis(2));
    }
}

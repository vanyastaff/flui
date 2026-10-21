//! Where and when bridged image decodes run, observed through real decoder
//! hooks. A target of its own because image hook registrations are global and
//! cannot be removed.
//!
//! Each test registers a format whose files are an eight-byte signature
//! followed by the committed PNG fixture, so its hook sees exactly its own
//! decodes.

use std::ffi::OsString;
use std::io::{Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use flui_assets::{AssetRegistryBuilder, ImageSource};

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

/// A decode that waits for a task on the host runtime's only async worker
/// completes: the decode runs on the blocking pool, so the worker is free to
/// run that task. Decoding on the worker would wait for itself.
#[test]
fn bridged_decodes_leave_a_host_runtimes_async_workers_free() {
    const SIGNATURE: &[u8; 8] = b"FLUIWRK1";
    let host = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("host runtime starts");
    let worker = host.handle().clone();
    register_signed_png(SIGNATURE, "flui-worker-probe", move || {
        let (tx, rx) = std::sync::mpsc::channel();
        worker.spawn(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| std::io::Error::other("the host runtime's async worker is blocked"))
    });
    let files = SignedFiles::new("worker-probe");
    let path = files.write(SIGNATURE, "probe.img");
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_runtime_handle(host.handle().clone())
        .build();

    let image = host
        .block_on(async { tokio::time::timeout(BUDGET, registry.load_image_bridged(path)).await })
        .expect("the load completes")
        .expect("the decode reaches a free async worker");
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
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();

    let blocker = registry.load_image_bridged(files.write(SIGNATURE, "blocker.img"));
    gate.wait_for_decodes(1);
    let stale: Vec<_> = (0..5)
        .map(|index| {
            registry.load_image_bridged(files.write(SIGNATURE, &format!("stale-{index}.img")))
        })
        .collect();
    // Let the stale reads finish so they wait for the decode slot.
    std::thread::sleep(Duration::from_millis(100));
    drop(stale);
    let live_path = files.write(SIGNATURE, "live.img");
    let live = registry.load_image_bridged(live_path.clone());
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

/// Serves one request with `body`, but only once `release` fires; reports when
/// the request has arrived.
#[cfg(feature = "network")]
fn spawn_held_server(
    body: Vec<u8>,
) -> (
    std::net::SocketAddr,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback server binds");
    let address = listener.local_addr().expect("server has an address");
    let (arrived_tx, arrived) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the client connects");
        // Accepted sockets can inherit nonblocking mode on Windows.
        stream.set_nonblocking(false).expect("blocking reads");
        stream.set_read_timeout(Some(BUDGET)).expect("read timeout");
        let mut request = Vec::new();
        let mut byte = [0_u8];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("the request arrives");
            request.push(byte[0]);
        }
        let _ = arrived_tx.send(());
        release_rx
            .recv_timeout(BUDGET)
            .expect("the test releases the response");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("response header writes");
        stream.write_all(&body).expect("response body writes");
    });
    (address, arrived, release)
}

/// Accepts every connection, reads its request and never answers; counts the
/// requests that arrived. Runs until `stop` is set.
#[cfg(feature = "network")]
fn spawn_silent_server(
    requests: Arc<AtomicUsize>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> std::net::SocketAddr {
    use std::io::Read;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback server binds");
    let address = listener.local_addr().expect("server has an address");
    listener.set_nonblocking(true).expect("nonblocking accept");
    std::thread::spawn(move || {
        // Held, never read again: an answered or closed connection would let
        // its load finish and free its slot.
        let mut held_open = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking reads");
                    stream.set_read_timeout(Some(BUDGET)).expect("read timeout");
                    let mut request = Vec::new();
                    let mut byte = [0_u8];
                    while !request.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).is_err() {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    requests.fetch_add(1, Ordering::SeqCst);
                    held_open.push(stream);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
        drop(held_open);
    });
    address
}

/// With one decode thread a registry lets four loads past admission: while a
/// decode holds its load slot, only three more requests go out, and the next
/// one starts once that decode returns.
#[cfg(feature = "network")]
#[test]
fn loads_past_admission_are_bounded_by_count() {
    const BLOCKER: &[u8; 8] = b"FLUIBND0";
    const SETTLE: Duration = Duration::from_millis(300);
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    register_signed_png(BLOCKER, "flui-bound-blocker", move || hook_gate.pass());
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let address = spawn_silent_server(Arc::clone(&requests), Arc::clone(&stop));
    let files = SignedFiles::new("bound");
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();

    let blocker = registry.load_image_bridged(files.write(BLOCKER, "blocker.img"));
    gate.wait_for_decodes(1);
    let held: Vec<_> = (0..5)
        .map(|index| registry.load_network_image_bridged(format!("http://{address}/{index}.img")))
        .collect();
    std::thread::sleep(SETTLE);
    assert_eq!(
        requests.load(Ordering::SeqCst),
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
    while requests.load(Ordering::SeqCst) < 4 {
        assert!(
            Instant::now() < deadline,
            "the slot the blocker freed starts the next request"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    std::thread::sleep(SETTLE);
    assert_eq!(
        requests.load(Ordering::SeqCst),
        4,
        "one freed slot, one request"
    );
    drop(held);
    stop.store(true, Ordering::SeqCst);
}

/// With one decode thread, a decode slot goes to the earliest-admitted load
/// whose bytes are ready. With the decoder free, a file load decodes while a
/// network load admitted before it still waits for its response: the decoder
/// does not idle behind a slow fetch. With both waiting behind a running
/// decode, the earlier-admitted network load goes first, although the file's
/// bytes were ready long before the response arrived.
#[cfg(feature = "network")]
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
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();
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
    let (address, arrived, release) = spawn_held_server(response.clone());
    let fetching = registry.load_network_image_bridged(format!("http://{address}/slow.img"));
    arrived.recv_timeout(BUDGET).expect("the request arrives");
    finish(registry.load_image_bridged(files.write(FILE, "ready.img")));
    assert_eq!(
        *order.lock().expect("order log"),
        ["file"],
        "the ready file decodes while the earlier network load still fetches"
    );
    release.send(()).expect("the server waits for the release");
    finish(fetching);
    order.lock().expect("order log").clear();

    // With both ready and waiting, the earlier admission wins.
    let (address, arrived, release) = spawn_held_server(response);

    let blocker = registry.load_image_bridged(files.write(BLOCKER, "blocker.img"));
    gate.wait_for_decodes(1);
    let network = registry.load_network_image_bridged(format!("http://{address}/held.img"));
    arrived.recv_timeout(BUDGET).expect("the request arrives");
    let file = registry.load_image_bridged(files.write(FILE, "file.img"));
    // The file's read finishes and it waits for the slot before the response
    // is even sent; then the network load's bytes arrive and it waits too.
    std::thread::sleep(Duration::from_millis(100));
    release.send(()).expect("the server waits for the release");
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

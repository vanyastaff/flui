//! `NetworkImage` end-to-end tests (`network-images` feature): a hermetic
//! local HTTP server, no external network.
//!
//! Mirrors the hermetic-server pattern already established in
//! `flui-assets`' own `NetworkLoader` test
//! (`crates/flui-assets/src/loaders/network.rs`,
//! `load_url_round_trips_bytes_from_a_hermetic_local_server`): a
//! single-request, single-response HTTP/1.1 responder bound to an ephemeral
//! loopback port, serving real PNG fixture bytes as the response body.
#![cfg(feature = "network-images")]

#[path = "common/cases.rs"]
mod cases;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_assets::AssetRegistry;
use flui_widgets::{Image, ImageProvider, NetworkImage};

const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Absolute path to the committed 5x3 RGBA fixture PNG (shared with the
/// synchronous `Image::file` test in `tests/image.rs`).
fn fixture_bytes() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.png");
    std::fs::read(path).expect("the committed fixture PNG must be readable")
}

/// Accepts exactly one connection, discards the request, writes `body` as a
/// `200 OK` `image/png` response, then the listener thread exits. No mocking
/// library, no external network — a real socket on an ephemeral loopback
/// port.
fn spawn_single_response_server(body: Vec<u8>) -> SocketAddr {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("binding an ephemeral port must succeed");
    let addr = listener
        .local_addr()
        .expect("a bound listener must report its local address");

    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });

    addr
}

/// `Image::network` against a hermetic local server, on the background lanes
/// a native host has by default: the request runs on the tokio IO lane and
/// the decoded image lands in the registry. The headless host's own lanes
/// run IO on the test thread, outside tokio, where a network load is a typed
/// error instead (`docs/gaps.md`).
///
/// Red-check: run the load where it is polled instead of on the lanes (the
/// owner thread has no reactor, and the image never decodes).
#[test]
fn network_image_loads_on_a_native_hosts_default_lanes() {
    use flui_platform::capability::{CapabilityRegistrar, ProviderContext};
    use flui_platform::executor::Executor;
    use flui_runtime::capability::CapabilityRegistry;
    use flui_runtime::execution::ExecutionServices;
    use flui_testing::{HeadlessHost, HeadlessWindow};
    use flui_view::background::{Background, BackgroundCapability};

    let addr = spawn_single_response_server(fixture_bytes());
    let registry = Arc::new(AssetRegistry::default());
    let url = format!("http://{addr}/fixture.png");
    let services = Arc::new(ExecutionServices::with_defaults());
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<BackgroundCapability>(move |cx: &ProviderContext<'_>| {
        Ok(Background::new(
            Arc::clone(&services) as Arc<dyn Executor>,
            cx.completions().clone(),
        ))
    });
    let capabilities = CapabilityRegistry::builder()
        .overrides(registrar)
        .build()
        .expect("one provider");
    let mut host = HeadlessHost::with_capabilities(HeadlessWindow::new(100, 100), capabilities);
    host.attach(&Image::network(Arc::clone(&registry), url.clone()))
        .expect("root attaches");
    let source = flui_assets::ImageSource::Network(url.into());
    let deadline = Instant::now() + DECODE_BUDGET;
    while registry.cached_image(&source).is_none() {
        assert!(
            Instant::now() < deadline,
            "the network image did not decode within the {DECODE_BUDGET:?} budget"
        );
        let _ = host.pump(Duration::ZERO);
        std::thread::sleep(POLL_INTERVAL);
    }
    let image = registry.cached_image(&source).expect("the image is kept");
    assert_eq!((image.width(), image.height()), (5, 3));
}

#[test]
fn network_images_scope_cached_and_pending_responses_to_the_registry() {
    cases::run_cases(
        "network registry identity",
        &[
            ("cached responses", cached_responses),
            ("pending responses", pending_responses),
        ],
    );
}

fn cached_responses() {
    registry_responses(false);
}

fn pending_responses() {
    registry_responses(true);
}

fn registry_responses(concurrent: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime starts");
    let listener = TcpListener::bind("127.0.0.1:0").expect("test listener binds");
    listener
        .set_nonblocking(true)
        .expect("listener is nonblocking");
    let url = format!(
        "http://{}/identity.png",
        listener.local_addr().expect("bound address")
    );
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + DECODE_BUDGET;
        for body in [
            include_bytes!("fixtures/tiny.png").as_slice(),
            include_bytes!("fixtures/tiny_4x2.png").as_slice(),
        ] {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "both registries must fetch");
                        std::thread::sleep(POLL_INTERVAL);
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            stream
                .set_nonblocking(false)
                .expect("accepted socket is blocking");
            stream
                .set_read_timeout(Some(DECODE_BUDGET))
                .expect("read deadline");
            stream
                .set_write_timeout(Some(DECODE_BUDGET))
                .expect("write deadline");
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream
                    .read_exact(&mut byte)
                    .expect("request header arrives");
                header.push(byte[0]);
                assert!(header.len() <= 16 * 1024, "bounded request header");
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("response header writes");
            stream.write_all(body).expect("PNG writes");
        }
    });
    let first_registry = Arc::new(AssetRegistry::default());
    let second_registry = Arc::new(AssetRegistry::default());
    let owner = Arc::downgrade(&first_registry);
    let first = NetworkImage::new(Arc::clone(&first_registry), &url);
    let second = NetworkImage::new(second_registry, &url);
    let retained_key = first.cache_key().expect("network provider has a key");
    let mut dimensions = runtime.block_on(async {
        let load = async {
            let (a, b) = if concurrent {
                let (a, same_owner, b) = tokio::join!(
                    first.resolve_async(None),
                    first.clone().resolve_async(None),
                    second.resolve_async(None)
                );
                let original = a.as_ref().expect("first image decodes");
                let shared = same_owner.expect("same-owner image decodes");
                assert_eq!(
                    (original.width(), original.height()),
                    (shared.width(), shared.height()),
                    "same-registry subscribers must share the response"
                );
                (a, b)
            } else {
                let a = first.resolve_async(None).await;
                assert!(
                    second.resolve().is_err(),
                    "another registry must not inherit the first registry's cached response"
                );
                let b = second.resolve_async(None).await;
                (a, b)
            };
            [a, b].map(|image| {
                let image = image.expect("HTTP image decodes");
                (image.width(), image.height())
            })
        };
        tokio::time::timeout(DECODE_BUDGET, load)
            .await
            .expect("bounded decoding")
    });
    let first_dimensions = dimensions[0];
    dimensions.sort_unstable();
    assert_eq!(
        dimensions,
        [(4, 2), (5, 3)],
        "distinct registries must not share HTTP responses (concurrent={concurrent})"
    );
    let cached = first
        .resolve()
        .expect("same registry can use its decoded cache");
    assert_eq!(
        (cached.width(), cached.height()),
        first_dimensions,
        "another registry must not overwrite the first registry's cached response"
    );
    server.join().expect("both HTTP requests served");
    drop(first);
    drop(first_registry);
    assert!(
        owner.upgrade().is_none(),
        "retained cache keys must not own the registry"
    );
    drop(retained_key);
}

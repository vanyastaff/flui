//! `NetworkImage` end-to-end test (`network-images` feature): a hermetic
//! local HTTP server, no external network — placeholder → decoded.
//!
//! Mirrors the hermetic-server pattern already established in
//! `flui-assets`' own `NetworkLoader` test
//! (`crates/flui-assets/src/loaders/network.rs`,
//! `load_url_round_trips_bytes_from_a_hermetic_local_server`): a
//! single-request, single-response HTTP/1.1 responder bound to an ephemeral
//! loopback port, serving real PNG fixture bytes as the response body.
#![cfg(feature = "network-images")]

mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{lay_out, loose, size};
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

fn pump_until(laid: &mut common::LaidOut, mut check: impl FnMut(&mut common::LaidOut) -> bool) {
    let deadline = Instant::now() + DECODE_BUDGET;
    loop {
        laid.tick();
        if check(laid) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the network image did not decode within the {DECODE_BUDGET:?} budget -- \
             the hermetic server or the bridged fetch is stuck",
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// `Image::network` against a hermetic local server: placeholder on the
/// first frame (the fetch cannot complete synchronously), then the fixture's
/// true 5×3 dimensions once the response lands as a scheduled rebuild.
#[test]
fn network_image_placeholder_then_decodes_from_a_hermetic_local_server() {
    let addr = spawn_single_response_server(fixture_bytes());
    let registry = Arc::new(AssetRegistry::default());

    let mut laid = lay_out(
        Image::network(registry, format!("http://{addr}/fixture.png")),
        loose(1000.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "the first frame must show the empty-box placeholder while the \
         hermetic fetch is in flight",
    );

    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });
}

#[test]
fn network_images_scope_cached_and_pending_responses_to_the_registry() {
    common::cases::run_cases(
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
                    first.resolve_async(),
                    first.clone().resolve_async(),
                    second.resolve_async()
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
                let a = first.resolve_async().await;
                assert!(
                    second.resolve().is_err(),
                    "another registry must not inherit the first registry's cached response"
                );
                let b = second.resolve_async().await;
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

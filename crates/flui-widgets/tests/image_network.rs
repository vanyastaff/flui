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
use flui_widgets::Image;

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

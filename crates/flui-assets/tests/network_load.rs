//! Network image loads: HTTP pooling and recovery on the lanes a load is
//! given, and the typed error of a lane without the tokio reactor the HTTP
//! client needs.

use std::time::Duration;

use flui_assets::{AssetError, AssetRegistryBuilder};

use crate::lanes::{QueuedLanes, ThreadLanes, block_on};
use crate::network_support::{Response, spawn_server};

const PNG: &[u8] = include_bytes!("fixtures/tiny.png");

#[test]
fn network_loads_reuse_connections_and_recover_after_decode_errors() {
    crate::cases::run_cases(&[
        ("default lanes", round_trip),
        ("configured HTTP loader", configured_loader),
        ("independent registry pools", independent_registries),
        ("lanes replaced", lanes_replaced),
        ("no reactor", no_reactor),
    ]);
}

fn round_trip() {
    let (address, server) = spawn_server(
        vec![
            Response::ok(b"invalid image"),
            Response::ok(PNG),
            Response::ok(PNG),
        ],
        true,
    );
    let lanes = ThreadLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let url = format!("http://{address}/image.png");
    let result = block_on(registry.load_network_image(url.clone(), Some(lanes.lanes())));
    assert!(result.is_err(), "malformed image must fail decoding");
    // The failure is not kept, so the same URL is requested again; the next
    // URL is a distinct image, because a decoded one would be a cache hit.
    for next_url in [url, format!("http://{address}/next.png")] {
        let image = block_on(registry.load_network_image(next_url, Some(lanes.lanes())))
            .expect("next image load recovers");
        assert_eq!((image.width(), image.height()), (4, 2));
        assert_eq!(image.data().len(), 4 * 2 * 4);
    }
    assert_eq!(
        server.join().expect("server completed"),
        1,
        "one registry must reuse its HTTP connection after a decode error"
    );
}

fn configured_loader() {
    assert!(
        AssetRegistryBuilder::new()
            .with_network_client(reqwest::Client::builder().user_agent("invalid\nheader"))
            .is_err(),
        "invalid client configuration must return a construction error"
    );
    let (address, server) = spawn_server(
        vec![
            Response {
                status: "302 Found",
                headers: "Location: /actual.png\r\n",
                ..Response::ok(b"")
            },
            Response::ok(PNG),
        ],
        false,
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .no_proxy();
    let origin = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("configuration runtime starts");
    let registry = {
        let _entered = origin.enter();
        AssetRegistryBuilder::new()
            .with_network_client(client)
            .expect("configured HTTP client initializes")
            .with_default_capacity()
            .with_capacity(flui_assets::CacheCapacity::Entries(
                std::num::NonZeroU64::new(102).expect("nonzero test capacity"),
            ))
            .build()
    };
    // Configuration does not bind a connection driver to this undriven runtime.
    let lanes = ThreadLanes::new();
    let url = format!("http://{address}/image.png");
    assert!(
        block_on(registry.load_network_image(url.clone(), Some(lanes.lanes()))).is_err(),
        "registry must honor the injected redirect policy across capacity transitions"
    );
    let decoded = block_on(registry.load_network_image(url, Some(lanes.lanes())))
        .expect("next image recovers");
    assert_eq!((decoded.width(), decoded.height()), (4, 2));
    server.join().expect("server completed");
}

fn independent_registries() {
    let (address, server) = spawn_server((0..4).map(|_| Response::ok(PNG)).collect(), true);
    let lanes = ThreadLanes::new();
    for _ in 0..2 {
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        // Two distinct images, so the second is a request and not a cache hit.
        for name in ["first", "second"] {
            let decoded =
                block_on(registry.load_network_image(
                    format!("http://{address}/{name}.png"),
                    Some(lanes.lanes()),
                ))
                .expect("image loads");
            assert_eq!((decoded.width(), decoded.height()), (4, 2));
        }
        // Releasing the registry closes its idle connection; another registry
        // must establish its own pool even for an equal URL.
        drop(registry);
    }
    assert_eq!(
        server.join().expect("server completed"),
        2,
        "independent registries must own separate HTTP pools"
    );
}

/// Each load runs on lanes of its own, retired after it: the retained client
/// reconnects on the next lanes.
fn lanes_replaced() {
    let (address, server) = spawn_server(vec![Response::ok(PNG), Response::ok(PNG)], true);
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    for name in ["first", "second"] {
        let lanes = ThreadLanes::new();
        let decoded = block_on(
            registry
                .load_network_image(format!("http://{address}/{name}.png"), Some(lanes.lanes())),
        )
        .expect("live lanes load");
        assert_eq!((decoded.width(), decoded.height()), (4, 2));
    }
    assert_eq!(
        server.join().expect("server completed"),
        2,
        "retired lanes retire their connection; the retained client can reconnect"
    );
}

/// With no lanes, or lanes without a tokio reactor, a network load is a
/// typed error, not a panic inside the HTTP client.
///
/// Red-check: drop the reactor check (reqwest panics on the polling thread).
fn no_reactor() {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let url = "http://127.0.0.1:9/unreached.png";
    let without_lanes = block_on(registry.load_network_image(url, None));
    assert!(
        matches!(&without_lanes, Err(AssetError::ReactorUnavailable { url: failed }) if failed == url),
        "a load polled outside tokio is a typed error, got {without_lanes:?}"
    );
    let queued = QueuedLanes::new();
    let pending = registry.load_network_image(url, Some(queued.lanes()));
    queued.run_until_idle();
    assert!(
        matches!(
            block_on(pending),
            Err(AssetError::ReactorUnavailable { .. })
        ),
        "an IO lane outside tokio is a typed error"
    );
}

/// What the stalled server saw on its one connection.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    /// The request arrived; the server never answers it.
    Request,
    /// The client kept the connection open for another read timeout.
    Open,
    /// The client closed the connection; carries whether a second connection
    /// was ever made.
    Closed { second_connection: bool },
}

/// Accepts one connection, reads its request and never answers, reporting
/// whether the client keeps the connection open.
fn spawn_stalled_server() -> (std::net::SocketAddr, std::sync::mpsc::Receiver<Seen>) {
    use std::io::Read;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback server binds");
    let address = listener.local_addr().expect("server has an address");
    let (seen, events) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the client connects");
        // Accepted sockets can inherit nonblocking mode on Windows.
        stream
            .set_nonblocking(false)
            .expect("blocking request reads");
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("read timeout");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            assert!(std::time::Instant::now() < deadline, "no request arrived");
            if let Ok(read) = stream.read(&mut buffer) {
                assert!(read > 0, "the client closed before sending its request");
                request.extend_from_slice(&buffer[..read]);
            }
        }
        let _ = seen.send(Seen::Request);
        while std::time::Instant::now() < deadline {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    listener.set_nonblocking(true).expect("nonblocking accept");
                    let second_connection = listener.accept().is_ok();
                    let _ = seen.send(Seen::Closed { second_connection });
                    return;
                }
                Ok(_) => {}
                Err(_) => {
                    if seen.send(Seen::Open).is_err() {
                        return;
                    }
                }
            }
        }
    });
    (address, events)
}

/// Two waiters for one URL share one request. It stays open while either
/// waits, and dropping the last one cancels it and closes its connection.
#[test]
fn an_image_request_lives_exactly_as_long_as_someone_waits_for_it() {
    const WAIT: Duration = Duration::from_secs(5);
    let (address, events) = spawn_stalled_server();
    let lanes = ThreadLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let url = format!("http://{address}/stalled.png");
    let first = registry.load_network_image(url.clone(), Some(lanes.lanes()));
    let second = registry.load_network_image(url, Some(lanes.lanes()));
    assert_eq!(events.recv_timeout(WAIT), Ok(Seen::Request));

    drop(first);
    assert_eq!(
        events.recv_timeout(WAIT),
        Ok(Seen::Open),
        "the request stays open while another waiter remains"
    );

    drop(second);
    let closed =
        std::iter::from_fn(|| events.recv_timeout(WAIT).ok()).find(|seen| *seen != Seen::Open);
    assert_eq!(
        closed,
        Some(Seen::Closed {
            second_connection: false
        }),
        "the last waiter's drop closes the one shared connection"
    );
}

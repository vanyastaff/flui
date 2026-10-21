//! HTTP pooling and recovery through the consumer-facing image bridge.

use std::time::Duration;

use flui_assets::AssetRegistryBuilder;

use crate::network_support::{Response, spawn_server};

const PNG: &[u8] = include_bytes!("fixtures/tiny.png");

#[test]
fn network_bridge_reuses_connections_and_recovers_after_decode_errors() {
    crate::cases::run_cases(&[
        ("owned runtime", owned_runtime),
        ("injected runtime", injected_runtime),
        ("configured HTTP loader", configured_loader),
        ("independent registry pools", independent_registries),
        ("ambient runtime replacement", ambient_runtime_replacement),
    ]);
}

fn owned_runtime() {
    round_trip(false);
}

fn injected_runtime() {
    round_trip(true);
}

fn round_trip(injected: bool) {
    let (address, server) = spawn_server(
        vec![
            Response::ok(b"invalid image"),
            Response::ok(PNG),
            Response::ok(PNG),
        ],
        true,
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test runtime starts");
    let builder = AssetRegistryBuilder::new().with_default_capacity();
    let registry = if injected {
        builder
            .with_runtime_handle(runtime.handle().clone())
            .build()
    } else {
        builder.build()
    };
    let url = format!("http://{address}/image.png");
    // Start each load outside Runtime::block_on so the owned-runtime row really
    // selects the registry's fallback. Only polling the result uses this runtime.
    let first = registry.load_network_image_bridged(url.clone());
    let result = runtime.block_on(tokio_timeout(first));
    assert!(result.is_err(), "malformed image must fail decoding");
    // The failure is not kept, so the same URL is requested again; the next
    // URL is a distinct image, because a decoded one would be a cache hit.
    for next_url in [url, format!("http://{address}/next.png")] {
        let next = registry.load_network_image_bridged(next_url);
        let image = runtime
            .block_on(tokio_timeout(next))
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

async fn tokio_timeout<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("bridged load completes within timeout")
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
    let runtime = test_runtime();
    let url = format!("http://{address}/image.png");
    let first = registry.load_network_image_bridged(url.clone());
    assert!(
        runtime.block_on(tokio_timeout(first)).is_err(),
        "registry must honor the injected redirect policy across capacity transitions"
    );
    let next = registry.load_network_image_bridged(url);
    let decoded = runtime
        .block_on(tokio_timeout(next))
        .expect("next image recovers");
    assert_eq!((decoded.width(), decoded.height()), (4, 2));
    server.join().expect("server completed");
}

fn independent_registries() {
    let (address, server) = spawn_server((0..4).map(|_| Response::ok(PNG)).collect(), true);
    let runtime = test_runtime();
    for _ in 0..2 {
        let registry = AssetRegistryBuilder::new()
            .with_runtime_handle(runtime.handle().clone())
            .with_default_capacity()
            .build();
        // Two distinct images, so the second is a request and not a cache hit.
        for name in ["first", "second"] {
            let image = registry.load_network_image_bridged(format!("http://{address}/{name}.png"));
            let decoded = runtime.block_on(tokio_timeout(image)).expect("image loads");
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

fn ambient_runtime_replacement() {
    let (address, server) = spawn_server(vec![Response::ok(PNG), Response::ok(PNG)], true);
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    for name in ["first", "second"] {
        let runtime = test_runtime();
        let image = {
            let _entered = runtime.enter();
            registry.load_network_image_bridged(format!("http://{address}/{name}.png"))
        };
        let decoded = runtime
            .block_on(tokio_timeout(image))
            .expect("live ambient runtime loads");
        assert_eq!((decoded.width(), decoded.height()), (4, 2));
        drop(runtime);
    }
    assert_eq!(
        server.join().expect("server completed"),
        2,
        "runtime shutdown retires its connection; the retained client can reconnect"
    );
}

fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test runtime starts")
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
/// waits, and dropping the last one aborts it and closes its connection.
#[test]
fn an_image_request_lives_exactly_as_long_as_someone_waits_for_it() {
    const WAIT: Duration = Duration::from_secs(5);
    let (address, events) = spawn_stalled_server();
    let runtime = test_runtime();
    let registry = AssetRegistryBuilder::new()
        .with_runtime_handle(runtime.handle().clone())
        .with_default_capacity()
        .build();
    let url = format!("http://{address}/stalled.png");
    let first = registry.load_network_image_bridged(url.clone());
    let second = registry.load_network_image_bridged(url);
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

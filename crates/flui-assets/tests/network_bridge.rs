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
    for _ in 0..2 {
        let next = registry.load_network_image_bridged(url.clone());
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
            .with_capacity(1024 * 1024)
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
        for _ in 0..2 {
            let image = registry.load_network_image_bridged(format!("http://{address}/image.png"));
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
    for _ in 0..2 {
        let runtime = test_runtime();
        let image = {
            let _entered = runtime.enter();
            registry.load_network_image_bridged(format!("http://{address}/image.png"))
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

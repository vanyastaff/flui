//! `NetworkImage` end-to-end tests (`asset-images` feature), fetched with a
//! scripted HTTP client: no test reaches the network.
#![cfg(feature = "asset-images")]

#[path = "common/cases.rs"]
mod cases;

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_assets::AssetRegistry;
use flui_testing::{HeadlessHost, HeadlessWindow, ScriptedHttp, ScriptedResponse};
use flui_widgets::{Image, ImageProvider, ImageServices, NetworkImage};

const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const URL: &str = "https://example.test/fixture.png";

/// The committed 5x3 RGBA fixture PNG (shared with the synchronous
/// `Image::file` test in `tests/image.rs`).
fn fixture_bytes() -> Vec<u8> {
    include_bytes!("fixtures/tiny.png").to_vec()
}

/// `Image::network` on the headless host: the widget acquires the host's
/// HTTP client, the request goes out from the deterministic lanes, which run
/// no async runtime, and the decoded image lands in the registry.
///
/// Red-check: leave `HttpCapability` out of what `Image` acquires (the load
/// answers `HttpUnavailable` and the image never decodes).
#[test]
fn network_image_loads_with_the_hosts_http_client() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(100, 100));
    host.http()
        .respond(URL, ScriptedResponse::ok(fixture_bytes()));
    let registry = Arc::new(AssetRegistry::default());
    host.attach(&Image::network(Arc::clone(&registry), URL))
        .expect("root attaches");
    let source = flui_assets::ImageSource::network(URL, &host.http().handle());
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
    assert_eq!(host.http().requests_to(URL), 1);
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

/// Two registries fetch the same URL with clients that answer different
/// images: a registry that served another's response would show the other
/// image and send no request of its own.
fn registry_responses(concurrent: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime starts");
    let url = URL;
    let first_http = ScriptedHttp::new();
    first_http.respond(url, ScriptedResponse::ok(fixture_bytes()));
    let second_http = ScriptedHttp::new();
    second_http.respond(
        url,
        ScriptedResponse::ok(include_bytes!("fixtures/tiny_4x2.png").to_vec()),
    );
    let first_services = ImageServices::new().with_http(first_http.handle());
    let second_services = ImageServices::new().with_http(second_http.handle());
    let first_registry = Arc::new(AssetRegistry::default());
    let second_registry = Arc::new(AssetRegistry::default());
    let owner = Arc::downgrade(&first_registry);
    let first = NetworkImage::new(Arc::clone(&first_registry), url);
    let second = NetworkImage::new(second_registry, url);
    let retained_key = first.cache_key().expect("network provider has a key");
    let mut dimensions = runtime.block_on(async {
        let load = async {
            let (a, b) = if concurrent {
                let (a, same_owner, b) = tokio::join!(
                    first.resolve_async(&first_services),
                    first.clone().resolve_async(&first_services),
                    second.resolve_async(&second_services)
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
                let a = first.resolve_async(&first_services).await;
                assert!(
                    second.cached(&second_services).is_none(),
                    "another registry must not inherit the first registry's cached response"
                );
                let b = second.resolve_async(&second_services).await;
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
        .cached(&first_services)
        .expect("same registry can use its decoded cache");
    assert_eq!(
        (cached.width(), cached.height()),
        first_dimensions,
        "another registry must not overwrite the first registry's cached response"
    );
    assert_eq!(first_http.requests_to(url), 1, "one request per registry");
    assert_eq!(second_http.requests_to(url), 1, "one request per registry");
    drop(first);
    drop(first_registry);
    assert!(
        owner.upgrade().is_none(),
        "retained cache keys must not own the registry"
    );
    drop(retained_key);
}

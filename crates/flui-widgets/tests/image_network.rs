//! `NetworkImage` end-to-end tests (`asset-images` feature), fetched with a
//! scripted HTTP client: no test reaches the network.
#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
#![cfg(feature = "asset-images")]

#[path = "common/cases.rs"]
mod cases;
#[path = "common/image_cache.rs"]
mod image_cache;

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_assets::ImageCache;
use flui_testing::host::{HeadlessHost, HeadlessWindow};
use flui_testing::host::{ScriptedHttp, ScriptedResponse};
use flui_widgets::{
    AssetImage, Image, ImageProvider, ImageProviderError, ImageServices, NetworkImage,
};

const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const URL: &str = "https://example.test/fixture.png";

/// The committed 5x3 RGBA fixture PNG (shared with the synchronous
/// `Image::file` test in `tests/image.rs`).
fn fixture_bytes() -> Vec<u8> {
    include_bytes!("fixtures/tiny.png").to_vec()
}

/// `Image::network` on the headless host: the widget acquires the host's
/// HTTP client and its UI runtime's image cache, the request goes out from
/// the deterministic lanes, which run no async runtime, and the decoded image
/// lands in that cache.
///
/// Red-check: leave `HttpCapability` or `ImageCacheCapability` out of what
/// `Image` acquires (the load answers `HttpUnavailable` or
/// `ImageCacheUnavailable` and the image never decodes).
#[test]
fn network_image_loads_with_the_hosts_http_client() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(100, 100));
    host.http()
        .respond(URL, ScriptedResponse::ok(fixture_bytes()));
    let cache = image_cache::runtime_image_cache(&mut host);
    host.attach(&Image::network(URL)).expect("root attaches");
    let source = flui_assets::ImageSource::network(URL, &host.http().handle());
    let deadline = Instant::now() + DECODE_BUDGET;
    while cache.get(&source).is_none() {
        assert!(
            Instant::now() < deadline,
            "the network image did not decode within the {DECODE_BUDGET:?} budget"
        );
        let _ = host.pump(Duration::ZERO);
        std::thread::sleep(POLL_INTERVAL);
    }
    let image = cache.get(&source).expect("the image is kept");
    assert_eq!((image.width(), image.height()), (5, 3));
    assert_eq!(host.http().requests_to(URL), 1);
}

#[test]
fn network_images_scope_cached_and_pending_responses_to_their_cache() {
    cases::run_cases(
        "network cache identity",
        &[
            ("cached responses", cached_responses),
            ("pending responses", pending_responses),
        ],
    );
}

fn cached_responses() {
    cache_responses(false);
}

fn pending_responses() {
    cache_responses(true);
}

/// Two caches fetch the same URL with clients that answer different images:
/// a cache that served another's response would show the other image and
/// send no request of its own.
fn cache_responses(concurrent: bool) {
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
    let first_services = ImageServices::new()
        .with_http(first_http.handle())
        .with_image_cache(Arc::new(ImageCache::default()));
    let second_services = ImageServices::new()
        .with_http(second_http.handle())
        .with_image_cache(Arc::new(ImageCache::default()));
    let first = NetworkImage::new(url);
    let second = NetworkImage::new(url);
    let mut dimensions = runtime.block_on(async {
        let load = async {
            let (a, b) = if concurrent {
                let (a, same_cache, b) = tokio::join!(
                    first.resolve_async(&first_services),
                    first.clone().resolve_async(&first_services),
                    second.resolve_async(&second_services)
                );
                let original = a.as_ref().expect("first image decodes");
                let shared = same_cache.expect("same-cache image decodes");
                assert_eq!(
                    (original.width(), original.height()),
                    (shared.width(), shared.height()),
                    "subscribers of one cache must share the response"
                );
                (a, b)
            } else {
                let a = first.resolve_async(&first_services).await;
                assert!(
                    second.cached(&second_services).is_none(),
                    "another cache must not inherit the first cache's response"
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
        "distinct caches must not share HTTP responses (concurrent={concurrent})"
    );
    let cached = first
        .cached(&first_services)
        .expect("the same cache keeps its decoded image");
    assert_eq!(
        (cached.width(), cached.height()),
        first_dimensions,
        "another cache must not overwrite the first cache's response"
    );
    assert_eq!(first_http.requests_to(url), 1, "one request per cache");
    assert_eq!(second_http.requests_to(url), 1, "one request per cache");
}

/// An image whose host lacks a service it needs answers which one, and
/// nothing is read or sent: a network image without an HTTP client answers
/// `HttpUnavailable`, and an asset or network image without an image cache
/// answers `ImageCacheUnavailable`.
///
/// The client check itself is a type: a network source cannot be built
/// without its client, so this pins the answer, not the check.
#[test]
fn an_image_missing_a_service_answers_which_one() {
    let http = ScriptedHttp::new();
    let network = NetworkImage::new(URL);
    let cache_only = ImageServices::new().with_image_cache(Arc::new(ImageCache::default()));
    let answered = block_on(network.resolve_async(&cache_only));
    assert!(
        matches!(&answered, Err(ImageProviderError::HttpUnavailable { url }) if url == URL),
        "{answered:?}"
    );
    assert!(network.cached(&cache_only).is_none());

    let client_only = ImageServices::new().with_http(http.handle());
    let answered = block_on(network.resolve_async(&client_only));
    assert!(
        matches!(&answered, Err(ImageProviderError::ImageCacheUnavailable { image }) if image == URL),
        "{answered:?}"
    );
    assert!(network.cached(&client_only).is_none());
    assert_eq!(http.requests_to(URL), 0, "nothing is sent");

    let asset = AssetImage::new("logo.png");
    let answered = block_on(asset.resolve_async(&ImageServices::new()));
    assert!(
        matches!(&answered, Err(ImageProviderError::ImageCacheUnavailable { image }) if image == "logo.png"),
        "{answered:?}"
    );
    assert!(asset.cached(&ImageServices::new()).is_none());
}

/// An asset image whose host lacks the bundle, or names a key no bundle can
/// hold, answers which, and nothing is read: a cache without a bundle answers
/// `AssetBundleUnavailable` with the key, and a key that is not a relative
/// `/`-separated path answers `InvalidAssetKey`.
///
/// Red-check: answer the missing bundle as `NotFound`, or look for the
/// bundle before the key (the bad key answers `AssetBundleUnavailable`).
#[test]
fn an_asset_image_without_a_bundle_or_with_a_bad_key_answers_which() {
    let no_bundle = ImageServices::new().with_image_cache(Arc::new(ImageCache::default()));
    let asset = AssetImage::new("logo.png");
    let answered = block_on(asset.resolve_async(&no_bundle));
    assert!(
        matches!(&answered, Err(ImageProviderError::AssetBundleUnavailable { key }) if key == "logo.png"),
        "{answered:?}"
    );
    assert!(asset.cached(&no_bundle).is_none());

    let bundle = flui_platform::assets::AssetBundle::new(Arc::new(
        flui_platform::assets::DirectoryAssets::new(std::env::temp_dir()),
    ));
    let with_bundle = no_bundle.clone().with_asset_bundle(bundle);
    let escaping = AssetImage::new("../logo.png");
    let answered = block_on(escaping.resolve_async(&with_bundle));
    assert!(
        matches!(&answered, Err(ImageProviderError::InvalidAssetKey { key, .. }) if key == "../logo.png"),
        "{answered:?}"
    );
    assert!(escaping.cached(&with_bundle).is_none());
    let answered = block_on(escaping.resolve_async(&no_bundle));
    assert!(
        matches!(&answered, Err(ImageProviderError::InvalidAssetKey { key, .. }) if key == "../logo.png"),
        "a bad key is named before a missing bundle: {answered:?}"
    );
}

/// Polls `future` on this thread until it is ready.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("BUG: a current-thread runtime builds")
        .block_on(future)
}

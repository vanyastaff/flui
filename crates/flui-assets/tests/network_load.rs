//! Network image loads: fetched with the caller's HTTP client, on the lanes a
//! load is given or on none, and the typed errors of a failed fetch.

use std::time::{Duration, Instant};

use flui_assets::{AssetError, AssetRegistryBuilder, LoadServices};
use flui_platform::http::{HttpError, StatusCode};
use flui_platform::testing::{ScriptedHttp, ScriptedResponse};

use crate::lanes::{QueuedLanes, ThreadLanes, block_on};

const PNG: &[u8] = include_bytes!("fixtures/tiny.png");
const URL: &str = "https://example.test/image.png";

#[test]
fn network_loads_fetch_with_the_callers_client_and_recover_after_errors() {
    crate::cases::run_cases(&[
        ("decode error, then recovery", decode_error_then_recovery),
        ("no lanes, no runtime", no_lanes),
        ("error status", error_status),
        ("failed request", failed_request),
        ("no client", no_client),
        ("handed over to the next client", handed_over),
    ]);
}

/// A failed decode is not kept, so the same URL is fetched again; each
/// request carries the registry's body limit.
fn decode_error_then_recovery() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::ok(b"invalid image".to_vec()));
    let lanes = ThreadLanes::new();
    let services = lanes.services().with_http(http.handle());
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let malformed = block_on(registry.load_network_image(URL, services.clone()));
    assert!(
        matches!(
            &malformed,
            Err(AssetError::LoadFailed { .. } | AssetError::DecodeFailed { .. })
        ),
        "a malformed image fails decoding, got {malformed:?}"
    );
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let image = block_on(registry.load_network_image(URL, services.clone()))
        .expect("the next load recovers");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(http.requests_to(URL), 2, "the failure was not kept");
    let _cached = block_on(registry.load_network_image(URL, services)).expect("a cache hit");
    assert_eq!(
        http.requests_to(URL),
        2,
        "a decoded image is served without HTTP"
    );
    let limit = http.requests()[0].body_limit();
    assert_eq!(limit, 256 * 1024 * 1024, "the registry bounds the body");
}

/// With no lanes the load runs where it is polled, and the client needs no
/// async runtime there: a headless host loads network images.
///
/// Red-check: put back a Tokio reactor check before the request.
fn no_lanes() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let image =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())))
            .expect("the image loads with no lanes and no runtime");
    assert_eq!((image.width(), image.height()), (4, 2));
}

fn error_status() {
    let http = ScriptedHttp::new();
    http.respond(
        URL,
        ScriptedResponse::Respond {
            status: StatusCode::NOT_FOUND,
            body: b"not here".to_vec(),
        },
    );
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let answered =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())));
    assert!(
        matches!(&answered, Err(AssetError::HttpStatus { status, .. }) if *status == StatusCode::NOT_FOUND),
        "{answered:?}"
    );
}

fn failed_request() {
    let http = ScriptedHttp::new();
    http.respond(URL, ScriptedResponse::Fail(HttpError::Timeout));
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let answered =
        block_on(registry.load_network_image(URL, LoadServices::new().with_http(http.handle())));
    assert!(
        matches!(
            &answered,
            Err(AssetError::Http {
                source: HttpError::Timeout,
                ..
            })
        ),
        "{answered:?}"
    );
}

/// A host with no HTTP client answers a typed error and sends nothing.
fn no_client() {
    let lanes = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let pending = registry.load_network_image(URL, lanes.services());
    lanes.run_until_idle();
    let answered = block_on(pending);
    assert!(
        matches!(&answered, Err(AssetError::HttpUnavailable { url }) if url == URL),
        "{answered:?}"
    );
}

/// The lead's lanes shut down under its held request; the request waiting on
/// other lanes takes the load over and fetches with its own client.
fn handed_over() {
    let first_http = ScriptedHttp::new();
    first_http.hold(URL);
    let second_http = ScriptedHttp::new();
    second_http.respond(URL, ScriptedResponse::ok(PNG.to_vec()));
    let retiring = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let first =
        registry.load_network_image(URL, retiring.services().with_http(first_http.handle()));
    retiring.run_until_idle();
    assert_eq!(
        first_http.requests_to(URL),
        1,
        "the lead's request is in flight"
    );
    let second =
        registry.load_network_image(URL, LoadServices::new().with_http(second_http.handle()));
    retiring.shut_down();
    assert_eq!(
        first_http.abandoned(),
        1,
        "the retired lanes dropped the request"
    );
    let image = block_on(second).expect("the waiter takes the load over");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(second_http.requests_to(URL), 1);
    drop(first);
}

/// Two waiters for one URL share one request. It stays in flight while
/// either waits, and dropping the last one abandons it.
#[test]
fn an_image_request_lives_exactly_as_long_as_someone_waits_for_it() {
    let http = ScriptedHttp::new();
    http.hold(URL);
    let lanes = ThreadLanes::new();
    let services = lanes.services().with_http(http.handle());
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let first = registry.load_network_image(URL, services.clone());
    let second = registry.load_network_image(URL, services);
    eventually(|| http.requests_to(URL) == 1, "the shared request goes out");

    drop(first);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(http.abandoned(), 0, "the request stays while another waits");
    assert_eq!(http.requests_to(URL), 1, "one request for both waiters");

    drop(second);
    eventually(
        || http.abandoned() == 1,
        "the last waiter's drop abandons it",
    );
}

/// Waits up to five seconds for `done`.
fn eventually(done: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

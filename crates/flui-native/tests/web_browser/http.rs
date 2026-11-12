//! The web's HTTP client: requests through the page's `fetch`, one against
//! the test server and the rest against a `fetch` the test stands in for, so
//! each answer, failure and abort is the test's to choose.

use flui_native::http::FetchClient;
use flui_platform::http::{HeaderName, HeaderValue, HttpClient, HttpError, HttpRequest, Method};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::wait;

#[wasm_bindgen(inline_js = r#"
const real = globalThis.fetch;
const seen = { calls: 0, aborted: 0, method: '', header: '', body: 0 };
function record(url, init) {
    seen.calls += 1;
    seen.method = init.method;
    seen.header = init.headers.get('x-flui') ?? '';
    seen.body = init.body ? init.body.length : 0;
    init.signal.addEventListener('abort', () => { seen.aborted += 1; });
}
// A body that hands out `chunks` chunks of `size` bytes, then ends unless
// `endless`.
function stream(chunks, size, endless) {
    let sent = 0;
    return new ReadableStream({
        pull(controller) {
            if (sent < chunks || endless) {
                sent += 1;
                controller.enqueue(new Uint8Array(size).fill(7));
            } else if (!endless) {
                controller.close();
            }
        },
    }, { highWaterMark: 0 });
}
// Stand in for the page's `fetch` with one answer:
// 'ok', 'declared', 'endless', 'silent' or 'refused'.
export function standIn(answer) {
    for (const key of Object.keys(seen)) seen[key] = typeof seen[key] === 'number' ? 0 : '';
    globalThis.fetch = (url, init) => {
        record(url, init);
        switch (answer) {
            case 'ok':
                return Promise.resolve(new Response(new Uint8Array([1, 2, 3]),
                    { status: 201, headers: { 'x-answer': 'yes' } }));
            case 'declared':
                return Promise.resolve(new Response(stream(1, 8, false),
                    { headers: { 'content-length': '1000' } }));
            case 'endless':
                return Promise.resolve(new Response(stream(0, 1024, true)));
            case 'silent':
                return new Promise(() => {});
            default:
                return Promise.reject(new TypeError('Failed to fetch'));
        }
    };
}
export function restore() { globalThis.fetch = real; }
export function calls() { return seen.calls; }
export function aborted() { return seen.aborted; }
export function method() { return seen.method; }
export function header() { return seen.header; }
export function bodyLength() { return seen.body; }
export function origin() { return location.origin + '/'; }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = standIn)]
    fn stand_in(answer: &str);
    fn restore();
    fn calls() -> u32;
    fn aborted() -> u32;
    fn method() -> String;
    fn header() -> String;
    #[wasm_bindgen(js_name = bodyLength)]
    fn body_length() -> u32;
    fn origin() -> String;
}

/// Puts the page's own `fetch` back when a test ends, however it ends.
struct StandIn;

impl StandIn {
    fn answering(answer: &str) -> Self {
        stand_in(answer);
        Self
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        restore();
    }
}

/// The table of what a request answers: a response with its status, headers
/// and body, the request as the page sent it, a URL that is not HTTP, a
/// declared body past the limit, a streamed one past it, and a failed
/// connection.
#[wasm_bindgen_test]
async fn a_request_through_fetch_answers_its_head_and_body() {
    let http = FetchClient::new();
    {
        let _page = StandIn::answering("ok");
        let request = HttpRequest::new(Method::Post, "https://example.test/upload")
            .header(
                HeaderName::try_new("X-Flui").expect("a token"),
                HeaderValue::try_new("sent").expect("a value"),
            )
            .body(vec![9_u8; 5]);
        let response = http.send(request).await.expect("the page answers");
        assert_eq!(response.status().as_u16(), 201);
        assert_eq!(
            response
                .headers()
                .get("x-answer")
                .and_then(HeaderValue::to_str),
            Some("yes")
        );
        assert_eq!(response.into_body().into_bytes().await, Ok(vec![1, 2, 3]));
        assert_eq!(
            (method(), header(), body_length()),
            ("POST".to_owned(), "sent".to_owned(), 5),
            "the page sent the request's method, headers and body"
        );
    }
    {
        let _page = StandIn::answering("ok");
        let not_http = http.send(HttpRequest::get("file:///etc/hosts")).await;
        assert_eq!(
            not_http.map(|_| ()),
            Err(HttpError::InvalidUrl {
                url: "file:///etc/hosts".to_owned()
            })
        );
        let relative = http.send(HttpRequest::get("/image.png")).await;
        assert!(
            matches!(relative, Err(HttpError::InvalidUrl { .. })),
            "a URL is absolute on every host: {relative:?}"
        );
        assert_eq!(calls(), 0, "nothing that is not HTTP reaches the page");
    }
    {
        let _page = StandIn::answering("declared");
        let declared = http
            .send(HttpRequest::get("https://example.test/large").body_limit(100))
            .await;
        assert_eq!(
            declared.map(|_| ()),
            Err(HttpError::TooLarge { limit: 100 })
        );
    }
    {
        let _page = StandIn::answering("endless");
        let response = http
            .send(HttpRequest::get("https://example.test/stream").body_limit(4096))
            .await
            .expect("the head arrives");
        assert_eq!(
            response.into_body().into_bytes().await,
            Err(HttpError::TooLarge { limit: 4096 })
        );
        wait(0).await;
        assert_eq!(aborted(), 1, "a body past its limit aborts the fetch");
    }
    {
        let _page = StandIn::answering("refused");
        let refused = http.send(HttpRequest::get("https://example.test/")).await;
        assert_eq!(
            refused.map(|_| ()),
            Err(HttpError::Connect {
                detail: "Failed to fetch".to_owned()
            })
        );
    }
}

/// Dropping a request before its head, or its body before the end, aborts
/// the page's fetch; a deadline aborts it too and answers `Timeout`, also
/// while the reader holds the body unread.
#[wasm_bindgen_test]
async fn a_request_left_or_past_its_deadline_aborts_the_fetch() {
    let http = FetchClient::new();
    {
        let _page = StandIn::answering("silent");
        let pending = http.send(HttpRequest::get("https://example.test/silent"));
        wait(0).await;
        assert_eq!((calls(), aborted()), (1, 0));
        drop(pending);
        wait(0).await;
        assert_eq!(aborted(), 1, "a request dropped before its head is aborted");
    }
    {
        let _page = StandIn::answering("endless");
        let response = http
            .send(HttpRequest::get("https://example.test/stream"))
            .await
            .expect("the head arrives");
        let mut body = response.into_body();
        assert!(matches!(body.next_chunk().await, Some(Ok(_))));
        drop(body);
        wait(0).await;
        assert_eq!(aborted(), 1, "a body dropped mid-stream aborts the fetch");
    }
    {
        let _page = StandIn::answering("silent");
        let silent = http
            .send(
                HttpRequest::get("https://example.test/silent")
                    .deadline(std::time::Duration::from_millis(20)),
            )
            .await;
        assert_eq!(silent.map(|_| ()), Err(HttpError::Timeout));
        assert_eq!(aborted(), 1, "the deadline aborts the fetch");
    }
    {
        let _page = StandIn::answering("endless");
        let held = http
            .send(
                HttpRequest::get("https://example.test/stream")
                    .deadline(std::time::Duration::from_millis(50)),
            )
            .await
            .expect("the head arrives within the deadline");
        wait(150).await;
        assert_eq!(
            aborted(),
            1,
            "a body held unread past its deadline aborts the fetch"
        );
        assert_eq!(
            held.into_body().into_bytes().await,
            Err(HttpError::Timeout),
            "the reader learns the deadline passed"
        );
    }
}

/// The page's own `fetch` answers a real request: the test server's page.
#[wasm_bindgen_test]
async fn the_pages_fetch_answers_from_the_test_server() {
    let response = FetchClient::new()
        .send(HttpRequest::get(origin()))
        .await
        .expect("the test server answers");
    assert!(response.status().is_success(), "{:?}", response.status());
    let body = response
        .into_body()
        .into_bytes()
        .await
        .expect("the page arrives");
    assert_ne!(body.len(), 0, "the page has a body");
}

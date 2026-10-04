//! Preserve exact asset bytes, strict text and configured HTTP policy.

use std::time::Duration;

use flui_assets::{AssetError, NetworkLoader};

use crate::network_support::{Response, spawn_server};

#[test]
fn network_loader_preserves_transport_and_text_contracts_after_failures() {
    crate::cases::run_cases(&[
        ("status rejection", status_rejection),
        ("strict UTF-8 and BOM", strict_utf8),
        ("truncated body", truncated_body),
        ("body deadline", body_deadline),
        ("redirect policy", redirects),
        ("invalid URL recovery", invalid_url),
    ]);
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime starts")
}

fn status_rejection() {
    runtime().block_on(async {
        for status in ["302 Found", "404 Not Found", "503 Service Unavailable"] {
            let (address, server) = spawn_server(
                vec![
                    Response {
                        status,
                        ..Response::ok(b"not an asset")
                    },
                    Response::ok(b"recovered"),
                ],
                false,
            );
            let loader = NetworkLoader::new().expect("HTTP client initializes");
            let url = format!("http://{address}/asset");
            let result = loader.load_url(&url).await;
            assert!(
                matches!(result, Err(AssetError::LoadFailed { ref path, .. }) if path == &url),
                "non-success status {status} must not become asset bytes"
            );
            assert_eq!(
                loader.load_url(&url).await.expect("next request succeeds"),
                b"recovered"
            );
            server.join().expect("server completed");
        }
    });
}

fn strict_utf8() {
    runtime().block_on(async {
        let (address, server) = spawn_server(
            vec![
                Response::ok(b"\xff"),
                Response {
                    headers: "Content-Type: text/plain; charset=iso-8859-1\r\n",
                    ..Response::ok(b"\xef\xbb\xbfhello")
                },
            ],
            false,
        );
        let loader = NetworkLoader::new().expect("HTTP client initializes");
        let url = format!("http://{address}/text");
        assert!(
            loader.load_text(&url).await.is_err(),
            "malformed UTF-8 must not be replaced"
        );
        assert_eq!(
            loader.load_text(&url).await.expect("valid UTF-8 recovers"),
            "\u{feff}hello"
        );
        server.join().expect("server completed");
    });
}

fn truncated_body() {
    failed_body_recovers(
        Response {
            length: Some(10),
            ..Response::ok(b"short")
        },
        false,
    );
}

fn body_deadline() {
    failed_body_recovers(
        Response {
            delay: Duration::from_millis(600),
            ..Response::ok(b"late")
        },
        true,
    );
}

fn failed_body_recovers(first: Response, timed: bool) {
    runtime().block_on(async {
        let (address, server) = spawn_server(vec![first, Response::ok(b"recovered")], false);
        let mut client = reqwest::Client::builder().no_proxy();
        if timed {
            client = client.timeout(Duration::from_millis(200));
        }
        let loader = NetworkLoader::with_client(client.build().expect("HTTP client initializes"));
        let url = format!("http://{address}/asset");
        assert!(
            loader.load_url(&url).await.is_err(),
            "incomplete or overdue body must fail"
        );
        if timed {
            tokio::time::sleep(Duration::from_millis(650)).await;
        }
        assert_eq!(
            loader.load_url(&url).await.expect("next body succeeds"),
            b"recovered"
        );
        server.join().expect("server completed");
    });
}

fn redirects() {
    runtime().block_on(async {
        for follow in [true, false] {
            let responses = vec![
                Response {
                    status: "302 Found",
                    headers: "Location: /actual\r\n",
                    ..Response::ok(b"")
                },
                Response::ok(b"actual bytes"),
            ];
            let (address, server) = spawn_server(responses, false);
            let client = if follow {
                reqwest::Client::builder()
            } else {
                reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
            }
            .no_proxy()
            .build()
            .expect("HTTP client initializes");
            let loader = NetworkLoader::with_client(client);
            let url = format!("http://{address}/redirect");
            if follow {
                assert_eq!(
                    loader.load_url(&url).await.expect("redirect follows"),
                    b"actual bytes"
                );
            } else {
                assert!(
                    loader.load_url(&url).await.is_err(),
                    "unfollowed redirect must fail"
                );
                assert_eq!(
                    loader.load_url(&url).await.expect("next request recovers"),
                    b"actual bytes"
                );
            }
            server.join().expect("server completed");
        }
    });
}

fn invalid_url() {
    runtime().block_on(async {
        let loader = NetworkLoader::new().expect("HTTP client initializes");
        assert!(loader.load_url("not a URL").await.is_err());
        let (address, server) = spawn_server(vec![Response::ok(b"recovered")], false);
        assert_eq!(
            loader
                .load_url(&format!("http://{address}/asset"))
                .await
                .expect("valid URL recovers"),
            b"recovered"
        );
        server.join().expect("server completed");
    });
}

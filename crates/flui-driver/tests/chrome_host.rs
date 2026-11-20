//! What the Chrome lane runs on the host beside the browser: the page's server and the
//! ChromeDriver it takes, kept or fetched, reached through `testing::chrome` (ADR-0243). Every
//! fetch goes to stand-in hosts on the loopback: no test reaches the network.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use flui_driver::Error;
use flui_driver::testing::chrome::{
    CONNECTION_DEADLINE, Chrome, MAX_HEAD, PageServer, archive_url, chrome_args, executable_name,
    is_version, newest_version, page_file, place_without_links, platform, reported_port,
    resolve_chromedriver, verify_kept,
};

/// The address the server bound, as a `Host` names it.
fn authority(server: &PageServer) -> String {
    server
        .url()
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned()
}

/// The status line and body of `request` sent to `server`.
fn http(server: &PageServer, request: &[u8]) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(authority(server)).expect("the server accepts");
    stream.write_all(request).expect("the request goes out");
    let mut answer = Vec::new();
    stream
        .read_to_end(&mut answer)
        .expect("the answer comes back");
    let text = String::from_utf8_lossy(&answer).into_owned();
    let status = text.lines().next().unwrap_or("").to_owned();
    let body = text
        .split_once("\r\n\r\n")
        .map_or_else(Vec::new, |(_, body)| body.as_bytes().to_vec());
    (status, body)
}

/// `method target` with the server's own `Host`.
fn get(server: &PageServer, method: &str, target: &str) -> (String, Vec<u8>) {
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: {}\r\n\r\n",
        authority(server)
    );
    http(server, request.as_bytes())
}

fn site() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path().join("site");
    std::fs::create_dir_all(root.join("pkg")).expect("the build's directories");
    std::fs::write(root.join("index.html"), "<p>app</p>").expect("the page");
    std::fs::write(root.join("pkg").join("app_bg.wasm"), b"\0asm").expect("the module");
    std::fs::write(dir.path().join("secret.txt"), "outside").expect("a file outside");
    (dir, root)
}

#[test]
fn the_page_server_serves_its_root_and_nothing_outside_it() {
    let (_dir, root) = site();
    let server = PageServer::serve(&root).expect("the build is served");

    let (status, body) = get(&server, "GET", "/");
    assert_eq!(
        (status.as_str(), body.as_slice()),
        ("HTTP/1.1 200 OK", b"<p>app</p>".as_slice())
    );
    let (status, body) = get(&server, "GET", "/pkg/app_bg.wasm?v=1");
    assert_eq!(
        (status.as_str(), body.as_slice()),
        ("HTTP/1.1 200 OK", b"\0asm".as_slice())
    );
    let (status, body) = get(&server, "HEAD", "/index.html");
    assert_eq!((status.as_str(), body.len()), ("HTTP/1.1 200 OK", 0));
    for target in [
        "/../secret.txt",
        "/pkg/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/pkg\\..\\..\\secret.txt",
        "/missing.js",
        "//etc/passwd",
    ] {
        let (status, _) = get(&server, "GET", target);
        assert_eq!(status, "HTTP/1.1 404 Not Found", "{target}");
    }
    let (status, _) = get(&server, "POST", "/");
    assert_eq!(status, "HTTP/1.1 405 Method Not Allowed");
    let mut long = format!("GET / HTTP/1.1\r\nHost: {}\r\nX: ", authority(&server)).into_bytes();
    long.extend(std::iter::repeat_n(b'a', MAX_HEAD));
    long.extend(b"\r\n\r\n");
    let (status, _) = http(&server, &long);
    assert_eq!(status, "HTTP/1.1 431 Request Header Fields Too Large");

    let canonical = root.canonicalize().expect("the root");
    assert_eq!(
        page_file(&canonical, "/"),
        Some(canonical.join("index.html"))
    );
    assert_eq!(
        page_file(&canonical, "index.html"),
        None,
        "a target starts at the root"
    );
    assert_eq!(page_file(&canonical, "/pkg/./app_bg.wasm"), None);
    assert_eq!(
        page_file(&canonical, "/pkg"),
        None,
        "a directory is no file"
    );
}

/// A page of another origin whose name was rebound to the loopback names its own host: it reads
/// nothing, and neither does a request that names none.
#[test]
fn a_request_for_another_host_is_refused() {
    let (_dir, root) = site();
    let server = PageServer::serve(&root).expect("the build is served");
    let port = authority(&server)
        .rsplit_once(':')
        .map(|(_, port)| port.to_owned())
        .expect("a port");
    for host in [
        format!("Host: evil.example:{port}\r\n"),
        format!("Host: localhost:{port}\r\n"),
        "Host: 127.0.0.1:1\r\n".to_owned(),
        String::new(),
    ] {
        let request = format!("GET / HTTP/1.1\r\n{host}\r\n");
        let (status, body) = http(&server, request.as_bytes());
        assert_eq!(status, "HTTP/1.1 421 Misdirected Request", "{host:?}");
        assert!(!body.windows(3).any(|w| w == b"app"), "{host:?}");
    }
    let host_case = format!("GET / HTTP/1.1\r\nhOsT:  {} \r\n\r\n", authority(&server));
    assert_eq!(http(&server, host_case.as_bytes()).0, "HTTP/1.1 200 OK");
}

/// A client that asks for a large file and reads it a trickle at a time cannot hold the server:
/// the connection ends at its deadline, so dropping the server returns.
#[test]
fn a_slow_reader_cannot_hold_the_server_past_its_deadline() {
    let (_dir, root) = site();
    std::fs::write(root.join("large.bin"), vec![7_u8; 64 << 20]).expect("a large file");
    let server = PageServer::serve(&root).expect("the build is served");
    let mut stream = TcpStream::connect(authority(&server)).expect("the server accepts");
    let request = format!(
        "GET /large.bin HTTP/1.1\r\nHost: {}\r\n\r\n",
        authority(&server)
    );
    stream
        .write_all(request.as_bytes())
        .expect("the request goes out");
    let reader = std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        let started = Instant::now();
        // A little at a time, enough that each of the server's writes makes progress.
        while started.elapsed() < Duration::from_secs(60) {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    drop(server);
    assert!(
        started.elapsed() < CONNECTION_DEADLINE + Duration::from_secs(3),
        "the drop waited {:?}",
        started.elapsed()
    );
    drop(reader);
}

#[test]
fn chromedriver_versions_ports_archives_and_arguments() {
    assert!(is_version("157.0.8103.79"));
    for not in [
        "157.0.8103",
        "Google",
        "157.0..79",
        "157.0.8103.79.1",
        "1.2.3.x",
        "1234567890.0.0.0",
        "1a.2.3.4",
    ] {
        assert!(!is_version(not), "{not}");
    }
    let names = [
        "99.0.1.2",
        "157.0.8103.79",
        "SetupMetrics",
        "157.0.8103.110",
    ]
    .map(str::to_owned);
    assert_eq!(newest_version(&names).as_deref(), Some("157.0.8103.110"));
    assert_eq!(
        reported_port("ChromeDriver was started successfully on port 51234.\n"),
        Some(51234)
    );
    assert_eq!(reported_port("Starting ChromeDriver 157.0.8103.79"), None);
    assert_eq!(
        reported_port("ChromeDriver was started successfully on port 0."),
        None,
        "port 0 is no port a client reaches"
    );
    assert_eq!(
        archive_url("https://host/root", "157.0.8103.79", "win64"),
        "https://host/root/157.0.8103.79/win64/chromedriver-win64.zip"
    );
    let args = chrome_args(800, 600);
    assert_eq!(
        args.first().map(String::as_str),
        Some("--headless=new"),
        "always headless"
    );
    assert!(args.contains(&"--window-size=800,600".to_owned()));
    assert!(args.contains(&"--force-device-scale-factor=1".to_owned()));
}

/// A ChromeDriver archive holding `name` with `contents`.
fn archive(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("an entry");
        zip.write_all(contents).expect("its contents");
        zip.finish().expect("the archive");
    }
    bytes.into_inner()
}

/// A stand-in for the Chrome for Testing hosts on a loopback port, answering each path from
/// `files` and 404 otherwise: its root, and the count of requests served.
fn archives(files: Vec<(String, Vec<u8>)>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let root = format!("http://{}", listener.local_addr().expect("its address"));
    let served = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let files = files.clone();
            let counted = Arc::clone(&counted);
            // Each request on a thread of its own, so parallel fetches race.
            std::thread::spawn(move || {
                let mut head = Vec::new();
                let mut byte = [0_u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                counted.fetch_add(1, Ordering::SeqCst);
                let path = String::from_utf8_lossy(&head)
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                let answer = match files.iter().find(|(name, _)| *name == path) {
                    Some((_, body)) => {
                        let mut answer = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        answer.extend(body);
                        answer
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&answer);
            });
        }
    });
    (root, served)
}

fn chrome(version: &str) -> Chrome {
    Chrome {
        binary: "chrome".into(),
        version: version.to_owned(),
    }
}

/// The stand-in's archive of ChromeDriver `version` for this host, holding `binary`.
fn published(version: &str, binary: &[u8]) -> (String, Vec<u8>) {
    let platform = platform().expect("Chrome for Testing publishes for this host");
    (
        format!("/archives/{version}/{platform}/chromedriver-{platform}.zip"),
        archive(
            &format!("chromedriver-{platform}/{}", executable_name()),
            binary,
        ),
    )
}

#[test]
fn a_chromedriver_is_fetched_with_its_hashes_and_an_unpublished_patch_takes_its_build() {
    let (root, _) = archives(vec![
        published("157.0.8103.79", b"driver 79"),
        (
            "/latest/LATEST_RELEASE_157.0.8103".to_owned(),
            b"157.0.8103.79\n".to_vec(),
        ),
        (
            "/latest/LATEST_RELEASE_157.0.8104".to_owned(),
            b"157.0.8103.79\n".to_vec(),
        ),
        (
            format!(
                "/archives/158.0.1.1/{}/chromedriver-{}.zip",
                platform().expect("a platform"),
                platform().expect("a platform")
            ),
            archive("chromedriver/LICENSE", b"no driver here"),
        ),
    ]);
    let cache = tempfile::tempdir().expect("a temporary directory");
    let kept = resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, Some(&root))
        .expect("a published patch");
    assert_eq!(std::fs::read(&kept).expect("the executable"), b"driver 79");
    let record = std::fs::read_to_string(
        cache
            .path()
            .join("157.0.8103.79")
            .join("chromedriver.sha256"),
    )
    .expect("the hashes are recorded beside it");
    assert!(
        record.starts_with("version 157.0.8103.79\nbinary "),
        "{record}"
    );
    verify_kept(&cache.path().join("157.0.8103.79")).expect("the kept one matches its record");
    let fallback = resolve_chromedriver(&chrome("157.0.8103.80"), cache.path(), None, Some(&root))
        .expect("an unpublished patch takes the newest of its build");
    assert_eq!(
        std::fs::read(&fallback).expect("the executable"),
        b"driver 79"
    );
    let fallback_record = std::fs::read_to_string(fallback.with_file_name("chromedriver.sha256"))
        .expect("its record");
    assert!(
        fallback_record.starts_with("version 157.0.8103.79\n"),
        "the record names the version fetched, kept under the installed one: {fallback_record}"
    );
    let other_build =
        resolve_chromedriver(&chrome("157.0.8104.5"), cache.path(), None, Some(&root));
    assert!(
        matches!(&other_build, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("is named")),
        "a newest release of another build is refused: {other_build:?}"
    );
    let staged: Vec<String> = std::fs::read_dir(fallback.parent().expect("its directory"))
        .expect("the cache")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".partial"))
        .collect();
    assert!(staged.is_empty(), "nothing staged is left: {staged:?}");
    let refused = resolve_chromedriver(&chrome("158.0.1.1"), cache.path(), None, Some(&root));
    assert!(
        matches!(&refused, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("holds no")),
        "{refused:?}"
    );
    let missing = resolve_chromedriver(&chrome("159.0.0.1"), cache.path(), None, Some(&root));
    assert!(
        matches!(
            &missing,
            Err(Error::Lane {
                unavailable: true,
                ..
            })
        ),
        "{missing:?}"
    );
    let off_loopback = resolve_chromedriver(
        &chrome("160.0.0.1"),
        cache.path(),
        None,
        Some("http://192.0.2.1:80"),
    );
    assert!(
        matches!(off_loopback, Err(Error::InvalidArgument { .. })),
        "plain HTTP reaches the loopback stand-in alone: {off_loopback:?}"
    );
}

/// Without a fetch allowed, a host with no ChromeDriver is one that cannot run the lane, and
/// nothing is fetched.
#[test]
fn without_a_fetch_allowed_a_missing_chromedriver_is_unavailable() {
    let (_root, served) = archives(vec![published("157.0.8103.79", b"driver 79")]);
    let cache = tempfile::tempdir().expect("a temporary directory");
    let refused = resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, None);
    assert!(
        matches!(&refused, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("CHROMEDRIVER")),
        "{refused:?}"
    );
    assert!(refused.as_ref().is_err_and(Error::is_cannot_verify));
    assert_eq!(served.load(Ordering::SeqCst), 0, "nothing was fetched");
    // A ChromeDriver the launch names is taken as it is.
    let named = cache.path().join("named-chromedriver");
    assert_eq!(
        resolve_chromedriver(
            &chrome("157.0.8103.79"),
            cache.path(),
            Some(named.clone()),
            None
        )
        .expect("the named one"),
        named
    );
}

/// A kept ChromeDriver runs only while its hash is the one recorded: replaced, torn or without
/// its record, it is refused, and no fetch replaces it.
#[test]
fn a_kept_chromedriver_runs_only_while_its_hash_matches_its_record() {
    let (root, served) = archives(vec![published("157.0.8103.79", b"driver 79")]);
    let cache = tempfile::tempdir().expect("a temporary directory");
    let kept = resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, Some(&root))
        .expect("fetched once");
    let fetched = served.load(Ordering::SeqCst);
    assert_eq!(
        resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, None)
            .expect("the kept one, no fetch allowed"),
        kept
    );
    assert_eq!(
        served.load(Ordering::SeqCst),
        fetched,
        "taken without a fetch"
    );

    std::fs::write(&kept, b"driver 79 with a payload").expect("a tampered binary");
    for archives in [None, Some(root.as_str())] {
        let refused = resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, archives);
        assert!(
            matches!(&refused, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("SHA-256")),
            "{refused:?}"
        );
    }
    std::fs::remove_file(kept.with_file_name("chromedriver.sha256")).expect("the record gone");
    std::fs::write(&kept, b"driver 79").expect("the original bytes");
    let unrecorded = resolve_chromedriver(&chrome("157.0.8103.79"), cache.path(), None, None);
    assert!(
        matches!(&unrecorded, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("record")),
        "{unrecorded:?}"
    );
}

/// Fetches racing for one version leave one whole binary that every one of them takes.
#[test]
fn parallel_fetches_leave_one_whole_chromedriver() {
    let binary = vec![9_u8; 4 << 20];
    let (root, _) = archives(vec![published("157.0.8103.79", &binary)]);
    let cache = tempfile::tempdir().expect("a temporary directory");
    let fetches: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let cache = cache.path().to_owned();
            std::thread::spawn(move || {
                resolve_chromedriver(&chrome("157.0.8103.79"), &cache, None, Some(&root))
            })
        })
        .collect();
    for fetch in fetches {
        let kept = fetch
            .join()
            .expect("the fetch ran")
            .expect("every fetch takes the one binary");
        assert_eq!(std::fs::read(&kept).expect("the executable"), binary);
    }
    verify_kept(&cache.path().join("157.0.8103.79")).expect("the binary matches its record");
}

/// On a filesystem that refuses hard links, a cache file is created in place, never over the one
/// another fetch put there, and a cache that cannot be written is a host that cannot run the lane,
/// not an I/O failure.
#[test]
fn where_links_are_refused_a_cache_file_is_created_in_place() {
    let cache = tempfile::tempdir().expect("a temporary directory");
    place_without_links(cache.path(), "chromedriver.sha256", b"binary 1\n")
        .expect("created in place");
    assert_eq!(
        std::fs::read(cache.path().join("chromedriver.sha256")).expect("the file"),
        b"binary 1\n"
    );
    let staged: Vec<String> = std::fs::read_dir(cache.path())
        .expect("the cache")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".partial"))
        .collect();
    assert!(staged.is_empty(), "nothing staged is left: {staged:?}");
    place_without_links(cache.path(), "chromedriver.sha256", b"binary 1\n")
        .expect("the same bytes put first are taken");
    let differs = place_without_links(cache.path(), "chromedriver.sha256", b"binary 2\n");
    assert!(
        matches!(&differs, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("differs")),
        "{differs:?}"
    );
    assert_eq!(
        std::fs::read(cache.path().join("chromedriver.sha256")).expect("the file"),
        b"binary 1\n",
        "the file put first is never overwritten"
    );
    let unusable = place_without_links(&cache.path().join("missing"), "chromedriver", b"x");
    assert!(
        matches!(&unusable, Err(Error::Lane { unavailable: true, message, .. }) if message.contains("unusable")),
        "{unusable:?}"
    );
}

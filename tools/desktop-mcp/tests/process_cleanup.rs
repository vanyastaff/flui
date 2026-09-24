//! Process lifetime guarantees through the real stdio server, without a desktop.

mod support;

use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::json;
use support::Client;

const FIXTURE_PORT: &str = "FLUI_MCP_PROCESS_FIXTURE_PORT";

/// Executed only as a child of the server. Keeping this socket open proves
/// the fixture is still alive; dropping the parent's socket also lets it
/// exit if an assertion fails before the server can clean it up.
#[test]
#[ignore = "helper launched by process cleanup tests"]
fn launched_process_fixture() {
    let Ok(port) = std::env::var(FIXTURE_PORT) else {
        return;
    };
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .expect("BUG: the parent listens for the fixture");
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .expect("BUG: the fixture read timeout can be set");
    let mut byte = [0];
    let _ = stream.read(&mut byte);
}

fn launch_fixture(client: &mut Client) -> TcpStream {
    let listener = TcpListener::bind("127.0.0.1:0").expect("BUG: the fixture listener binds");
    listener
        .set_nonblocking(true)
        .expect("BUG: nonblocking listener");
    let port = listener
        .local_addr()
        .expect("BUG: listener has an address")
        .port();
    let program = std::env::current_exe().expect("BUG: the test binary has a path");
    let launched = client.call(
        "launch",
        json!({
            "program": program,
            "args": ["--ignored", "--exact", "launched_process_fixture"],
        "env": { (FIXTURE_PORT): port.to_string() },
        }),
    );
    assert_ne!(
        launched["isError"], true,
        "fixture launch failed: {launched}"
    );
    assert!(
        launched["structuredContent"]["pid"].as_u64().is_some(),
        "{launched}"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .expect("BUG: the fixture observation has a timeout");
                let alive = stream.read(&mut [0]);
                assert!(
                    matches!(alive, Err(ref error) if matches!(
                        error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut
                    )),
                    "fixture must still be running before shutdown: {alive:?}"
                );
                return stream;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "the launched fixture did not connect"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("accepting the launched fixture failed: {error}"),
        }
    }
}

fn assert_fixture_ended(mut fixture: TcpStream) {
    fixture
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("BUG: the fixture observation has a timeout");
    let mut byte = [0];
    match fixture.read(&mut byte) {
        Ok(0) => {}
        // A hard exit may reset its open connection rather than send FIN.
        Err(error) if error.kind() == ErrorKind::ConnectionReset => {}
        result => panic!("the launched process must end with the server: {result:?}"),
    }
}

#[test]
fn stdin_eof_ends_the_launched_process() {
    let (mut client, _) = Client::start();
    let fixture = launch_fixture(&mut client);
    client.close_stdin();
    let status = client.wait_for_exit(Duration::from_secs(30));
    assert!(status.success(), "clean shutdown failed: {status}");
    assert_fixture_ended(fixture);
}

#[cfg(windows)]
#[test]
fn killing_the_server_ends_the_launched_process() {
    let (mut client, _) = Client::start();
    let fixture = launch_fixture(&mut client);
    client.hard_kill();
    client.wait_for_exit(Duration::from_secs(10));
    assert_fixture_ended(fixture);
}

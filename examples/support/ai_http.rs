//! Real HTTP transport for the streaming example's tests.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub fn spawn_server() -> (String, JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("provider server binds");
    let address = listener.local_addr().expect("provider address");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut requests = 0;
        let mut connections = 0;
        while requests < 3 {
            assert!(Instant::now() < deadline, "provider request deadline");
            let (stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("provider accept failed: {error}"),
            };
            stream
                .set_nonblocking(false)
                .expect("blocking request reads");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read deadline");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("write deadline");
            connections += 1;
            let mut stream = BufReader::new(stream);
            while requests < 3 && read_request(&mut stream) {
                let body = if requests == 0 {
                    "data: {broken}\n\n"
                } else {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Привет\"}}]}\r\n\r\ndata: [DONE]\n\n"
                };
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                stream
                    .get_mut()
                    .write_all(headers.as_bytes())
                    .expect("provider headers");
                // Exercise real incremental reads. The parser's separate table
                // checks every possible UTF-8/CRLF split, regardless of TCP coalescing.
                for part in body.as_bytes().chunks(7) {
                    stream
                        .get_mut()
                        .write_all(part)
                        .expect("provider event bytes");
                }
                requests += 1;
            }
        }
        connections
    });
    (format!("http://{address}/chat/completions"), server)
}

fn read_request(stream: &mut BufReader<TcpStream>) -> bool {
    let mut line = String::new();
    if stream.read_line(&mut line).expect("request line") == 0 {
        return false;
    }
    assert!(line.starts_with("POST /chat/completions HTTP/1.1"));
    let mut length = None;
    let mut header_bytes = line.len();
    loop {
        line.clear();
        assert!(stream.read_line(&mut line).expect("request header") > 0);
        header_bytes += line.len();
        assert!(header_bytes < 16_384, "request headers exceed test limit");
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(value.trim().parse::<usize>().expect("body length"));
        }
    }
    let length = length.expect("JSON request has a known length");
    assert!(length < 16_384);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).expect("request JSON bytes");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("valid request JSON");
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["content"], "hello");
    true
}

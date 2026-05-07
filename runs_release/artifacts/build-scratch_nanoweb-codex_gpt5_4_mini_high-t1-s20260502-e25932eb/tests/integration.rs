use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use tempfile::tempdir;
use tokio::process::Command;

#[derive(Debug)]
struct RawResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn auth_header() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("user:pass")
    )
}

fn write_config(path: &std::path::Path, listen: u16) {
    let config = format!(
        r#"
listen = "127.0.0.1:{listen}"
metrics = "/metrics"
log_level = "warn"
graceful_timeout_s = 5
session_secret = "test-secret"

[[middleware]]
kind = "logging"

[[middleware]]
kind = "basic_auth"
username = "user"
password = "pass"

[[middleware]]
kind = "rate_limit"
permits_per_second = 0.0
burst = 8.0

[[middleware]]
kind = "cors"
allowed_origins = ["https://example.com"]

[[middleware]]
kind = "csrf"
cookie_name = "csrf_token"
header_name = "x-csrf-token"

[[middleware]]
kind = "gzip"

[[routes]]
method = "GET"
path = "/hello"
kind = "text"
body = "hello"

[[routes]]
method = "POST"
path = "/echo"
kind = "echo_body"

[[routes]]
method = "GET"
path = "/greet/:name"
kind = "template"
template = "<h1>Hello {{{{name}}}}</h1>"
context = {{ title = "Greeting" }}

[[routes]]
method = "GET"
path = "/limited"
kind = "text"
body = "limited"
"#,
    );
    std::fs::write(path, config).unwrap();
}

fn send_request(port: u16, request: &str) -> std::io::Result<RawResponse> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.write_all(request.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing response terminator",
            )
        })?;
    let (head, body) = bytes.split_at(header_end + 4);
    let head = String::from_utf8(head.to_vec())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let mut lines = head.lines();
    let status_line = lines.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing status line")
    })?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing status code"))?
        .parse::<u16>()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok(RawResponse {
        status,
        headers,
        body: body.to_vec(),
    })
}

async fn spawn_server(port: u16, config_path: &std::path::Path) -> tokio::process::Child {
    let binary = env!("CARGO_BIN_EXE_nanoweb");
    Command::new(binary)
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--config")
        .arg(config_path)
        .arg("--log-level")
        .arg("warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn wait_until_ready(port: u16) {
    for _ in 0..50 {
        let response = send_request(
            port,
            "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        );
        if let Ok(response) = response {
            if response.status == 200 {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("server did not become ready");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_to_end_server_stack() {
    let tempdir = tempdir().unwrap();
    let port = free_port();
    let config_path = tempdir.path().join("nanoweb.toml");
    write_config(&config_path, port);

    let mut child = spawn_server(port, &config_path).await;
    wait_until_ready(port).await;

    let unauthorized = send_request(
        port,
        "GET /hello HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    assert_eq!(unauthorized.status, 401);

    let auth = auth_header();
    let hello = send_request(
        port,
        &format!(
            "GET /hello HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nOrigin: https://example.com\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n"
        ),
    )
    .unwrap();
    assert_eq!(hello.status, 200);
    assert_eq!(
        hello.headers.get("content-encoding").map(String::as_str),
        Some("gzip")
    );
    assert_eq!(
        hello
            .headers
            .get("access-control-allow-origin")
            .map(String::as_str),
        Some("https://example.com")
    );
    assert!(hello.headers.contains_key("x-request-id"));
    let csrf_cookie = hello
        .headers
        .get("set-cookie")
        .expect("csrf cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf_token = csrf_cookie.split_once('=').unwrap().1.to_string();

    let csrf_rejected = send_request(
        port,
        &format!(
            "POST /echo HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nCookie: {csrf_cookie}\r\nContent-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{{\"a\":1}}"
        ),
    )
    .unwrap();
    assert_eq!(csrf_rejected.status, 403);

    let echoed = send_request(
        port,
        &format!(
            "POST /echo HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nCookie: {csrf_cookie}\r\nX-CSRF-Token: {csrf_token}\r\nContent-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{{\"a\":1}}"
        ),
    )
    .unwrap();
    assert_eq!(echoed.status, 200);
    assert_eq!(String::from_utf8(echoed.body).unwrap(), "{\"a\":1}");

    let greet = send_request(
        port,
        &format!(
            "GET /greet/Alice HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
        ),
    )
    .unwrap();
    assert_eq!(greet.status, 200);
    assert!(
        String::from_utf8(greet.body)
            .unwrap()
            .contains("Hello Alice")
    );

    for _ in 0..4 {
        let response = send_request(
            port,
            &format!(
                "GET /limited HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
            ),
        )
        .unwrap();
        assert_eq!(response.status, 200);
    }
    let limited = send_request(
        port,
        &format!(
            "GET /limited HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
        ),
    )
    .unwrap();
    assert_eq!(limited.status, 429);

    let _ = child.kill().await;
    let _ = child.wait().await;
}

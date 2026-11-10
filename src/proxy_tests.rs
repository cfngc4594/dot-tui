//! Loopback-only transport tests. Each child gets its own proxy environment,
//! avoiding unsafe process-global environment mutation and real credentials.
use super::*;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    process::{Command, Stdio},
    thread,
    time::Instant,
};

const PROXY_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "REQUEST_METHOD",
];
const HOST: &str = "proxy-fixture.invalid";

fn fixture_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer synthetic-account-secret"),
    );
    headers.insert(
        "ChatGPT-Account-Id",
        HeaderValue::from_static("synthetic-account-id"),
    );
    headers
}

// Invoked by the parent test with only a loopback destination and fake headers.
#[test]
fn transport_probe() {
    let Ok(destination) = std::env::var("DOT_TUI_PROXY_TEST_DESTINATION") else {
        return;
    };
    let destination: SocketAddr = destination.parse().unwrap();
    assert!(destination.ip().is_loopback());
    let direct = std::env::var_os("DOT_TUI_PROXY_TEST_DIRECT").is_some();
    let client = transport_builder(fixture_headers(), direct)
        .resolve(HOST, destination)
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let result = client
        .get(format!(
            "https://{HOST}:{}/backend-api/tbo",
            destination.port()
        ))
        .send();
    // The local fixture rejects CONNECT or closes the TLS handshake. Never
    // print the raw request/error, and never accept an insecure response.
    assert!(result.is_err());
}

fn check_route(vars: &[(&str, &str)], force_direct: bool, expect_proxy: bool, tunnel: bool) {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let destination_addr = destination.local_addr().unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    destination.set_nonblocking(true).unwrap();
    proxy.set_nonblocking(true).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "backend::proxy_tests::transport_probe",
            "--nocapture",
        ])
        .env(
            "DOT_TUI_PROXY_TEST_DESTINATION",
            destination_addr.to_string(),
        )
        .env_remove("DOT_TUI_PROXY_TEST_DIRECT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in PROXY_VARS {
        command.env_remove(name);
    }
    for (name, value) in vars {
        command.env(
            name,
            if *value == "PROXY" {
                proxy_url.as_str()
            } else {
                *value
            },
        );
    }
    if force_direct {
        command.env("DOT_TUI_PROXY_TEST_DIRECT", "1");
    }
    let mut child = command.spawn().unwrap();
    let expected = if expect_proxy { &proxy } else { &destination };
    let deadline = Instant::now() + Duration::from_secs(8);
    let stream = loop {
        match expected.accept() {
            Ok((stream, _)) => break Some(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
                    break None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };
    if let Some(mut stream) = stream {
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        if expect_proxy {
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 16384 {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.starts_with(&format!(
                "CONNECT {HOST}:{} HTTP/1.1\r\n",
                destination_addr.port()
            )));
            let lower = request.to_ascii_lowercase();
            assert!(!lower.contains("authorization:"));
            assert!(!lower.contains("chatgpt-account-id"));
            assert!(!lower.contains("synthetic-account"));
            assert!(!lower.contains("/backend-api/"));
            if tunnel {
                stream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .unwrap();
                let mut tls = [0; 5];
                stream.read_exact(&mut tls).unwrap();
                assert_eq!(&tls[..2], &[0x16, 0x03]); // TLS ClientHello, never plaintext auth.
            } else {
                stream
                    .write_all(
                        b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                    )
                    .unwrap();
            }
        } else {
            let mut tls = [0; 5];
            stream.read_exact(&mut tls).unwrap();
            assert_eq!(&tls[..2], &[0x16, 0x03]);
        }
        drop(stream);
    } else {
        let _ = child.kill();
        let _ = child.wait();
        panic!("transport did not use the expected loopback route");
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "isolated transport probe failed");
    // Neither failed CONNECT nor failed TLS may trigger a retry or fall back
    // to the other route. Any extra accepted connection is a regression.
    for listener in [&proxy, &destination] {
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    }
}

#[test]
fn proxy_environment_matches_grok_reqwest_behavior() {
    check_route(&[("HTTPS_PROXY", "PROXY")], false, true, false);
    check_route(&[("https_proxy", "PROXY")], false, true, false);
    check_route(&[("ALL_PROXY", "PROXY")], false, true, false);
    check_route(&[("all_proxy", "PROXY")], false, true, false);
    check_route(
        &[
            ("HTTPS_PROXY", "PROXY"),
            ("ALL_PROXY", "http://127.0.0.1:9"),
        ],
        false,
        true,
        false,
    );
    // HTTP_PROXY alone must not intercept an HTTPS destination.
    check_route(&[("HTTP_PROXY", "PROXY")], false, false, false);
    check_route(&[], false, false, false);
}

#[test]
fn no_proxy_environment_and_explicit_direct_are_respected() {
    check_route(
        &[("HTTPS_PROXY", "PROXY"), ("NO_PROXY", HOST)],
        false,
        false,
        false,
    );
    check_route(
        &[("HTTPS_PROXY", "PROXY"), ("no_proxy", ".invalid")],
        false,
        false,
        false,
    );
    check_route(
        &[("HTTPS_PROXY", "PROXY"), ("NO_PROXY", "*")],
        false,
        false,
        false,
    );
    check_route(
        &[
            ("HTTPS_PROXY", "PROXY"),
            ("NO_PROXY", "not-proxy-fixture.invalid"),
        ],
        false,
        true,
        false,
    );
    check_route(
        &[("HTTPS_PROXY", "PROXY"), ("ALL_PROXY", "PROXY")],
        true,
        false,
        false,
    );
}

#[test]
fn connect_is_credential_free_and_then_starts_tls() {
    check_route(&[("HTTPS_PROXY", "PROXY")], false, true, true);
}

#[test]
fn plain_http_remains_forbidden() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = transport_builder(fixture_headers(), true).build().unwrap();
    assert!(
        client
            .get(format!(
                "http://{}/backend-api/tbo",
                listener.local_addr().unwrap()
            ))
            .send()
            .is_err()
    );
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
}

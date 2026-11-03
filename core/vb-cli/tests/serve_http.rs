// Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

//! Integration tests against the real HTTP server.
//!
//! These bind a real socket and speak HTTP over it, rather than calling the
//! handler directly, so the wiring between the transport and the shared API
//! is covered rather than assumed.
//!
//! Requests are written by hand over `TcpStream` to keep the test free of an
//! HTTP client dependency.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;

/// Start a server on an ephemeral port and return its address.
///
/// The thread is left running; the test process ending cleans it up.
fn start(web_root: Option<PathBuf>) -> SocketAddr {
    let server = vb_cli::serve::bind(0).expect("should bind an ephemeral port");
    let addr = server
        .server_addr()
        .to_ip()
        .expect("a TCP listener always has an IP address");
    std::thread::spawn(move || {
        let options = vb_cli::serve::ServeOptions {
            port: 0,
            web_root,
            allow_origin: None,
            open_browser: false,
        };
        let _ = vb_cli::serve::run(server, &options);
    });
    addr
}

/// Send a raw request and return the whole response.
fn request(addr: SocketAddr, raw: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("server should accept a connection");
    stream
        .write_all(raw.as_bytes())
        .expect("request should send");
    stream.flush().ok();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response should read");
    response
}

fn get(addr: SocketAddr, path: &str) -> String {
    request(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn body_of(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("")
}

fn json_body(response: &str) -> serde_json::Value {
    serde_json::from_str(body_of(response)).unwrap_or_else(|e| {
        panic!("body was not JSON ({e}): {}", body_of(response));
    })
}

#[test]
fn the_version_endpoint_reports_both_contracts() {
    let addr = start(None);
    let value = json_body(&get(addr, "/api/version"));
    assert_eq!(value["status"], "ok");
    assert_eq!(value["data"]["footageGraph"], "footage-graph/0.1");
    assert_eq!(value["data"]["editingDsl"], "editing-dsl/0.1");
}

#[test]
fn the_doctor_endpoint_lists_adapters_and_their_mock_status() {
    let addr = start(None);
    let value = json_body(&get(addr, "/api/doctor"));
    assert_eq!(value["status"], "ok");
    let adapters = value["data"]["adapters"].as_array().unwrap();
    assert!(!adapters.is_empty());
    // Every adapter says whether it is a mock. Whether any is depends on the
    // machine -- a Mac with a speech model downloaded runs none -- but the UI
    // relies on always being told.
    assert!(
        adapters.iter().all(|a| a["mock"].is_boolean()),
        "the UI relies on knowing which adapters are mocks: {adapters:?}"
    );
    assert_eq!(value["data"]["requiresNetwork"], false);
}

#[test]
fn the_templates_endpoint_returns_the_prior_store() {
    let addr = start(None);
    let value = json_body(&get(addr, "/api/templates"));
    assert!(value["data"].as_array().unwrap().len() >= 9);
}

#[test]
fn browsing_works_over_http_with_an_encoded_path() {
    let dir = std::env::temp_dir().join("vb serve browse test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("clip.mp4"), b"x").unwrap();

    let addr = start(None);
    // The space in the directory name has to survive percent-encoding, which
    // is exactly the case a naive query parser gets wrong.
    let encoded = dir
        .to_string_lossy()
        .replace(' ', "%20")
        .replace('/', "%2F");
    let value = json_body(&get(addr, &format!("/api/browse?dir={encoded}")));

    assert_eq!(value["status"], "ok");
    let names: Vec<&str> = value["data"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"clip.mp4"), "got {names:?}");
}

#[test]
fn an_unknown_endpoint_is_an_error_rather_than_a_crash() {
    let addr = start(None);
    let value = json_body(&get(addr, "/api/nonsense"));
    assert_eq!(value["status"], "error");
}

#[test]
fn a_run_without_an_input_is_rejected() {
    let addr = start(None);
    let body = r#"{"projectDir":"/tmp"}"#;
    let response = request(
        addr,
        &format!(
            "POST /api/run HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let value = json_body(&response);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("input"));
}

#[test]
fn no_cors_header_is_sent_by_default() {
    // Without this, a page on any other origin could read a directory listing
    // or start a pipeline run on the user's machine.
    let addr = start(None);
    let response = get(addr, "/api/version");
    assert!(
        !response
            .to_lowercase()
            .contains("access-control-allow-origin"),
        "a CORS header leaked into a default response"
    );
}

#[test]
fn static_files_are_served_and_traversal_is_refused() {
    let base = std::env::temp_dir().join("vb-serve-http-static");
    let root = base.join("web");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.html"), b"<html>the app</html>").unwrap();
    std::fs::write(base.join("secret.txt"), b"do not serve me").unwrap();

    let addr = start(Some(root));

    assert!(get(addr, "/index.html").contains("the app"));
    // An unknown route falls back to the app, because Flutter routes in the
    // browser and a deep link would otherwise 404.
    assert!(get(addr, "/some/deep/link").contains("the app"));

    let escaped = get(addr, "/../secret.txt");
    assert!(
        !escaped.contains("do not serve me"),
        "the server read a file outside its web root"
    );
}

#[test]
fn the_api_still_answers_when_no_web_build_is_present() {
    // `vb serve` is useful before the UI has ever been built, and should say
    // so rather than fail.
    let addr = start(None);
    assert!(get(addr, "/api/version").contains("\"ok\""));
    assert!(get(addr, "/").contains("flutter build web"));
}

fn post(addr: SocketAddr, path: &str, body: &str) -> String {
    request(
        addr,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn any_operation_is_reachable_by_name_over_http() {
    // The web build reaches every operation this way, so the route is the
    // one the whole operation set depends on.
    let addr = start(None);
    let v = json_body(&post(addr, "/api/call/version", "{}"));
    assert_eq!(v["status"], "ok", "{v}");
    assert_eq!(v["data"]["editingDsl"], "editing-dsl/0.1");

    let v = json_body(&post(addr, "/api/call/browse", r#"{"kind":"audio"}"#));
    assert_eq!(v["status"], "ok", "{v}");
    assert!(v["data"]["entries"].is_array());
}

#[test]
fn an_operation_cannot_be_triggered_by_a_get() {
    // Several of these change state -- projects, settings, downloads -- and a
    // GET that did would be triggerable by any link on any page.
    let addr = start(None);
    let v = json_body(&get(addr, "/api/call/projects.list"));
    assert_eq!(v["status"], "error");
    assert!(v["message"].as_str().unwrap().contains("POST"));
}

#[test]
fn a_run_started_over_http_reports_as_a_job() {
    let addr = start(None);
    let v = json_body(&post(
        addr,
        "/api/call/run.start",
        r#"{"inputs":["/definitely/not/here.mp4"]}"#,
    ));
    assert_eq!(v["status"], "ok", "{v}");
    let job = v["data"]["jobId"].as_str().expect("a job id").to_string();

    let mut last = serde_json::Value::Null;
    for _ in 0..200 {
        last = json_body(&post(
            addr,
            "/api/call/run.status",
            &format!(r#"{{"jobId":"{job}"}}"#),
        ));
        if last["data"]["finished"] == true {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(last["data"]["state"], "failed", "{last}");
    assert!(
        last["data"]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("does not exist")
    );
}

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

//! A local HTTP front end for the engine.
//!
//! This exists because Flutter cannot build a macOS app without full Xcode,
//! and installing Xcode should not be a prerequisite for having a working
//! desktop app. Flutter's *web* target needs no Xcode at all, so the same Dart
//! UI is compiled to web and served from here. When Xcode is installed later,
//! `flutter build macos` produces a native bundle from the identical source;
//! nothing about the UI diverges.
//!
//! # Why this is safe to run, and the line not to cross
//!
//! These endpoints browse the filesystem and execute a pipeline. That is fine
//! for a local application and would be a serious hole if it were reachable
//! from anywhere else, so two things hold:
//!
//! 1. **Loopback only.** The listener binds `127.0.0.1`, never `0.0.0.0`, so
//!    nothing off this machine can connect.
//! 2. **Same origin only.** No CORS headers are sent by default, so a page on
//!    another origin cannot read a response or send a JSON POST. `--allow-origin`
//!    opts out of that for hot-reload development, and says so when used.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_http::{Header, Request, Response, Server};

pub struct ServeOptions {
    pub port: u16,
    /// Directory holding the built Flutter web app. Absent means API only.
    pub web_root: Option<PathBuf>,
    /// An origin to allow for development hot reload. Off by default.
    pub allow_origin: Option<String>,
    pub open_browser: bool,
}

/// Bind the listener.
///
/// Loopback only, always. Binding anything else would expose filesystem
/// browsing and pipeline execution to the network, which these endpoints are
/// not written to withstand.
pub fn bind(port: u16) -> Result<Server> {
    let address = format!("127.0.0.1:{port}");
    Server::http(&address)
        .map_err(|e| anyhow::anyhow!("could not bind {address}: {e}"))
        .context("the port may already be in use; try --port")
}

/// Handle requests until the process ends. Separated from [`serve`] so tests
/// can bind an ephemeral port, learn which one, and drive the real server.
pub fn run(server: Server, options: &ServeOptions) -> Result<()> {
    for request in server.incoming_requests() {
        if let Err(e) = handle(request, options) {
            tracing::warn!(error = %e, "request failed");
        }
    }
    Ok(())
}

pub fn serve(options: ServeOptions) -> Result<()> {
    let server = bind(options.port)?;
    let address = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| format!("127.0.0.1:{}", options.port));

    let url = format!("http://{address}");
    println!("Video Butler is running at {url}");
    match &options.web_root {
        Some(root) => println!("Serving the app from {}", root.display()),
        None => println!(
            "No web build found, so only the API is available. Build the UI with:\n  \
             cd app && flutter build web"
        ),
    }
    if let Some(origin) = &options.allow_origin {
        println!(
            "Allowing cross-origin requests from {origin}. This is for development \
             only; do not leave it on."
        );
    }
    println!("Press Ctrl-C to stop.");

    if options.open_browser {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }

    run(server, &options)
}

fn handle(mut request: Request, options: &ServeOptions) -> Result<()> {
    let raw_url = request.url().to_string();
    let (path, query) = split_query(&raw_url);
    tracing::debug!(method = %request.method(), %path, "request");

    // A preflight only succeeds when an origin was explicitly allowed.
    if request.method().as_str() == "OPTIONS" {
        return Ok(request.respond(with_cors(Response::empty(204), options))?);
    }

    if let Some(endpoint) = path.strip_prefix("/api/") {
        let body = match endpoint {
            "version" => json_ok(vb_pipeline::api::version()),
            "doctor" => json_ok(vb_pipeline::api::doctor()),
            "templates" => json_ok(vb_pipeline::api::templates()),
            "browse" => {
                let dir = query.get("dir").map(String::as_str);
                json_ok(vb_pipeline::api::browse(dir))
            }
            "run" => {
                let mut text = String::new();
                request.as_reader().read_to_string(&mut text).ok();
                run_from_json(&text)
            }
            // Every other operation, by name. POST only: these change state
            // (projects, settings, downloads), and a GET that did would be
            // triggerable by any link on any page.
            call if call.starts_with("call/") => {
                if request.method().as_str() != "POST" {
                    json_err("operations are called with POST")
                } else {
                    let mut text = String::new();
                    request.as_reader().read_to_string(&mut text).ok();
                    call_from_json(&call["call/".len()..], &text)
                }
            }
            _ => json_err(format!("unknown endpoint {endpoint:?}")),
        };

        let response = Response::from_string(body)
            .with_header(header("Content-Type", "application/json; charset=utf-8"))
            // Results reflect files on disk that can change between calls.
            .with_header(header("Cache-Control", "no-store"));
        return Ok(request.respond(with_cors(response, options))?);
    }

    match &options.web_root {
        Some(root) => serve_static(request, root, &path),
        None => {
            let response = Response::from_string(
                "The API is running, but the app has not been built.\n\n\
                 Build it with:  cd app && flutter build web\n",
            )
            .with_status_code(404);
            Ok(request.respond(response)?)
        }
    }
}

fn call_from_json(method: &str, text: &str) -> String {
    let args: serde_json::Value = if text.trim().is_empty() {
        serde_json::Value::Object(Default::default())
    } else {
        match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return json_err(format!("the request body is not valid JSON: {e}")),
        }
    };
    match vb_pipeline::api::call(method, &args) {
        Ok(value) => json_ok(value),
        Err(message) => json_err(message),
    }
}

fn run_from_json(text: &str) -> String {
    let payload: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return json_err(format!("the request body is not valid JSON: {e}")),
    };
    // "inputs" is the list a project is made of; "input" stays for a single
    // path, which is what one video looks like from a caller's side.
    let inputs: Vec<String> = match payload["inputs"].as_array() {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => match payload["input"].as_str() {
            Some(one) => vec![one.to_string()],
            None => return json_err("the request must include \"inputs\" or an \"input\" path"),
        },
    };
    if inputs.is_empty() {
        return json_err("the request must include at least one input path");
    }
    match vb_pipeline::api::run(
        &inputs,
        payload["projectDir"].as_str(),
        payload["templateId"].as_str(),
    ) {
        Ok(value) => json_ok(value),
        Err(message) => json_err(message),
    }
}

/// Resolve a request path to a file inside the web root.
///
/// Returns `None` when the path escapes the root. Without that check, a
/// request for `/../../../../etc/passwd` would read arbitrary files off the
/// disk, and this server is pointed at a user's home directory by design.
///
/// Resolution is done with `canonicalize`, which follows symlinks, so a
/// symlink planted inside the web root cannot be used to step outside it
/// either.
fn resolve_static_path(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };

    let root = std::fs::canonicalize(root).ok()?;
    let resolved = std::fs::canonicalize(root.join(relative)).ok()?;
    if resolved.starts_with(&root) {
        Some(resolved)
    } else {
        None
    }
}

/// Serve a file from the web build.
///
/// Any path that does not resolve to a real file inside the root falls back to
/// `index.html`, because a Flutter web app routes in the browser and a deep
/// link would otherwise 404.
fn serve_static(request: Request, root: &Path, path: &str) -> Result<()> {
    let file = match resolve_static_path(root, path).filter(|p| p.is_file()) {
        Some(p) => p,
        None => root.join("index.html"),
    };

    match std::fs::read(&file) {
        Ok(bytes) => {
            let response =
                Response::from_data(bytes).with_header(header("Content-Type", content_type(&file)));
            Ok(request.respond(response)?)
        }
        Err(_) => Ok(request.respond(Response::from_string("not found").with_status_code(404))?),
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn with_cors<R: std::io::Read>(response: Response<R>, options: &ServeOptions) -> Response<R> {
    match &options.allow_origin {
        Some(origin) => response
            .with_header(header("Access-Control-Allow-Origin", origin))
            .with_header(header("Access-Control-Allow-Headers", "Content-Type"))
            .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")),
        None => response,
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("header names and values here are static ASCII")
}

fn json_ok(data: serde_json::Value) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

fn json_err(message: impl Into<String>) -> String {
    serde_json::json!({ "status": "error", "message": message.into() }).to_string()
}

/// Split a request target into its path and decoded query parameters.
fn split_query(url: &str) -> (String, HashMap<String, String>) {
    let mut params = HashMap::new();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    if let Some(query) = query {
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            params.insert(percent_decode(key), percent_decode(value));
        }
    }
    (percent_decode(path), params)
}

/// Decode percent-escapes and `+`.
///
/// File paths routinely contain spaces and non-ASCII characters, so this has
/// to handle multi-byte UTF-8 rather than only ASCII escapes.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_has_no_parameters() {
        let (path, params) = split_query("/api/doctor");
        assert_eq!(path, "/api/doctor");
        assert!(params.is_empty());
    }

    #[test]
    fn query_parameters_are_decoded() {
        let (path, params) = split_query("/api/browse?dir=%2FUsers%2Fzhou%2FMy%20Movies");
        assert_eq!(path, "/api/browse");
        assert_eq!(params["dir"], "/Users/zhou/My Movies");
    }

    #[test]
    fn non_ascii_paths_survive_decoding() {
        // Multi-byte UTF-8 arrives as several escapes and must reassemble.
        let (_, params) = split_query("/api/browse?dir=%2Ftmp%2F%E5%B1%B1%E8%A1%8C");
        assert_eq!(params["dir"], "/tmp/山行");
    }

    #[test]
    fn a_plus_decodes_to_a_space() {
        let (_, params) = split_query("/api/browse?dir=/tmp/a+b");
        assert_eq!(params["dir"], "/tmp/a b");
    }

    #[test]
    fn a_malformed_escape_is_left_alone_rather_than_dropped() {
        let (_, params) = split_query("/api/browse?dir=%zz");
        assert_eq!(params["dir"], "%zz");
    }

    #[test]
    fn a_run_request_without_an_input_is_an_error() {
        let body = run_from_json("{}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "error");
        assert!(v["message"].as_str().unwrap().contains("input"));
    }

    #[test]
    fn a_malformed_run_body_is_an_error_not_a_panic() {
        let body = run_from_json("not json at all");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "error");
    }

    #[test]
    fn content_types_cover_what_a_flutter_web_build_emits() {
        assert_eq!(
            content_type(Path::new("a/index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("main.dart.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("app.wasm")), "application/wasm");
        assert_eq!(content_type(Path::new("f.woff2")), "font/woff2");
    }

    /// A web root with one real file and one file outside it, for the
    /// traversal tests.
    ///
    /// Each test gets its own directory: they run in parallel, and a shared
    /// fixture that each test deletes and recreates makes them fail at random.
    fn fixture_root(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("vb-serve-static-{name}"));
        let root = base.join("web");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), b"<html>").unwrap();
        std::fs::write(root.join("assets/app.js"), b"//js").unwrap();
        std::fs::write(base.join("secret.txt"), b"do not serve me").unwrap();
        root
    }

    #[test]
    fn a_file_inside_the_root_resolves() {
        let root = fixture_root("inside");
        assert!(resolve_static_path(&root, "/index.html").is_some());
        assert!(resolve_static_path(&root, "/assets/app.js").is_some());
    }

    #[test]
    fn an_empty_path_resolves_to_the_index() {
        let root = fixture_root("index");
        let resolved = resolve_static_path(&root, "/").unwrap();
        assert!(resolved.ends_with("index.html"));
    }

    #[test]
    fn a_path_escaping_the_root_is_refused() {
        // This server is pointed at a user's machine, so traversal is not a
        // theoretical concern.
        let root = fixture_root("escape");
        assert!(
            resolve_static_path(&root, "/../secret.txt").is_none(),
            "traversal out of the web root must be refused"
        );
        assert!(resolve_static_path(&root, "/../../../../etc/passwd").is_none());
        assert!(resolve_static_path(&root, "/assets/../../secret.txt").is_none());
    }

    #[test]
    fn a_symlink_pointing_outside_the_root_is_refused() {
        // canonicalize follows symlinks, so a link planted inside the root
        // cannot be used to step outside it.
        let root = fixture_root("symlink");
        let outside = root.parent().unwrap().join("secret.txt");
        let link = root.join("sneaky.txt");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &link);
        // Windows lets a process make a link only in Developer Mode or as an
        // administrator; without either there is nothing to check.
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, &link);
        if linked.is_ok() {
            assert!(resolve_static_path(&root, "/sneaky.txt").is_none());
        }
    }

    #[test]
    fn a_missing_file_does_not_resolve_so_the_caller_can_fall_back() {
        let root = fixture_root("missing");
        assert!(resolve_static_path(&root, "/nope.html").is_none());
    }

    #[test]
    fn no_cors_headers_are_sent_unless_an_origin_was_allowed() {
        // A page on another origin must not be able to read these responses.
        let locked = ServeOptions {
            port: 0,
            web_root: None,
            allow_origin: None,
            open_browser: false,
        };
        let response = with_cors(Response::from_string("x"), &locked);
        assert!(
            !response
                .headers()
                .iter()
                .any(|h| h.field.equiv("Access-Control-Allow-Origin")),
            "CORS must be off by default"
        );

        let dev = ServeOptions {
            allow_origin: Some("http://localhost:1234".into()),
            ..locked
        };
        let response = with_cors(Response::from_string("x"), &dev);
        assert!(
            response
                .headers()
                .iter()
                .any(|h| h.field.equiv("Access-Control-Allow-Origin"))
        );
    }
}

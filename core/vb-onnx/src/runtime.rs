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

//! Finding and loading ONNX Runtime.
//!
//! The library is found the way `FfmpegCli` finds ffmpeg: an explicit path
//! first, then where the app bundle carries it, then where a developer's
//! checkout or a package manager puts it. It is loaded once per process;
//! ONNX Runtime allows one environment, and a second load would be ignored.

use ort::ep;
use ort::session::Session;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use vb_infer::InferError;

#[cfg(target_os = "macos")]
const FILE: &str = "libonnxruntime.dylib";
#[cfg(target_os = "windows")]
const FILE: &str = "onnxruntime.dll";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FILE: &str = "libonnxruntime.so";

/// The places ONNX Runtime may be, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("VB_ORT_DYLIB") {
        out.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // Beside the binary, and in a macOS bundle's Frameworks directory.
        out.push(dir.join(FILE));
        out.push(dir.join("../Frameworks").join(FILE));
    }
    // A developer's checkout, after scripts/build-onnxruntime-macos.sh or
    // scripts/fetch-onnxruntime-windows.sh. On Windows always by full path:
    // System32 holds an older onnxruntime.dll of Windows ML's own.
    let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build");
    out.push(build.join("onnxruntime-macos/current/lib").join(FILE));
    out.push(build.join("onnxruntime-windows/current/lib").join(FILE));
    for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
        out.push(Path::new(dir).join(FILE));
    }
    out
}

/// Where ONNX Runtime was found, without loading it.
pub fn library_path() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file()).or_else(|| {
        // Android loads an app's native libraries straight out of the APK,
        // where no file path reaches them; the dynamic linker finds them by
        // name, among the libraries the app was installed with.
        cfg!(target_os = "android").then(|| PathBuf::from(FILE))
    })
}

static LOADED: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Open a model on the CPU provider, loading ONNX Runtime first if needed.
///
/// CPU on purpose for now: these models take inputs whose size varies with
/// the picture's shape, which the CoreML provider handles poorly, and at
/// their size they run in a fraction of a second on the reference Mac.
pub fn open_session(adapter: &'static str, model: &Path) -> Result<Session, InferError> {
    open(adapter, model, false)
}

/// A model whose input is always the same size, on CoreML where the
/// platform has it -- the Neural Engine or the GPU, several times the CPU's
/// speed for a network like this -- and on the CPU where it does not, or
/// where CoreML will not take the model.
pub fn open_fixed_session(adapter: &'static str, model: &Path) -> Result<Session, InferError> {
    if cfg!(target_vendor = "apple") && std::env::var_os("VB_ORT_CPU_ONLY").is_none() {
        match open(adapter, model, true) {
            Ok(session) => return Ok(session),
            Err(e) => tracing::warn!(error = %e, "CoreML would not take the model; using the CPU"),
        }
    }
    open(adapter, model, false)
}

fn open(adapter: &'static str, model: &Path, coreml: bool) -> Result<Session, InferError> {
    ensure().map_err(|reason| InferError::Unavailable { adapter, reason })?;
    if !model.is_file() {
        return Err(InferError::Unavailable {
            adapter,
            reason: format!("no model at {}", model.display()),
        });
    }
    let failed = |detail: String| InferError::Failed { adapter, detail };
    let providers = if coreml {
        vec![
            ep::CoreML::default().build().error_on_failure(),
            ep::CPU::default().build(),
        ]
    } else {
        vec![ep::CPU::default().build()]
    };
    Session::builder()
        .map_err(|e| failed(e.to_string()))?
        .with_execution_providers(providers)
        .map_err(|e| failed(e.to_string()))?
        .commit_from_file(model)
        .map_err(|e| failed(e.to_string()))
}

/// Load ONNX Runtime if it is not loaded yet. Every model calls this before
/// its first session, so the first model to run pays for it.
pub fn ensure() -> Result<&'static Path, String> {
    LOADED
        .get_or_init(|| {
            let path = library_path().ok_or_else(|| {
                format!(
                    "ONNX Runtime ({FILE}) was not found; scripts/build-onnxruntime-macos.sh builds \
                     it, or VB_ORT_DYLIB names it"
                )
            })?;
            let builder = ort::init_from(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .with_name("video-butler");
            // False only when something else configured the environment
            // first, which leaves the library loaded all the same.
            let _ = builder.commit();
            tracing::info!(path = %path.display(), "loaded ONNX Runtime");
            Ok(path)
        })
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(Clone::clone)
}

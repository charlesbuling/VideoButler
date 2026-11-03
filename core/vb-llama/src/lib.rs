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

//! The model host's llama.cpp engine, from the engine's side.
//!
//! The model host runs every model the app carries and does inference only.
//! It has two engines: ONNX Runtime (vb-onnx) for the small vision models,
//! and llama.cpp for vision-language models in GGUF -- the composition judge
//! first, the general VLM and the screenwriter's LLM after it.
//!
//! llama.cpp lives in a library of its own, `vb-llama-engine`, which this
//! crate loads at runtime, exactly as vb-onnx loads ONNX Runtime. It is never
//! linked into the engine: whisper.cpp is, and the two carry different
//! versions of ggml under the same symbol names. The library is found the
//! way ONNX Runtime is -- an explicit path, beside the binary or in a macOS
//! bundle's Frameworks, or where a checkout's cargo build puts it -- and
//! without it the engine runs as before, with no composition judge.

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use vb_infer::{Adapter, InferError, VlmAdapter, VlmRequest};

/// The ABI this loader speaks; the library says which it speaks.
const ABI: u32 = 1;
const ERR_LEN: usize = 1024;

#[cfg(target_os = "macos")]
const FILE: &str = "libvb_llama_engine.dylib";
#[cfg(target_os = "windows")]
const FILE: &str = "vb_llama_engine.dll";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FILE: &str = "libvb_llama_engine.so";

type OpenFn = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_char, usize) -> *mut c_void;
type DescribeFn = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    u32,
    *mut c_char,
    usize,
) -> *mut c_char;
type FreeTextFn = unsafe extern "C" fn(*mut c_char);
type CloseFn = unsafe extern "C" fn(*mut c_void);
type AbiFn = unsafe extern "C" fn() -> u32;

/// The places the library may be, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("VB_LLAMA_DYLIB") {
        out.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join(FILE));
        out.push(dir.join("../Frameworks").join(FILE));
        // A test binary sits in target/<profile>/deps.
        out.push(dir.join("..").join(FILE));
    }
    // A developer's checkout, after `cargo build -p vb-llama-engine`.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
    for profile in ["release", "debug"] {
        out.push(target.join(profile).join(FILE));
    }
    out
}

/// Where the library was found, without loading it.
pub fn library_path() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file()).or_else(|| {
        // Android loads an app's native libraries straight out of the APK,
        // where no file path reaches them; the dynamic linker finds them by
        // name, among the libraries the app was installed with.
        cfg!(target_os = "android").then(|| PathBuf::from(FILE))
    })
}

/// The library's functions, resolved once.
struct Engine {
    open: OpenFn,
    describe: DescribeFn,
    free_text: FreeTextFn,
    close: CloseFn,
    // Kept for the life of the process: the functions above point into it.
    _library: Library,
}

fn engine() -> Result<&'static Engine, String> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let path = library_path().ok_or_else(|| {
                format!(
                    "the llama.cpp library ({FILE}) was not found; `cargo build -p \
                     vb-llama-engine` builds it, or VB_LLAMA_DYLIB names it"
                )
            })?;
            // SAFETY: loading runs the library's initialisers, which are
            // Rust's and llama.cpp's own; the symbols are resolved against
            // the signatures the library was built with, checked by its ABI
            // version before any other is called.
            unsafe {
                let library =
                    Library::new(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let abi = *library
                    .get::<AbiFn>(b"vb_llama_abi\0")
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                let found = abi();
                if found != ABI {
                    return Err(format!(
                        "{} speaks ABI {found}; this engine needs {ABI}",
                        path.display()
                    ));
                }
                let get = |name: &[u8]| -> Result<*const c_void, String> {
                    library
                        .get::<*const c_void>(name)
                        .map(|s| *s)
                        .map_err(|e| format!("{}: {e}", path.display()))
                };
                let engine = Engine {
                    open: std::mem::transmute::<*const c_void, OpenFn>(get(b"vb_llama_open\0")?),
                    describe: std::mem::transmute::<*const c_void, DescribeFn>(get(
                        b"vb_llama_describe\0",
                    )?),
                    free_text: std::mem::transmute::<*const c_void, FreeTextFn>(get(
                        b"vb_llama_free_text\0",
                    )?),
                    close: std::mem::transmute::<*const c_void, CloseFn>(get(b"vb_llama_close\0")?),
                    _library: library,
                };
                tracing::info!(path = %path.display(), "loaded llama.cpp");
                Ok(engine)
            }
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Can llama.cpp be loaded here? Loads it if it can.
pub fn available() -> Result<(), String> {
    engine().map(|_| ())
}

/// A loaded model's handle. Only ever used under [`LlamaVlm::handle`]'s lock.
struct Handle(*mut c_void);
// SAFETY: the handle is used by one thread at a time, under a mutex, which
// is all the library asks.
unsafe impl Send for Handle {}

/// A vision-language model in GGUF: the language model and its projector.
pub struct LlamaVlm {
    name: &'static str,
    model: PathBuf,
    mmproj: PathBuf,
    handle: Mutex<Option<Handle>>,
}

impl LlamaVlm {
    /// `name` is what provenance records, e.g. `PhotoFramer-7B`.
    pub fn new(
        name: &'static str,
        model: impl Into<PathBuf>,
        mmproj: impl Into<PathBuf>,
    ) -> LlamaVlm {
        LlamaVlm {
            name,
            model: model.into(),
            mmproj: mmproj.into(),
            handle: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }

    fn open(&self) -> Result<Handle, InferError> {
        let unavailable = |reason: String| InferError::Unavailable {
            adapter: self.name,
            reason,
        };
        for file in [&self.model, &self.mmproj] {
            if !file.is_file() {
                return Err(unavailable(format!("no model at {}", file.display())));
            }
        }
        let engine = engine().map_err(unavailable)?;
        let model = c_path(&self.model).map_err(unavailable)?;
        let mmproj = c_path(&self.mmproj).map_err(unavailable)?;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: NUL-terminated paths and an error buffer of ERR_LEN bytes.
        let raw =
            unsafe { (engine.open)(model.as_ptr(), mmproj.as_ptr(), err.as_mut_ptr(), ERR_LEN) };
        if raw.is_null() {
            return Err(unavailable(message(&err)));
        }
        tracing::info!(model = %self.model.display(), "loaded a vision-language model");
        Ok(Handle(raw))
    }
}

impl Drop for LlamaVlm {
    fn drop(&mut self) {
        let handle = self.handle.get_mut().unwrap_or_else(|p| p.into_inner());
        if let (Some(Handle(raw)), Ok(engine)) = (handle.take(), engine()) {
            // SAFETY: a handle from vb_llama_open, closed once, not in use.
            unsafe { (engine.close)(raw) };
        }
    }
}

fn c_path(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes().to_vec())
        .map_err(|_| format!("{} contains a NUL byte", path.display()))
}

fn message(err: &[c_char]) -> String {
    // SAFETY: the library NUL-terminates what it writes; the buffer starts
    // zeroed, so an empty message is still terminated.
    unsafe { CStr::from_ptr(err.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

impl Adapter for LlamaVlm {
    fn name(&self) -> &'static str {
        self.name
    }
}

impl VlmAdapter for LlamaVlm {
    fn release(&self) {
        let mut guard = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        if let (Some(Handle(raw)), Ok(engine)) = (guard.take(), engine()) {
            // SAFETY: a handle from vb_llama_open, closed once, under the
            // lock, so nothing is using it.
            unsafe { (engine.close)(raw) };
            tracing::info!(model = %self.model.display(), "released a vision-language model");
        }
    }

    fn describe(&self, request: &VlmRequest) -> Result<String, InferError> {
        let failed = |detail: String| InferError::Failed {
            adapter: self.name,
            detail,
        };
        let mut guard = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        let Handle(raw) = guard.as_ref().expect("opened above");
        let engine = engine().map_err(failed)?;
        let image = c_path(&request.image_path).map_err(failed)?;
        let prompt =
            CString::new(request.prompt.replace('\0', "")).map_err(|e| failed(e.to_string()))?;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: a live handle used under the lock, NUL-terminated strings,
        // an error buffer of ERR_LEN bytes.
        let text = unsafe {
            (engine.describe)(
                *raw,
                image.as_ptr(),
                prompt.as_ptr(),
                request.max_tokens,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        if text.is_null() {
            return Err(failed(message(&err)));
        }
        // SAFETY: a NUL-terminated string from vb_llama_describe, freed once.
        let answer = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        unsafe { (engine.free_text)(text) };
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let vlm = LlamaVlm::new(
            "Test",
            "/nonexistent/model.gguf",
            "/nonexistent/mmproj.gguf",
        );
        match vlm.describe(&VlmRequest::describe("/tmp/frame.jpg")) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    /// With the library built (`cargo build --release -p vb-llama-engine`),
    /// a GGUF vision model and a still:
    /// VB_VLM_MODEL=<model.gguf> VB_VLM_MMPROJ=<mmproj.gguf> VB_VLM_IMAGE=<frame.jpg> \
    ///   cargo test --release -p vb-llama a_real_model -- --nocapture
    #[test]
    fn a_real_model_answers_about_a_real_frame() {
        let (Some(model), Some(mmproj), Some(image)) = (
            std::env::var_os("VB_VLM_MODEL"),
            std::env::var_os("VB_VLM_MMPROJ"),
            std::env::var_os("VB_VLM_IMAGE"),
        ) else {
            eprintln!("skipped: VB_VLM_MODEL, VB_VLM_MMPROJ and VB_VLM_IMAGE are not all set");
            return;
        };
        let vlm = LlamaVlm::new("Test", PathBuf::from(model), PathBuf::from(mmproj));
        let request = VlmRequest {
            image_path: PathBuf::from(image),
            prompt: "Describe this picture in one sentence.".into(),
            max_tokens: 64,
        };
        for round in 0..2 {
            let started = std::time::Instant::now();
            let answer = vlm.describe(&request).expect("the model answers");
            eprintln!("round {round}, {:?}: {answer}", started.elapsed());
            assert!(!answer.trim().is_empty());
        }
    }
}

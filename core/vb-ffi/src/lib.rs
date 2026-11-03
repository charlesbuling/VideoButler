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

//! The C ABI the Flutter shell binds to.
//!
//! Deliberately narrow. The app is one consumer of the engine and the CLI is
//! another, so anything that belongs to both lives in `vb-pipeline`, not here.
//! This layer only marshals strings and catches panics.
//!
//! # Two rules for every exported function
//!
//! **Nothing unwinds across the boundary.** A Rust panic crossing into Dart is
//! undefined behaviour, so every entry point is wrapped in
//! `catch_unwind` and turns a panic into an error result.
//!
//! **The caller frees what we allocate.** Every returned string comes from
//! Rust's allocator and must go back to it through [`vb_free_string`]; freeing
//! it on the Dart side would corrupt the heap.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// What every call returns, as JSON.
///
/// A tagged result rather than a null-on-error convention, so the Dart side
/// gets the reason a run failed instead of only that it did.
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum FfiResult {
    Ok { data: serde_json::Value },
    Error { message: String },
}

impl FfiResult {
    fn ok(data: serde_json::Value) -> Self {
        FfiResult::Ok { data }
    }

    fn error(message: impl Into<String>) -> Self {
        FfiResult::Error {
            message: message.into(),
        }
    }

    /// Serialise into a freshly allocated C string.
    ///
    /// If even serialising the result fails there is nothing useful left to
    /// say, so a fixed JSON literal is returned rather than a null pointer the
    /// caller would have to special-case.
    fn into_c_string(self) -> *mut c_char {
        let text = serde_json::to_string(&self).unwrap_or_else(|_| {
            r#"{"status":"error","message":"could not serialise the result"}"#.to_string()
        });
        match CString::new(text) {
            Ok(s) => s.into_raw(),
            Err(_) => CString::new(r#"{"status":"error","message":"result contained a nul byte"}"#)
                .expect("literal has no nul byte")
                .into_raw(),
        }
    }
}

/// Read a C string argument, rejecting null and invalid UTF-8.
///
/// # Safety
/// `ptr` must be null or a valid nul-terminated C string.
unsafe fn read_str<'a>(ptr: *const c_char, name: &str) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("{name} must not be null"));
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| format!("{name} is not valid UTF-8"))
}

/// Wrap a call so no panic escapes into the caller's runtime.
fn guarded(f: impl FnOnce() -> FfiResult) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        FfiResult::error(format!("the engine panicked: {detail}"))
    });
    result.into_c_string()
}

/// The engine version.
///
/// # Safety
/// The returned string must be freed with [`vb_free_string`].
#[unsafe(no_mangle)]
pub extern "C" fn vb_version() -> *mut c_char {
    guarded(|| FfiResult::ok(vb_pipeline::api::version()))
}

/// Report which tools and adapters are available, without running anything.
///
/// # Safety
/// The returned string must be freed with [`vb_free_string`].
#[unsafe(no_mangle)]
pub extern "C" fn vb_doctor() -> *mut c_char {
    guarded(|| FfiResult::ok(vb_pipeline::api::doctor()))
}

/// List the templates in the prior store.
///
/// # Safety
/// The returned string must be freed with [`vb_free_string`].
#[unsafe(no_mangle)]
pub extern "C" fn vb_templates() -> *mut c_char {
    guarded(|| FfiResult::ok(vb_pipeline::api::templates()))
}

/// List directories and video files under a path.
///
/// The engine owns file browsing so that one picker works on every platform,
/// including the ones where the UI runs in a browser and cannot see the
/// filesystem at all.
///
/// # Safety
/// `dir` may be null, meaning the user's home directory. The returned string
/// must be freed with [`vb_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vb_browse(dir: *const c_char) -> *mut c_char {
    guarded(|| {
        let dir = if dir.is_null() {
            None
        } else {
            match unsafe { read_str(dir, "dir") } {
                Ok(s) => Some(s),
                Err(e) => return FfiResult::error(e),
            }
        };
        FfiResult::ok(vb_pipeline::api::browse(dir))
    })
}

/// Analyse a video and write a draft.
///
/// Returns JSON describing the run, or an error. Blocking: the caller is
/// expected to run it off the UI thread, which is the whole reason the engine
/// is native rather than in Dart.
///
/// `input_paths` is a JSON array of paths -- a project is several clips or a
/// whole folder, not one file -- and a folder among them is expanded by the
/// engine. A bare path is accepted too, so one video needs no JSON.
///
/// # Safety
/// `input_paths` and `project_dir` must be valid nul-terminated C strings.
/// `template_id` may be null to select a template automatically. The returned
/// string must be freed with [`vb_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vb_run(
    input_paths: *const c_char,
    project_dir: *const c_char,
    template_id: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let input = match unsafe { read_str(input_paths, "input_paths") } {
            Ok(s) => s,
            Err(e) => return FfiResult::error(e),
        };
        let dir = match unsafe { read_str(project_dir, "project_dir") } {
            Ok(s) => s,
            Err(e) => return FfiResult::error(e),
        };
        // A null template is a request to choose one, not an error.
        let template = if template_id.is_null() {
            None
        } else {
            match unsafe { read_str(template_id, "template_id") } {
                Ok(s) => Some(s),
                Err(e) => return FfiResult::error(e),
            }
        };

        let inputs = match parse_inputs(input) {
            Ok(inputs) => inputs,
            Err(e) => return FfiResult::error(e),
        };

        match vb_pipeline::api::run(&inputs, Some(dir), template) {
            Ok(value) => FfiResult::ok(value),
            Err(message) => FfiResult::error(message),
        }
    })
}

/// Call any operation by name, with its arguments as a JSON object.
///
/// Every operation added since the first four goes through here rather than
/// through a C function of its own: projects, runs with progress, the model
/// library, settings, the album, the self-test. The operation set grows in
/// `vb_pipeline::api::call`, and this boundary does not change with it --
/// which is what keeps the app's link line, and its list of symbols that must
/// survive dead-stripping, from growing with every feature.
///
/// Operations that take time (`run.start`, `models.download`) return a job
/// id at once; `run.status` reports on it and `run.cancel` stops it.
///
/// # Safety
/// `method` must be a valid nul-terminated C string. `args_json` may be null,
/// meaning no arguments. The returned string must be freed with
/// [`vb_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vb_call(method: *const c_char, args_json: *const c_char) -> *mut c_char {
    guarded(|| {
        let method = match unsafe { read_str(method, "method") } {
            Ok(s) => s,
            Err(e) => return FfiResult::error(e),
        };
        let args: serde_json::Value = if args_json.is_null() {
            serde_json::Value::Object(Default::default())
        } else {
            let text = match unsafe { read_str(args_json, "args_json") } {
                Ok(s) => s,
                Err(e) => return FfiResult::error(e),
            };
            if text.trim().is_empty() {
                serde_json::Value::Object(Default::default())
            } else {
                match serde_json::from_str(text) {
                    Ok(value) => value,
                    Err(e) => {
                        return FfiResult::error(format!("the arguments are not JSON: {e}"));
                    }
                }
            }
        };
        match vb_pipeline::api::call(method, &args) {
            Ok(value) => FfiResult::ok(value),
            Err(message) => FfiResult::error(message),
        }
    })
}

/// Read the input list: a JSON array of paths, or a single bare path.
///
/// Accepting both keeps one C function for one video and for a folder of
/// forty, and a caller that has never heard of JSON can still pass a path.
fn parse_inputs(raw: &str) -> Result<Vec<String>, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('[') {
        return Ok(vec![raw.to_string()]);
    }
    let parsed: Vec<String> = serde_json::from_str(trimmed)
        .map_err(|e| format!("input_paths is not a JSON array of paths: {e}"))?;
    if parsed.is_empty() {
        return Err("input_paths is an empty list".to_string());
    }
    Ok(parsed)
}

/// Free a string returned by any function in this module.
///
/// # Safety
/// `ptr` must have come from this module and must not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vb_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // Reclaim the allocation Rust made in `into_c_string`.
    drop(unsafe { CString::from_raw(ptr) });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read and free a returned string, as a well-behaved caller would.
    fn take(ptr: *mut c_char) -> serde_json::Value {
        assert!(!ptr.is_null(), "no function here may return null");
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { vb_free_string(ptr) };
        serde_json::from_str(&text).expect("every result must be valid JSON")
    }

    #[test]
    fn version_reports_both_contract_versions() {
        let v = take(vb_version());
        assert_eq!(v["status"], "ok");
        assert_eq!(v["data"]["footageGraph"], "footage-graph/0.1");
        assert_eq!(v["data"]["editingDsl"], "editing-dsl/0.1");
    }

    #[test]
    fn doctor_reports_without_running_anything() {
        let v = take(vb_doctor());
        assert_eq!(v["status"], "ok");
        assert_eq!(v["data"]["requiresNetwork"], false);
    }

    #[test]
    fn templates_come_back_as_a_list() {
        let v = take(vb_templates());
        assert_eq!(v["status"], "ok");
        assert!(v["data"].as_array().unwrap().len() >= 9);
    }

    #[test]
    fn a_null_argument_is_an_error_not_a_crash() {
        let v = take(unsafe { vb_run(std::ptr::null(), std::ptr::null(), std::ptr::null()) });
        assert_eq!(v["status"], "error");
        assert!(v["message"].as_str().unwrap().contains("must not be null"));
    }

    #[test]
    fn a_missing_input_file_is_reported_rather_than_panicking() {
        let input = CString::new("/definitely/not/here.mp4").unwrap();
        let dir = CString::new("/tmp/vb-ffi-test").unwrap();
        let v = take(unsafe { vb_run(input.as_ptr(), dir.as_ptr(), std::ptr::null()) });
        assert_eq!(v["status"], "error");
        let message = v["message"].as_str().unwrap();
        assert!(
            message.contains("does not exist"),
            "the error should name the problem, got: {message}"
        );
    }

    #[test]
    fn the_project_directory_argument_is_not_mistaken_for_the_template() {
        // Both are Option<&str>, so swapping them type-checks and would
        // silently write output to a directory named after a template.
        let sample =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
        if !sample.exists() || !vb_media::default_backend().is_available() {
            return;
        }
        let dir = std::env::temp_dir().join("vb-ffi-argorder");
        let _ = std::fs::remove_dir_all(&dir);

        let input = CString::new(sample.to_string_lossy().as_ref()).unwrap();
        let project = CString::new(dir.to_string_lossy().as_ref()).unwrap();
        let v = take(unsafe { vb_run(input.as_ptr(), project.as_ptr(), std::ptr::null()) });

        assert_eq!(v["status"], "ok", "run failed: {}", v["message"]);
        let written = v["data"]["projectDir"].as_str().unwrap();
        assert!(
            written.contains("vb-ffi-argorder"),
            "output went to {written}, not the requested project directory"
        );
    }

    #[test]
    fn any_operation_is_reachable_by_name() {
        let method = CString::new("version").unwrap();
        let v = take(unsafe { vb_call(method.as_ptr(), std::ptr::null()) });
        assert_eq!(v["status"], "ok");
        assert_eq!(v["data"]["editingDsl"], "editing-dsl/0.1");
    }

    #[test]
    fn an_unknown_operation_is_an_error_that_names_it() {
        let method = CString::new("no.such.thing").unwrap();
        let v = take(unsafe { vb_call(method.as_ptr(), std::ptr::null()) });
        assert_eq!(v["status"], "error");
        assert!(v["message"].as_str().unwrap().contains("no.such.thing"));
    }

    #[test]
    fn arguments_that_are_not_json_are_an_error_not_a_crash() {
        let method = CString::new("browse").unwrap();
        let args = CString::new("{ not json").unwrap();
        let v = take(unsafe { vb_call(method.as_ptr(), args.as_ptr()) });
        assert_eq!(v["status"], "error");
        assert!(v["message"].as_str().unwrap().contains("not JSON"));
    }

    #[test]
    fn a_run_starts_as_a_job_and_reports_on_it() {
        // The whole point of the job model: the call returns at once with an
        // id, and the status says what failed rather than the call blocking.
        let method = CString::new("run.start").unwrap();
        let args = CString::new(r#"{"inputs":["/definitely/not/here.mp4"]}"#).unwrap();
        let started = take(unsafe { vb_call(method.as_ptr(), args.as_ptr()) });
        assert_eq!(started["status"], "ok", "{started}");
        let job = started["data"]["jobId"].as_str().unwrap().to_string();

        let status = CString::new("run.status").unwrap();
        let args = CString::new(format!(r#"{{"jobId":"{job}"}}"#)).unwrap();
        let mut last = serde_json::Value::Null;
        for _ in 0..200 {
            last = take(unsafe { vb_call(status.as_ptr(), args.as_ptr()) });
            if last["data"]["finished"] == true {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(last["data"]["state"], "failed", "{last}");
        assert!(
            last["data"]["error"]
                .as_str()
                .unwrap()
                .contains("does not exist")
        );
    }

    #[test]
    fn freeing_null_is_harmless() {
        // Dart will do this on any error path; it must not fault.
        unsafe { vb_free_string(std::ptr::null_mut()) };
    }

    #[test]
    fn a_panic_becomes_an_error_instead_of_unwinding_into_the_caller() {
        // Unwinding across the C boundary is undefined behaviour, so the guard
        // has to hold for every entry point.
        let result = guarded(|| panic!("boom"));
        let v = take(result);
        assert_eq!(v["status"], "error");
        assert!(v["message"].as_str().unwrap().contains("boom"));
    }
}

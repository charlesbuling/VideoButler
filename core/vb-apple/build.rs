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

//! Compiles the Vision shim on Apple platforms, and nothing anywhere else.
//!
//! The crate is in the workspace on every platform so that `cargo check`
//! covers it, but the Objective-C and the frameworks only exist here.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let apple = target.contains("apple");
    println!("cargo:rustc-check-cfg=cfg(vb_vision)");
    if !apple {
        return;
    }

    println!("cargo:rerun-if-changed=csrc/vb_vision.m");
    println!("cargo:rerun-if-changed=csrc/vb_power.m");
    cc::Build::new()
        .file("csrc/vb_vision.m")
        .file("csrc/vb_power.m")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .warnings(true)
        .compile("vb_vision");

    // Vision does the work; Core Image crops the thumbnails; Foundation is
    // the objects both speak in.
    for framework in ["Vision", "CoreImage", "Foundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rustc-cfg=vb_vision");
}

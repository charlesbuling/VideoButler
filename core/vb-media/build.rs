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

//! Compiles the libav shim when the `libav` feature is on, and does nothing
//! otherwise.
//!
//! libav is found through pkg-config, which Homebrew, every Linux
//! distribution and an ffmpeg `make install` all provide. Cross builds, where
//! pkg-config is more trouble than it is worth, name an install prefix
//! instead: headers under `include/`, shared libraries under `lib/`.
//!
//! The prefix comes from `VB_LIBAV_DIR_<target>` (for example
//! `VB_LIBAV_DIR_aarch64_linux_android`), falling back to `VB_LIBAV_DIR`. The
//! per-target form is what lets one `cargo ndk` invocation build every ABI,
//! each against its own libraries -- the convention pkg-config uses too.

fn main() {
    #[cfg(feature = "libav")]
    libav::build();
}

#[cfg(feature = "libav")]
mod libav {
    use std::path::PathBuf;

    /// The libraries the shim calls, dependents first, with the oldest
    /// version it builds against: ffmpeg 6.0.
    ///
    /// Order matters on the link line: a library must precede the ones it
    /// depends on. libswresample is here because the shim decodes audio to
    /// mono for beat detection and speech recognition, and the downmix and
    /// resample are its job.
    const LIBS: [(&str, &str); 6] = [
        ("libavfilter", "9"),
        ("libavformat", "60"),
        ("libavcodec", "60"),
        ("libswscale", "7"),
        ("libswresample", "4"),
        ("libavutil", "58"),
    ];

    pub fn build() {
        println!("cargo:rerun-if-changed=csrc/vb_av.c");
        println!("cargo:rerun-if-changed=csrc/vb_av.h");
        let target = std::env::var("TARGET").unwrap_or_default();
        let per_target = format!("VB_LIBAV_DIR_{}", target.replace('-', "_"));
        println!("cargo:rerun-if-env-changed={per_target}");
        println!("cargo:rerun-if-env-changed=VB_LIBAV_DIR");

        let mut shim = cc::Build::new();
        shim.file("csrc/vb_av.c")
            .include("csrc")
            .std("c11")
            .warnings(true)
            .extra_warnings(true);

        let mut search: Vec<PathBuf> = Vec::new();
        let mut libs: Vec<String> = Vec::new();

        let prefix = std::env::var_os(&per_target).or_else(|| std::env::var_os("VB_LIBAV_DIR"));
        match prefix {
            Some(prefix) => {
                let prefix = PathBuf::from(prefix);
                shim.include(prefix.join("include"));
                search.push(prefix.join("lib"));
                libs.extend(LIBS.iter().map(|(name, _)| name["lib".len()..].to_string()));
            }
            None => {
                for (name, min) in LIBS {
                    let found = pkg_config::Config::new()
                        .atleast_version(min)
                        // Emitted by hand below, after the shim, so the link
                        // order is the shim first and libav after it.
                        .cargo_metadata(false)
                        .probe(name)
                        .unwrap_or_else(|e| {
                            panic!(
                                "the libav feature needs {name} {min} or newer: {e}\n\
                                 Install ffmpeg's development files (macOS: brew install \
                                 ffmpeg; Debian: apt install libavformat-dev libavfilter-dev \
                                 libswscale-dev), or set VB_LIBAV_DIR to an install prefix."
                            )
                        });
                    for dir in found.include_paths {
                        shim.include(dir);
                    }
                    search.extend(found.link_paths);
                    libs.extend(found.libs);
                }
            }
        }

        shim.compile("vb_av");

        search.dedup();
        for dir in search {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
        for lib in libs {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}

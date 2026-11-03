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

//! The model host's ONNX Runtime engine.
//!
//! The model host runs every model the app carries, open or trained by this
//! project, and does inference only -- training happens on the development
//! side and arrives here as a file. It has two engines: llama.cpp for VLMs
//! and LLMs, and this one for everything else. Each model is an impl of a
//! `vb-infer` trait, so the pipeline never names ONNX Runtime.
//!
//! ONNX Runtime is loaded at runtime from a library the app carries, never
//! linked, and never downloaded by the build (see [`runtime`]).

pub mod depth;
pub mod faceid;
pub mod faces;
pub mod labels;
pub mod people;
pub mod pose;
pub mod runtime;

pub use depth::DepthAnything;
pub use faceid::AuraFace;
pub use faces::YuNet;
pub use labels::SiglipLabels;
pub use people::PpHumanSeg;
pub use pose::RtmoPose;
pub use runtime::library_path;

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

//! The model library: weights the app downloads once and every project uses.
//!
//! Models live in the library directory, not in a project, because that is
//! what makes "downloaded once" true. A user who downloads a speech model for
//! one holiday should never be asked for it again, for any project, and
//! deleting a project must not delete it.
//!
//! # What is in the catalogue, and what is not
//!
//! Only weights that can ship in a paid product. Every entry carries its
//! licence, and a non-commercial one does not belong here at all -- the same
//! rule that keeps CoTracker3 and MMAudio out of the dependency list.
//!
//! Every entry also carries the size and the SHA-256 that Hugging Face
//! advertises for that file. A wrong digest would reject a good download, so
//! these are read from the service rather than remembered, and the download
//! checks what arrives against both the catalogue's digest and the one the
//! server sends.

use crate::library::Library;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

/// What a model does in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Transcribes speech, behind `AsrAdapter`.
    Speech,
    /// Reads how far each part of a frame is, behind `DepthAdapter`: the
    /// director's way of finding what blocks the view.
    Depth,
    /// Finds faces in a frame, behind `FaceDetector`.
    Faces,
    /// Finds people in a frame, behind `PersonSegmenter`.
    People,
    /// Rates the composition of a cut's shortlist, behind `VlmAdapter`.
    Composition,
    /// Says in a sentence what a moment shows, behind `VlmAdapter`: the
    /// captions on a cut.
    Describe,
    /// Finds people and how they stand, behind `PoseEstimator`: which
    /// moments are dancing, and where a dance cut can join two of them.
    Pose,
    /// Names a shot's place, people and activity, behind `VlmAdapter`:
    /// scene labels where the platform has no Apple Vision.
    Labels,
    /// Says whose face a face is, behind `TrackerAdapter`: the album's and
    /// the video butler's people, grouped across every video.
    FaceId,
}

impl Role {
    pub fn id(self) -> &'static str {
        match self {
            Role::Speech => "speech",
            Role::Depth => "depth",
            Role::Faces => "faces",
            Role::People => "people",
            Role::Composition => "composition",
            Role::Describe => "describe",
            Role::Pose => "pose",
            Role::Labels => "labels",
            Role::FaceId => "faceid",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Speech => "Speech",
            Role::Depth => "Depth",
            Role::Faces => "Faces",
            Role::People => "People",
            Role::Composition => "Composition",
            Role::Describe => "Captions",
            Role::Pose => "Body pose",
            Role::Labels => "Scene labels",
            Role::FaceId => "Face grouping",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Role::Speech => "语音",
            Role::Depth => "景深",
            Role::Faces => "人脸",
            Role::People => "人像",
            Role::Composition => "构图评分",
            Role::Describe => "画面描述",
            Role::Pose => "人体姿态",
            Role::Labels => "场景标签",
            Role::FaceId => "人脸分组",
        }
    }
}

/// A second file a model needs beside its weights: a vision-language
/// model's projector, which turns a picture into what the language model
/// reads.
#[derive(Debug, Clone)]
pub struct ModelFile {
    pub file_name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// One downloadable model.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub role: Role,
    /// The file as it is stored in the library.
    pub file_name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    /// One line on when to choose this one over its siblings.
    pub note: &'static str,
    pub note_zh: &'static str,
    /// Roughly how much memory it needs while running.
    pub memory_mb: u32,
    /// How capable this one is, within its role. Stated rather than inferred
    /// from the file size: a quantised base model is smaller than tiny and
    /// better than it, so size ranks them backwards.
    pub rank: u8,
    /// The projector a vision-language model needs, downloaded with it.
    pub projector: Option<ModelFile>,
    /// What provenance records when it runs, for a model the engine loads
    /// by name rather than through an adapter of its own.
    pub adapter: &'static str,
}

/// The models this build offers.
///
/// whisper.cpp's ggml conversions of OpenAI Whisper, which is MIT and
/// multilingual -- Chinese included, which matters here -- and the three
/// small vision models the director reads framing with, run on ONNX Runtime.
/// Sizes and digests were read from the files on Hugging Face.
pub const CATALOGUE: [ModelSpec; 14] = [
    ModelSpec {
        id: "whisper-tiny",
        name: "Whisper tiny",
        name_zh: "Whisper 微型",
        role: Role::Speech,
        file_name: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        license: "MIT",
        note: "Fastest and least accurate. Good for trying the speech path out.",
        note_zh: "最快、最不准，适合先试一试语音识别。",
        memory_mb: 390,
        rank: 1,
        projector: None,
        adapter: "",
    },
    ModelSpec {
        id: "whisper-base-q5",
        name: "Whisper base (quantised)",
        name_zh: "Whisper 基础版（量化）",
        role: Role::Speech,
        file_name: "ggml-base-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        bytes: 59_707_625,
        sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
        license: "MIT",
        note: "Small and quick. A sensible first download.",
        note_zh: "小而快，第一次下载选它就好。",
        memory_mb: 500,
        rank: 2,
        projector: None,
        adapter: "",
    },
    ModelSpec {
        id: "whisper-base",
        name: "Whisper base",
        name_zh: "Whisper 基础版",
        role: Role::Speech,
        file_name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        license: "MIT",
        note: "The unquantised base model, a little more accurate.",
        note_zh: "未量化的基础版，稍微准一点。",
        memory_mb: 500,
        rank: 3,
        projector: None,
        adapter: "",
    },
    ModelSpec {
        id: "whisper-small-q5",
        name: "Whisper small (quantised)",
        name_zh: "Whisper 小型（量化）",
        role: Role::Speech,
        file_name: "ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        bytes: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        license: "MIT",
        note: "Noticeably better on accented and noisy speech. The one to keep.",
        note_zh: "口音和嘈杂环境下明显更准，值得长期留着。",
        memory_mb: 1_000,
        rank: 4,
        projector: None,
        adapter: "",
    },
    ModelSpec {
        id: "whisper-small",
        name: "Whisper small",
        name_zh: "Whisper 小型",
        role: Role::Speech,
        file_name: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        license: "MIT",
        note: "Most accurate here, and the slowest to run.",
        note_zh: "这里最准的一个，也最慢。",
        memory_mb: 1_000,
        rank: 5,
        projector: None,
        adapter: "",
    },
    // Depth Anything V2 at its Small size, the only size that is Apache-2.0:
    // Base and Large are CC-BY-NC and cannot ship.
    ModelSpec {
        id: "depth-anything-v2-small",
        name: "Depth Anything V2 Small",
        name_zh: "Depth Anything V2 小型",
        role: Role::Depth,
        file_name: "depth-anything-v2-small.onnx",
        url: "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx",
        bytes: 99_060_839,
        sha256: "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c",
        license: "Apache-2.0",
        note: "Sees what is right against the lens -- a car door, a dashboard, a sleeve -- so \
               those shots go to the back of the queue.",
        note_zh: "看出紧贴镜头的东西（车门、仪表台、衣袖），把被挡住的画面排到后面。",
        memory_mb: 400,
        rank: 1,
        projector: None,
        adapter: "",
    },
    // OpenCV's own face detector. The development doc names MediaPipe's,
    // which is published only as TFLite; this one is ONNX as released.
    ModelSpec {
        id: "yunet-2023mar",
        name: "YuNet face detector",
        name_zh: "YuNet 人脸检测",
        role: Role::Faces,
        file_name: "face_detection_yunet_2023mar.onnx",
        url: "https://huggingface.co/opencv/face_detection_yunet/resolve/main/face_detection_yunet_2023mar.onnx",
        bytes: 232_589,
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        license: "MIT",
        note: "Tells a person close to the lens from something blocking it.",
        note_zh: "分清离镜头很近的是人，还是挡住镜头的东西。",
        memory_mb: 50,
        rank: 1,
        projector: None,
        adapter: "",
    },
    // fal's AuraFace v1: glintr100.onnx alone, Apache-2.0. The other files
    // of that repository are InsightFace's research-only packs and are never
    // fetched. Pinned to a revision, read from Hugging Face on 2026-10-07.
    ModelSpec {
        id: "auraface-v1",
        name: "AuraFace face grouping",
        name_zh: "AuraFace 人脸分组",
        role: Role::FaceId,
        file_name: "auraface-v1-glintr100.onnx",
        url: "https://huggingface.co/fal/AuraFace-v1/resolve/af6d057c9b0ec4071d4c49c80e3539258798b609/glintr100.onnx",
        bytes: 260_694_151,
        sha256: "a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60",
        license: "Apache-2.0",
        note: "Groups faces by who they are, across every video: the album's people \
               and the video butler's. Needs the YuNet face detector.",
        note_zh: "按是谁给人脸分组，跨所有视频：人物分组和视频管家都用它。需要同时装 YuNet 人脸检测。",
        memory_mb: 600,
        rank: 1,
        projector: None,
        adapter: "",
    },
    // PaddlePaddle's portrait segmentation, from OpenCV's model zoo.
    ModelSpec {
        id: "pphumanseg-2023mar",
        name: "PP-HumanSeg person mask",
        name_zh: "PP-HumanSeg 人像分割",
        role: Role::People,
        file_name: "human_segmentation_pphumanseg_2023mar.onnx",
        url: "https://huggingface.co/opencv/human_segmentation_pphumanseg/resolve/main/human_segmentation_pphumanseg_2023mar.onnx",
        bytes: 6_163_938,
        sha256: "552d8a984054e59b5d773d24b9b12022b22046ceb2bbc4c9aaeaceb36a9ddf24",
        license: "Apache-2.0",
        note: "Finds people whose faces are hidden by masks, sunglasses or hoods, so a \
               selfie is not taken for an obstruction.",
        note_zh: "认出被口罩、墨镜、帽兜遮住脸的人，自拍就不会被当成遮挡。",
        memory_mb: 50,
        rank: 1,
        projector: None,
        adapter: "",
    },
    // The development doc's general VLM, at the size for the reference Mac,
    // as Qwen published it in GGUF: the language model at Q4_K_M and its
    // vision projector at full precision, which is what reads the picture.
    ModelSpec {
        id: "qwen3-vl-8b-q4",
        name: "Qwen3-VL 8B",
        name_zh: "Qwen3-VL 8B",
        role: Role::Describe,
        file_name: "qwen3-vl-8b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/Qwen3VL-8B-Instruct-Q4_K_M.gguf",
        bytes: 5_027_784_800,
        sha256: "67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2",
        license: "Apache-2.0",
        note: "Looks at each moment the cut uses and writes a caption for it in your \
               language, from the picture rather than from what was said.",
        note_zh: "看成片用到的每个画面，按画面内容写一句字幕，不再用语音转文字。",
        memory_mb: 7_000,
        rank: 3,
        projector: Some(ModelFile {
            file_name: "qwen3-vl-8b-instruct-mmproj-f16.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-8B-Instruct-F16.gguf",
            bytes: 1_159_029_824,
            sha256: "ca524100ebf825c9a870db1c580d03879e0da0ab2541697e2458e64891cf9d38",
        }),
        adapter: "Qwen3-VL-8B",
    },
    // The same model family for devices with less memory, as the doc's tiers
    // give it: 4B for a phone around 8 GB, 2B below that. Chosen by what the
    // device can hold (`fits`), the largest that fits.
    ModelSpec {
        id: "qwen3-vl-4b-q4",
        name: "Qwen3-VL 4B",
        name_zh: "Qwen3-VL 4B",
        role: Role::Describe,
        file_name: "qwen3-vl-4b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        bytes: 2_497_281_664,
        sha256: "66358cb18bb6b3b1b6675aa412c7a88ef01d228f481184d13668e5201c730a0a",
        license: "Apache-2.0",
        note: "Captions from the picture, for a phone or tablet with about 8 GB of memory.",
        note_zh: "按画面内容写字幕，适合 8GB 左右内存的手机或平板。",
        memory_mb: 4_500,
        rank: 2,
        projector: Some(ModelFile {
            file_name: "qwen3-vl-4b-instruct-mmproj-f16.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-4B-Instruct-F16.gguf",
            bytes: 836_180_256,
            sha256: "256f3a43bd4205ffef48d6b92715e1e70b5b0e9aef06522584967513a9985331",
        }),
        adapter: "Qwen3-VL-4B",
    },
    ModelSpec {
        id: "qwen3-vl-2b-q4",
        name: "Qwen3-VL 2B",
        name_zh: "Qwen3-VL 2B",
        role: Role::Describe,
        file_name: "qwen3-vl-2b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF/resolve/main/Qwen3VL-2B-Instruct-Q4_K_M.gguf",
        bytes: 1_107_409_952,
        sha256: "089d75c52f4b7ffc56ba998ffc50aae89fcafc755f9e7208aacca281dca6c2ae",
        license: "Apache-2.0",
        note: "Captions from the picture on a phone with little memory; simpler sentences.",
        note_zh: "按画面内容写字幕，适合内存较小的手机；句子简单一些。",
        memory_mb: 2_800,
        rank: 1,
        projector: Some(ModelFile {
            file_name: "qwen3-vl-2b-instruct-mmproj-f16.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-2B-Instruct-F16.gguf",
            bytes: 819_394_848,
            sha256: "c3d5afbef5287953acd57b4043d2269456e5761a4eaccb3b71b062996970aea5",
        }),
        adapter: "Qwen3-VL-2B",
    },
    // RTMO-s, the one-stage multi-person model of OpenMMLab's RTMPose
    // family: people and their seventeen body keypoints from one pass, with
    // no person detector to choose beside it. ONNX as exported for
    // Transformers.js, Apache-2.0 like the original.
    ModelSpec {
        id: "rtmo-s",
        name: "RTMO-s body pose",
        name_zh: "RTMO-s 人体姿态",
        role: Role::Pose,
        file_name: "rtmo-s.onnx",
        url: "https://huggingface.co/Xenova/RTMO-s/resolve/main/onnx/model.onnx",
        bytes: 39_636_400,
        sha256: "1cd6a3517658903233e9ccb78666554862f57f0944b6886e2048093594aef4e7",
        license: "Apache-2.0",
        note: "Finds the people in a frame and how each is standing: which moments are \
               dancing, how many dance, and where one dance clip can flow into the next.",
        note_zh: "找出画面里的人和每个人的姿势：哪些是在跳舞、几个人在跳，以及两段舞蹈从哪里接上动作才连贯。",
        memory_mb: 200,
        rank: 1,
        projector: None,
        adapter: "",
    },
    // Google's SigLIP 2 (Apache-2.0), the image half as onnx-community
    // exported it, in half precision. The development doc names it for scene
    // similarity; here it names scenes where there is no Apple Vision,
    // against the label table vb-onnx carries. Pinned to the revision that
    // table was embedded with (scripts/make-scene-labels.sh). Half precision
    // rather than the export's int8, which is half the size again but
    // answered differently on Android than on a Mac -- a horse ride read as
    // nothing, a lake as a desert -- where this one answers as full
    // precision does on both.
    ModelSpec {
        id: "siglip2-base-fp16",
        name: "SigLIP 2 scene labels",
        name_zh: "SigLIP 2 场景标签",
        role: Role::Labels,
        file_name: "siglip2-base-patch16-224-vision-fp16.onnx",
        url: "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/ba1f3b0843f24bc5417d38e19c37b287d719b2f4/onnx/vision_model_fp16.onnx",
        bytes: 186_039_516,
        sha256: "a1959f7bd3993a607e48839f6d01e25b876fe76afda301b028b78eef68aabd95",
        license: "Apache-2.0",
        note: "Names each shot's place, people and what they are doing -- \"lake, couple, \
               dancing\" -- on a device without Apple Vision.",
        note_zh: "在没有 Apple Vision 的设备上，给每个镜头标出地点、人物和在做什么，例如“湖边、情侣、跳舞”。",
        memory_mb: 500,
        rank: 1,
        projector: None,
        adapter: "",
    },
];

/// A model this build uses but does not yet offer for download.
///
/// PhotoFramer is published as full-precision weights; the app runs it as a
/// GGUF that `scripts/convert-photoframer.sh` makes on the development side.
/// Until the converted files are hosted, they are placed in the library by
/// hand, and the listing says so rather than offering a download that does
/// not exist.
#[derive(Debug, Clone)]
pub struct LocalModel {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    /// What provenance records when it runs.
    pub adapter: &'static str,
    pub role: Role,
    /// The files by name in the library: the language model, then its
    /// vision projector.
    pub files: [&'static str; 2],
    pub license: &'static str,
    pub note: &'static str,
    pub note_zh: &'static str,
    pub memory_mb: u32,
}

pub const LOCAL: [LocalModel; 1] = [LocalModel {
    id: "photoframer-7b-q4",
    name: "PhotoFramer composition model (7B)",
    name_zh: "PhotoFramer 构图评分（7B）",
    adapter: "PhotoFramer-7B",
    role: Role::Composition,
    files: [
        "photoframer-7b-q4_k_m.gguf",
        "photoframer-7b-mmproj-f16.gguf",
    ],
    license: "MIT",
    note: "Rates the composition of the moments a cut might use, and the cut is made again \
           with the ratings. Converted on the development side; not yet offered for download.",
    note_zh: "给候选画面的构图打分，再按分数重新选片。由开发端转换生成，暂不提供下载。",
    memory_mb: 6_500,
}];

/// Where a local model's files sit.
pub fn local_paths(library: &Library, model: &LocalModel) -> [PathBuf; 2] {
    model.files.map(|f| file_path(library, f))
}

/// Models that came with the app, read only: an installer for a machine
/// with no network carries them. In a macOS bundle's `Resources/models`, or
/// a `models` folder beside the binary elsewhere; `VB_BUNDLED_MODELS` names
/// another -- on Android the app's OBB folder, where its installer puts
/// them, since an APK cannot hold them.
pub fn bundled_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("VB_BUNDLED_MODELS").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../Resources/models"), dir.join("models")]
        .into_iter()
        .find(|d| d.is_dir())
}

/// Where a model file is: the library's own copy first -- a download or an
/// import -- then the one the app came with. The library's path when
/// neither has it, which is where a download would put it.
pub fn file_path(library: &Library, name: &str) -> PathBuf {
    file_path_in(library, name, bundled_dir().as_deref())
}

fn file_path_in(library: &Library, name: &str, bundled: Option<&std::path::Path>) -> PathBuf {
    let own = library.models_dir().join(name);
    if own.is_file() {
        return own;
    }
    bundled
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
        .unwrap_or(own)
}

/// Is every file of the model one the app came with? Those cannot be
/// deleted from inside the app.
pub fn is_bundled(library: &Library, model: &ModelSpec) -> bool {
    is_bundled_in(library, model, bundled_dir().as_deref())
}

fn is_bundled_in(library: &Library, model: &ModelSpec, bundled: Option<&std::path::Path>) -> bool {
    let Some(dir) = bundled else {
        return false;
    };
    files_of(model).iter().all(|f| {
        !library.models_dir().join(f.file_name).is_file() && dir.join(f.file_name).is_file()
    })
}

pub fn local_installed(library: &Library, model: &LocalModel) -> bool {
    local_paths(library, model).iter().all(|p| p.is_file())
}

/// The installed local model for a role, if any, when this device can hold
/// it: a 7B judge on an 8 GB phone would close the app, not slow it.
pub fn local_for(library: &Library, role: Role) -> Option<&'static LocalModel> {
    LOCAL
        .iter()
        .find(|m| m.role == role && local_installed(library, m) && local_fits(m))
}

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOGUE.iter().find(|m| m.id == id)
}

/// Where a model's file sits once installed.
pub fn path_of(library: &Library, model: &ModelSpec) -> PathBuf {
    file_path(library, model.file_name)
}

/// Where its projector sits, for a model that has one.
pub fn projector_path(library: &Library, model: &ModelSpec) -> Option<PathBuf> {
    model
        .projector
        .as_ref()
        .map(|p| file_path(library, p.file_name))
}

/// Every file a model is made of, with what each should be: the weights
/// first.
pub fn files_of(model: &ModelSpec) -> Vec<ModelFile> {
    let mut out = vec![ModelFile {
        file_name: model.file_name,
        url: model.url,
        bytes: model.bytes,
        sha256: model.sha256,
    }];
    out.extend(model.projector.clone());
    out
}

/// Every file the catalogue knows that sits in `dir` or a folder directly
/// inside it, with what it should be. A local model's files are listed with
/// their size on disk, having no catalogue size to check against.
pub fn found_in(dir: &std::path::Path) -> Vec<(PathBuf, ModelFile)> {
    let mut known: Vec<ModelFile> = CATALOGUE.iter().flat_map(files_of).collect();
    for local in &LOCAL {
        for name in local.files {
            known.push(ModelFile {
                file_name: name,
                url: "",
                bytes: 0,
                sha256: "",
            });
        }
    }
    let mut dirs = vec![dir.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(dir) {
        dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    let mut out = Vec::new();
    for file in known {
        if let Some(path) = dirs
            .iter()
            .map(|d| d.join(file.file_name))
            .find(|p| p.is_file())
        {
            let bytes = if file.bytes > 0 {
                file.bytes
            } else {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            };
            out.push((path, ModelFile { bytes, ..file }));
        }
    }
    out
}

/// Copy a model file into the library, under a temporary name until it is
/// whole and, when the catalogue has a digest for it, matches it. A file
/// that does not is removed rather than installed.
pub fn copy_checked(
    from: &std::path::Path,
    dest: &std::path::Path,
    file: &ModelFile,
    progress: &dyn Fn(u64) -> bool,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    let partial = dest.with_extension("part");
    let mut input = std::fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut output =
        std::fs::File::create(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 4 << 20];
    let mut copied = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        copied += n as u64;
        if !progress(copied) {
            drop(output);
            let _ = std::fs::remove_file(&partial);
            return Err("cancelled".into());
        }
    }
    output.flush().map_err(|e| e.to_string())?;
    drop(output);
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let bad = (file.bytes > 0 && copied != file.bytes)
        || (!file.sha256.is_empty() && !digest.eq_ignore_ascii_case(file.sha256));
    if bad {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "{} is not the file the catalogue names (size or checksum differs)",
            file.file_name
        ));
    }
    std::fs::rename(&partial, dest).map_err(|e| format!("{}: {e}", dest.display()))
}

/// All of a model's bytes, projector included.
pub fn total_bytes(model: &ModelSpec) -> u64 {
    files_of(model).iter().map(|f| f.bytes).sum()
}

/// Is it there, and does it look complete?
///
/// The size is checked as well as the presence, because a download
/// interrupted by a lost connection leaves a file that exists and is wrong,
/// and the only thing worse than no speech model is one that fails halfway
/// through an hour of footage.
pub fn is_installed(library: &Library, model: &ModelSpec) -> bool {
    is_installed_in(library, model, bundled_dir().as_deref())
}

fn is_installed_in(
    library: &Library,
    model: &ModelSpec,
    bundled: Option<&std::path::Path>,
) -> bool {
    files_of(model).iter().all(|f| {
        std::fs::metadata(file_path_in(library, f.file_name, bundled))
            .map(|m| m.len() == f.bytes)
            .unwrap_or(false)
    })
}

/// The model a run uses for a role: the one the user chose, while it is
/// installed and this device can hold it, else the most capable installed
/// one that fits. `None` when there is none.
///
/// The choice matters once an installer brings every size: what came with
/// the app cannot be deleted, so without it the largest would always run.
pub fn installed_for(library: &Library, role: Role) -> Option<&'static ModelSpec> {
    let usable = |m: &ModelSpec| m.role == role && is_installed(library, m) && fits(m);
    let chosen = library
        .settings()
        .models
        .get(role.id())
        .and_then(|id| spec(id))
        .filter(|m| usable(m));
    chosen.or_else(|| {
        CATALOGUE
            .iter()
            .filter(|m| usable(m))
            .max_by_key(|m| m.rank)
    })
}

/// Use this model for its role from now on, in place of the most capable
/// one installed -- a smaller caption model for speed, say. It must be
/// installed and fit this device.
pub fn choose(library: &Library, id: &str, lang: crate::Lang) -> Result<(), String> {
    let model = spec(id).ok_or_else(|| format!("there is no model {id}"))?;
    if !is_installed(library, model) {
        return Err(match lang {
            crate::Lang::En => format!("{} is not installed", model.name),
            crate::Lang::Zh => format!("{} 还没有安装", model.name_zh),
        });
    }
    if !fits(model) {
        return Err(match lang {
            crate::Lang::En => {
                format!("This device does not have the memory to run {}", model.name)
            }
            crate::Lang::Zh => format!("这台设备内存不够，运行不了 {}", model.name_zh),
        });
    }
    let mut settings = library.settings();
    settings
        .models
        .insert(model.role.id().to_string(), model.id.to_string());
    library.save_settings(&settings)
}

/// This device's memory, in megabytes, where the platform says.
///
/// Read from `/proc/meminfo` on Linux and Android. Elsewhere unknown, and
/// every model is taken to fit: a Mac or a PC is the tier these models were
/// chosen for.
pub fn device_memory_mb() -> Option<u64> {
    if !cfg!(any(target_os = "linux", target_os = "android")) {
        return None;
    }
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Can this device hold the model while it runs? A model may take at most
/// three fifths of the memory: the app, the video decoder and the system
/// need the rest, and a phone that runs out closes the app rather than slow
/// it down.
///
/// The memory is the device's as it is sold -- 8, 12, 16 GB -- not the total
/// the kernel reports, which is a gigabyte or so less: the modem, the GPU
/// and the kernel's own reservations are taken before Linux counts. Judged
/// on the reported total, a 12 GB phone was refused the 8B, which is what
/// the doc's tier B (12-16 GB) runs. So 6 GB runs the 2B, 8 GB the 4B,
/// 12 GB the 8B and the composition judge. "RAM expansion" is swap on the
/// storage and does not count: a model's weights are read in full for every
/// word it writes, and must stay in memory to be read.
pub fn fits(model: &ModelSpec) -> bool {
    fits_in(model, device_memory_mb())
}

/// The same, for a model placed in the library by hand.
pub fn local_fits(model: &LocalModel) -> bool {
    needs_fit(model.memory_mb, device_memory_mb())
}

fn fits_in(model: &ModelSpec, memory_mb: Option<u64>) -> bool {
    needs_fit(model.memory_mb, memory_mb)
}

fn needs_fit(needs_mb: u32, memory_mb: Option<u64>) -> bool {
    memory_mb.is_none_or(|total| needs_mb as u64 * 5 <= sold_with_mb(total) * 3)
}

/// The memory a device is sold with, from the total its kernel reports: the
/// next size up that devices come in. No 10 GB, which almost nothing is
/// sold with, and which a 12 GB phone that reserves a lot would round to.
fn sold_with_mb(reported_mb: u64) -> u64 {
    const SIZES_GB: [u64; 13] = [1, 2, 3, 4, 6, 8, 12, 16, 18, 24, 32, 48, 64];
    SIZES_GB
        .iter()
        .map(|gb| gb * 1024)
        .find(|&mb| reported_mb <= mb)
        .unwrap_or(reported_mb)
}

/// Delete an installed model. The catalogue entry stays, so it can be
/// downloaded again.
pub fn remove(library: &Library, id: &str) -> Result<(), String> {
    // Only the library's own copies: what came with the app stays with it.
    if let Some(local) = LOCAL.iter().find(|m| m.id == id) {
        for path in local.files.map(|f| library.models_dir().join(f)) {
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        return Ok(());
    }
    let model = spec(id).ok_or_else(|| format!("there is no model {id}"))?;
    for file in files_of(model) {
        let path = library.models_dir().join(file.file_name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The catalogue as a front end sees it, with what is installed marked.
pub fn listing(library: &Library) -> Value {
    listing_in(library, crate::Lang::En)
}

pub fn listing_in(library: &Library, lang: crate::Lang) -> Value {
    json!({
        "directory": library.models_dir(),
        "models": CATALOGUE.iter().map(|m| json!({
            "id": m.id,
            "name": lang.t(m.name, m.name_zh),
            "role": m.role.id(),
            "roleLabel": lang.t(m.role.label(), m.role.label_zh()),
            "bytes": total_bytes(m),
            "license": m.license,
            "note": lang.t(m.note, m.note_zh),
            "memoryMb": m.memory_mb,
            "rank": m.rank,
            "installed": is_installed(library, m),
            "bundled": is_bundled(library, m),
            "downloadable": true,
            // Whether this device has the memory to run it; a model that
            // does not fit is listed, and not used.
            "fits": fits(m),
            // The one a run takes for this role, chosen or the default.
            "inUse": installed_for(library, m.role).is_some_and(|used| used.id == m.id),
            "files": files_of(m).iter().map(|f| json!({
                "name": f.file_name,
                "bytes": f.bytes,
            })).collect::<Vec<_>>(),
            "path": path_of(library, m),
        })).chain(LOCAL.iter().map(|m| {
            let paths = local_paths(library, m);
            json!({
                "id": m.id,
                "name": lang.t(m.name, m.name_zh),
                "role": m.role.id(),
                "roleLabel": lang.t(m.role.label(), m.role.label_zh()),
                "bytes": paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum::<u64>(),
                "license": m.license,
                "note": lang.t(m.note, m.note_zh),
                "memoryMb": m.memory_mb,
                "rank": 1,
                "installed": local_installed(library, m),
                "bundled": bundled_dir().is_some_and(|dir| m.files.iter().all(|f| {
                    !library.models_dir().join(f).is_file() && dir.join(f).is_file()
                })),
                "downloadable": false,
                "fits": local_fits(m),
                "inUse": local_for(library, m.role).is_some_and(|used| used.id == m.id),
                "files": m.files.iter().map(|f| json!({ "name": f, "bytes": null })).collect::<Vec<_>>(),
                "path": paths[0],
            })
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Library {
        let root = std::env::temp_dir().join(format!("vb-models-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let library = Library::at(root);
        library.ensure().unwrap();
        library
    }

    #[test]
    fn every_catalogue_entry_is_shippable_and_complete() {
        // A non-commercial model in here would be the same release blocker as
        // a non-commercial crate, and cargo-deny cannot see model weights.
        for model in &CATALOGUE {
            assert!(
                matches!(model.license, "MIT" | "Apache-2.0" | "BSD-3-Clause"),
                "{} is offered under {}, which may not be shippable",
                model.id,
                model.license
            );
            assert!(
                model.url.starts_with("https://"),
                "{} is not over TLS",
                model.id
            );
            assert_eq!(model.sha256.len(), 64, "{} has no usable digest", model.id);
            // YuNet is the smallest, at 232 KB.
            assert!(
                model.bytes > 100_000,
                "{} has an implausible size",
                model.id
            );
            assert!(
                !model.note.is_empty(),
                "{} tells the user nothing",
                model.id
            );
            assert!(model.rank > 0, "{} has no capability ranking", model.id);
        }
    }

    #[test]
    fn ids_and_file_names_are_unique() {
        let mut ids: Vec<&str> = CATALOGUE.iter().map(|m| m.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);

        let mut files: Vec<&str> = CATALOGUE
            .iter()
            .flat_map(|m| files_of(m).into_iter().map(|f| f.file_name))
            .collect();
        let count = files.len();
        files.sort();
        files.dedup();
        assert_eq!(files.len(), count, "two models would overwrite each other");
        for model in &CATALOGUE {
            for file in files_of(model) {
                assert_eq!(
                    file.sha256.len(),
                    64,
                    "{} has no usable digest",
                    file.file_name
                );
                assert!(file.url.starts_with("https://"));
            }
            if model.projector.is_some() {
                assert!(!model.adapter.is_empty(), "{} runs unnamed", model.id);
            }
        }
    }

    #[test]
    fn a_half_finished_download_does_not_count_as_installed() {
        let library = scratch("partial");
        let model = spec("whisper-tiny").unwrap();
        assert!(!is_installed(&library, model));

        std::fs::write(path_of(&library, model), b"not the whole file").unwrap();
        assert!(
            !is_installed(&library, model),
            "a truncated model must not be offered as a working one"
        );
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn the_most_capable_installed_model_is_the_one_used() {
        let library = scratch("pick");
        assert!(installed_for(&library, Role::Speech).is_none());

        let small = spec("whisper-tiny").unwrap();
        std::fs::write(path_of(&library, small), vec![0u8; small.bytes as usize]).unwrap();
        assert_eq!(
            installed_for(&library, Role::Speech).unwrap().id,
            "whisper-tiny"
        );

        // Quantised base is a smaller file than tiny and a better model, so
        // this also pins that the ranking is not file size.
        let better = spec("whisper-base-q5").unwrap();
        assert!(better.bytes < small.bytes);
        std::fs::write(path_of(&library, better), vec![0u8; better.bytes as usize]).unwrap();
        assert_eq!(
            installed_for(&library, Role::Speech).unwrap().id,
            "whisper-base-q5"
        );
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn a_chosen_model_is_used_while_it_is_installed() {
        let library = scratch("choose");
        // Sparse files of the right length stand in for the weights.
        let install = |id: &str| {
            let model = spec(id).unwrap();
            let file = std::fs::File::create(path_of(&library, model)).unwrap();
            file.set_len(model.bytes).unwrap();
        };
        install("whisper-small-q5");
        install("whisper-small");
        let used = |library: &Library| installed_for(library, Role::Speech).map(|m| m.id);
        assert_eq!(
            used(&library),
            Some("whisper-small"),
            "the most capable by default"
        );

        choose(&library, "whisper-small-q5", crate::Lang::En).unwrap();
        assert_eq!(used(&library), Some("whisper-small-q5"));
        let listing = listing(&library);
        let in_use: Vec<&str> = listing["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["inUse"] == true)
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(in_use, ["whisper-small-q5"]);

        // A choice of something not there is refused, and one that goes
        // away gives way to the default rather than to nothing.
        let err = choose(&library, "whisper-tiny", crate::Lang::En).unwrap_err();
        assert!(err.contains("not installed"), "{err}");
        assert!(choose(&library, "not-a-model", crate::Lang::En).is_err());
        remove(&library, "whisper-small-q5").unwrap();
        assert_eq!(used(&library), Some("whisper-small"));
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn the_installers_carry_every_model() {
        // The packaging scripts name each file with its digest by hand; this
        // holds them to the catalogue, so a model added here cannot be left
        // out of an installer, nor a changed digest missed there.
        let script = |name: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts")
                .join(name);
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        };
        let mac = script("package-macos.sh");
        let apk = script("build-android.sh");
        let beside_apk = script("package-android.sh");
        for model in &CATALOGUE {
            for file in files_of(model) {
                let entry = format!("{} {}", file.file_name, file.sha256);
                // A Mac names scenes with Apple Vision; SigLIP 2 would never run.
                if model.role != Role::Labels {
                    assert!(
                        mac.contains(&entry),
                        "the Mac installer lacks {}",
                        file.file_name
                    );
                }
                // An Android device gets each file once: in the APK or beside it.
                assert!(
                    apk.contains(&entry) != beside_apk.contains(&entry),
                    "{} must be in the APK or beside it, and only one",
                    file.file_name
                );
            }
        }
        for name in LOCAL.iter().flat_map(|m| m.files) {
            assert!(mac.contains(name), "the Mac installer lacks {name}");
            assert!(
                beside_apk.contains(name),
                "the Android installer lacks {name}"
            );
        }
    }

    #[test]
    fn a_local_model_runs_only_where_it_fits() {
        // PhotoFramer is a 7B: an 8 GB phone cannot hold it, a 12 GB one can.
        let judge = &LOCAL[0];
        assert!(!needs_fit(judge.memory_mb, Some(7_500)));
        assert!(needs_fit(judge.memory_mb, Some(11_000)));
        assert!(needs_fit(judge.memory_mb, Some(15_400)));
        assert!(needs_fit(judge.memory_mb, None));
    }

    #[test]
    fn a_model_pack_is_found_and_a_wrong_file_is_refused() {
        let pack = std::env::temp_dir().join("vb-models-pack");
        let _ = std::fs::remove_dir_all(&pack);
        std::fs::create_dir_all(pack.join("small")).unwrap();
        let yunet = spec("yunet-2023mar").unwrap();
        // The right name, the wrong bytes.
        std::fs::write(pack.join("small").join(yunet.file_name), b"not yunet").unwrap();
        std::fs::write(pack.join("notes.txt"), b"ignored").unwrap();
        let found = found_in(&pack);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1.file_name, yunet.file_name);

        let library = scratch("import");
        let dest = path_of(&library, yunet);
        let err = copy_checked(&found[0].0, &dest, &found[0].1, &|_| true).unwrap_err();
        assert!(err.contains("not the file"), "{err}");
        assert!(!dest.exists() && !dest.with_extension("part").exists());
        let _ = std::fs::remove_dir_all(&pack);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn models_that_came_with_the_app_are_used_and_never_deleted() {
        let library = scratch("bundled");
        let bundle = std::env::temp_dir().join("vb-app-bundle-models");
        let _ = std::fs::remove_dir_all(&bundle);
        std::fs::create_dir_all(&bundle).unwrap();
        let yunet = spec("yunet-2023mar").unwrap();
        let file = std::fs::File::create(bundle.join(yunet.file_name)).unwrap();
        file.set_len(yunet.bytes).unwrap();

        let bundled = Some(bundle.as_path());
        assert!(is_installed_in(&library, yunet, bundled));
        assert!(is_bundled_in(&library, yunet, bundled));
        assert_eq!(
            file_path_in(&library, yunet.file_name, bundled),
            bundle.join(yunet.file_name)
        );
        // A download or an import lands in the library, and is then the one
        // used; deleting it leaves what came with the app.
        std::fs::copy(bundle.join(yunet.file_name), path_of(&library, yunet)).unwrap();
        assert!(!is_bundled_in(&library, yunet, bundled));
        remove(&library, yunet.id).unwrap();
        assert!(
            bundle.join(yunet.file_name).is_file(),
            "the bundle is read only"
        );
        assert!(is_installed_in(&library, yunet, bundled));

        let _ = std::fs::remove_dir_all(&bundle);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn a_device_uses_the_largest_caption_model_it_can_hold() {
        let by_id = |id: &str| spec(id).unwrap();
        // Totals as kernels report them, a gigabyte or so under what the
        // device is sold with. A first version checked "12 GB" as 12,000 MB,
        // which no 12 GB phone reports, and refused the 8B on all of them.
        let phone_12 = [Some(10_900), Some(11_700)];
        let phone_8 = [Some(7_400), Some(7_939)];
        let phone_6 = Some(5_500);
        // 12 GB holds the 8B, the doc's tier B; 8 GB the 4B; 6 GB the 2B.
        for memory in phone_12 {
            assert!(fits_in(by_id("qwen3-vl-8b-q4"), memory), "{memory:?}");
        }
        for memory in phone_8 {
            assert!(!fits_in(by_id("qwen3-vl-8b-q4"), memory), "{memory:?}");
            assert!(fits_in(by_id("qwen3-vl-4b-q4"), memory), "{memory:?}");
        }
        assert!(!fits_in(by_id("qwen3-vl-4b-q4"), phone_6));
        assert!(fits_in(by_id("qwen3-vl-2b-q4"), phone_6));
        // Where memory cannot be read, everything is taken to fit.
        assert!(fits_in(by_id("qwen3-vl-8b-q4"), None));
        assert_eq!(sold_with_mb(11_000), 12 * 1024);
        assert_eq!(sold_with_mb(15_433), 16 * 1024);
        assert_eq!(sold_with_mb(100_000), 100_000);
    }

    #[test]
    fn a_vision_model_without_its_projector_is_not_installed() {
        let library = scratch("projector");
        let vlm = CATALOGUE
            .iter()
            .find(|m| m.projector.is_some())
            .expect("the catalogue has a vision-language model");
        // A sparse file of the right length stands in for the weights.
        let weights = std::fs::File::create(path_of(&library, vlm)).unwrap();
        weights.set_len(vlm.bytes).unwrap();
        assert!(!is_installed(&library, vlm), "the projector is missing");
        let projector = std::fs::File::create(projector_path(&library, vlm).unwrap()).unwrap();
        projector
            .set_len(vlm.projector.as_ref().unwrap().bytes)
            .unwrap();
        assert!(is_installed(&library, vlm));
        remove(&library, vlm.id).unwrap();
        assert!(!projector_path(&library, vlm).unwrap().exists(), "both go");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn removing_a_model_that_is_not_there_is_not_an_error() {
        let library = scratch("remove");
        assert!(remove(&library, "whisper-tiny").is_ok());
        assert!(remove(&library, "not-a-model").is_err());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn the_listing_says_where_models_live_and_which_are_installed() {
        let library = scratch("listing");
        let listing = listing(&library);
        assert_eq!(
            listing["directory"].as_str().unwrap(),
            library.models_dir().to_string_lossy()
        );
        let models = listing["models"].as_array().unwrap();
        assert_eq!(models.len(), CATALOGUE.len() + LOCAL.len());
        // A model with nowhere to download it from says so.
        assert!(models.iter().any(|m| m["downloadable"] == false));
        assert!(models.iter().all(|m| m["installed"] == false));
        assert!(models.iter().all(|m| m["inUse"] == false));
        let _ = std::fs::remove_dir_all(library.root());
    }
}

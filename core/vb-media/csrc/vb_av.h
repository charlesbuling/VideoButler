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

/*
 * vb_av: the narrow C surface the LibAv media backend calls.
 *
 * Rust never touches a libav struct. Every layout that changes between ffmpeg
 * releases stays on the C side, where the headers describe it, and Rust sees
 * only the plain functions and the one plain struct declared here. That is
 * what lets one Rust source build against whichever ffmpeg a platform
 * provides, with no bindgen and no hand-copied struct definitions to drift.
 *
 * Keep this file and `src/libav.rs` in step: the struct below is mirrored
 * there field for field.
 */
#ifndef VB_AV_H
#define VB_AV_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes. Every function returns one of these; on anything but
 * VB_AV_OK a human-readable reason has been written to `err`. */
#define VB_AV_OK 0
#define VB_AV_ERR_OPEN -1        /* the container could not be opened or read */
#define VB_AV_ERR_NO_VIDEO -2    /* the file has no video stream */
#define VB_AV_ERR_DECODE -3      /* decoding, filtering or conversion failed */
#define VB_AV_ERR_PAST_END -4    /* the requested time is beyond the footage */
#define VB_AV_ERR_ARG -5         /* a caller error: bad size, null pointer */
#define VB_AV_ERR_IO -6          /* writing an output file failed */
#define VB_AV_ERR_UNSUPPORTED -7 /* the build lacks a codec, or a format is unknown */
#define VB_AV_ERR_NO_AUDIO -8    /* the file has no audio stream */
#define VB_AV_ERR_STOPPED -9     /* the caller's progress callback asked to stop */

typedef struct vb_av_info {
    /* Container duration, in microseconds. Zero when unknown. */
    int64_t duration_us;
    uint32_t fps_num;
    uint32_t fps_den;
    uint32_t width;
    uint32_t height;
    int32_t has_audio;
    /* NUL-terminated libav codec names, e.g. "h264". Empty when absent. */
    char video_codec[32];
    char audio_codec[32];
    /* The clockwise turn, in degrees (0, 90, 180 or 270), that shows the
     * picture upright: what a phone records in its display matrix, and what
     * the CLI's autorotate applies. */
    int32_t rotation;
    /* The container's creation_time tag, verbatim. Empty when absent. */
    char created_at[48];
    /* The GPS fix in ISO 6709, verbatim: the first of the `location`,
     * `com.apple.quicktime.location.ISO6709` and `location-eng` tags that is
     * there. Empty when absent. Parsed on the Rust side, by the same code
     * as the CLI backend's. */
    char location[64];
} vb_av_info;

/* Called once per detected cut, in presentation order. */
typedef void (*vb_av_scene_cb)(void *ctx, int64_t at_us, double score);

int vb_av_probe(const char *path, vb_av_info *out, char *err, size_t err_len);

int vb_av_detect_scenes(const char *path, double threshold, vb_av_scene_cb cb,
                        void *ctx, char *err, size_t err_len);

/* Writes exactly width * height bytes of 8-bit greyscale to `out`. */
int vb_av_gray_frame(const char *path, int64_t at_us, uint32_t width,
                     uint32_t height, uint8_t *out, char *err, size_t err_len);

/*
 * Called once per swept frame: `rgb` holds width * height * 3 bytes of
 * full-range RGB, valid only during the call. Return 0 to stop the sweep.
 */
typedef int (*vb_av_frame_cb)(void *ctx, int64_t at_us, const uint8_t *rgb);

/*
 * Decode the whole video once, scaling a frame to width x height RGB and
 * handing it to `cb` for the first frame and then for each frame at least
 * `min_gap_us` after the last one handed over. *count receives how many were.
 * Frames are turned upright first, as the CLI's autorotate turns them.
 */
int vb_av_sample_frames(const char *path, int64_t min_gap_us, uint32_t width, uint32_t height,
                        vb_av_frame_cb cb, void *ctx, size_t *count, char *err,
                        size_t err_len);

/*
 * vb_av_detect_scenes and vb_av_sample_frames from one decode of the file:
 * the same cuts to `scene_cb`, the same frames to `frame_cb`. Returning 0
 * from `frame_cb` stops both.
 */
int vb_av_scan(const char *path, double threshold, vb_av_scene_cb scene_cb, void *scene_ctx,
               int64_t min_gap_us, uint32_t width, uint32_t height, vb_av_frame_cb frame_cb,
               void *frame_ctx, size_t *count, char *err, size_t err_len);

/*
 * vb_av_sample_frames over one stretch, from_us to to_us: seeks rather than
 * decoding from the start. Frames are upright, as a sweep's are.
 */
int vb_av_sample_range(const char *path, int64_t from_us, int64_t to_us, int64_t min_gap_us,
                       uint32_t width, uint32_t height, vb_av_frame_cb cb, void *ctx,
                       size_t *count, char *err, size_t err_len);

/* Writes exactly width * height * 3 bytes of 8-bit RGB to `out`: one frame at
 * `at_us`, turned upright and then scaled, as a sweep frame is. */
int vb_av_rgb_frame(const char *path, int64_t at_us, uint32_t width, uint32_t height,
                    uint8_t *out, char *err, size_t err_len);

/* The format follows the extension of `dest`: .jpg, .jpeg or .png. */
int vb_av_extract_still(const char *path, int64_t at_us, const char *dest,
                        char *err, size_t err_len);

/*
 * Photographs: JPEG, PNG, WebP, and HEIF, whose tile grid is assembled here.
 * None of these turn the picture on their own; `rotation` (0, 90, 180 or 270,
 * clockwise) is the EXIF orientation the caller read.
 */

/* The picture's size as stored -- a HEIF grid's, not a tile's -- and its
 * codec's name. */
int vb_av_picture_info(const char *path, uint32_t *width, uint32_t *height, char *codec,
                       size_t codec_len, char *err, size_t err_len);

/* Writes the picture turned by `rotation` and scaled to width x height:
 * width * height * 3 bytes of RGB, or width * height of grey when `gray`. */
int vb_av_picture(const char *path, int32_t rotation, uint32_t width, uint32_t height,
                  int32_t gray, uint8_t *out, char *err, size_t err_len);

/* The picture turned and scaled likewise, written as a .jpg or .png still. */
int vb_av_picture_still(const char *path, int32_t rotation, uint32_t width, uint32_t height,
                        const char *dest, char *err, size_t err_len);

/*
 * Decode the audio as one channel at `sample_rate`, as 32-bit floats.
 *
 * `max_us` stops the decode early; 0 means the whole file. On success *out
 * holds *count samples and belongs to the caller, who frees it with
 * vb_av_free_samples, *start_us is where the first sample sits in the file's
 * timeline, and *duration_us is how long the audio runs in the file, which is
 * not how much was decoded when max_us cut it short.
 */
int vb_av_decode_audio(const char *path, uint32_t sample_rate, int64_t max_us, float **out,
                       size_t *count, int64_t *start_us, int64_t *duration_us, char *err,
                       size_t err_len);

void vb_av_free_samples(float *samples);

/*
 * The renderer's three needs, as handles that live across calls. Each is
 * used from one thread at a time; each must be closed (or finished, or
 * aborted) exactly once.
 */

/* A video's frames in order from a moment, upright and scaled to RGBA. */
typedef struct vb_av_reader vb_av_reader;

/* The first frame handed out is the first at or after `from_us`, as an exact
 * seek finds it -- or, when the file has none after it, the last before it. */
int vb_av_reader_open(const char *path, int64_t from_us, uint32_t width, uint32_t height,
                      vb_av_reader **out, char *err, size_t err_len);

/* 1 with the next frame in `rgba` (width * height * 4 bytes, full range,
 * opaque) and its time from the start of the file in *at_us; 0 once there
 * are no more; or an error code. */
int vb_av_reader_next(vb_av_reader *r, uint8_t *rgba, int64_t *at_us, char *err,
                      size_t err_len);

void vb_av_reader_close(vb_av_reader *r);

/* A file's sound in order from `from_us` to `to_us` (negative: its end),
 * through `filters` (an ffmpeg audio filter description, or NULL), as
 * interleaved floats at `sample_rate` with `channels` channels. */
typedef struct vb_av_sound vb_av_sound;

int vb_av_sound_open(const char *path, int64_t from_us, int64_t to_us, uint32_t sample_rate,
                     uint32_t channels, const char *filters, vb_av_sound **out, char *err,
                     size_t err_len);

/* Up to `max_frames` frames (a frame is one sample per channel) into `out`;
 * *got receives how many, which is 0 only once there is no more. */
int vb_av_sound_read(vb_av_sound *s, float *out, size_t max_frames, size_t *got, char *err,
                     size_t err_len);

void vb_av_sound_close(vb_av_sound *s);

/* How a finished piece is written. Mirrored in `src/libav.rs`. */
typedef struct vb_av_encode_settings {
    uint64_t video_bitrate;
    uint64_t audio_bitrate;
    uint32_t width;
    uint32_t height;
    uint32_t fps_num;
    uint32_t fps_den;
    uint32_t sample_rate;
    uint32_t channels;
    /* 0 for H.264, 1 for HEVC. */
    int32_t hevc;
} vb_av_encode_settings;

/* Whether this build can write H.264 (0) or HEVC (1) with AAC in an MP4. */
int vb_av_can_encode(int32_t hevc);

/* A finished piece written frame by frame, the sound alongside. */
typedef struct vb_av_writer vb_av_writer;

int vb_av_writer_open(const char *dest, const vb_av_encode_settings *settings,
                      vb_av_writer **out, char *err, size_t err_len);

/* One frame of width * height * 4 bytes of RGBA. */
int vb_av_writer_video(vb_av_writer *w, const uint8_t *rgba, char *err, size_t err_len);

/* `frames` frames of interleaved floats. */
int vb_av_writer_audio(vb_av_writer *w, const float *interleaved, size_t frames, char *err,
                       size_t err_len);

/* Write what is left and close the file. Frees `w` whatever it returns. */
int vb_av_writer_finish(vb_av_writer *w, char *err, size_t err_len);

/* Free `w` without finishing: what is on disk is not a playable file, and
 * the caller removes it. */
void vb_av_writer_abort(vb_av_writer *w);

/*
 * Lossless export: stretches of files joined by copying their packets.
 */

/* The video's keyframes from `from_us` to `to_us`, both included, from the
 * packets' flags: *out holds *count times, sorted, which the caller frees
 * with vb_av_free_times. */
int vb_av_keyframes(const char *path, int64_t from_us, int64_t to_us, int64_t **out,
                    size_t *count, char *err, size_t err_len);

void vb_av_free_times(int64_t *times);

/* What the file's streams are as far as copying goes, as text: equal
 * fingerprints can be joined without re-encoding. */
int vb_av_copy_fingerprint(const char *path, char *out, size_t out_len, char *err,
                           size_t err_len);

/* Whether this build can join files: the MP4 muxer. */
int vb_av_can_copy(void);

/* Told the share done; returns 0 to stop. */
typedef int (*vb_av_share_cb)(void *ctx, double share);

/* One stretch of a file, from a keyframe. Mirrored in `src/libav/copy.rs`. */
typedef struct vb_av_part {
    const char *path;
    int64_t from_us;
    int64_t to_us;
} vb_av_part;

/* Where a copy from the keyframe at `from_us` to `to_us` ends, in the file's
 * own time: at `to_us`, or a few frames on where the frames are whole when
 * it falls inside a group of B-frames -- what vb_av_join keeps of a part. */
int vb_av_copy_end(const char *path, int64_t from_us, int64_t to_us, int64_t *end_us, char *err,
                   size_t err_len);

/* Join `count` stretches into `dest` (an MP4) by copying their packets:
 * the video, and the first sound when `audio`. Each stretch runs from its
 * keyframe to its vb_av_copy_end, every frame in it shown, with the sound
 * under that; the next starts after both. VB_AV_ERR_STOPPED when `cb`
 * stopped it. */
int vb_av_join(const vb_av_part *parts, size_t count, const char *dest, int32_t audio,
               vb_av_share_cb cb, void *ctx, char *err, size_t err_len);

/* Static strings owned by libav, e.g. "9.0.1" and "LGPL version 2.1 or later". */
const char *vb_av_version(void);
const char *vb_av_license(void);

#ifdef __cplusplus
}
#endif

#endif /* VB_AV_H */

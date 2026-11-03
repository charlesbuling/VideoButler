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
 * The in-process media backend, over linked libav.
 *
 * Behaviour deliberately mirrors the ffmpeg CLI backend: the same scene
 * filter, the same timestamp origin, the same seek semantics. Two backends
 * that disagreed about where a shot starts would make the cache and the
 * provenance record lie, so where the CLI makes a choice, this makes the
 * same one and says so.
 *
 * Every entry point opens the file afresh. That costs a demuxer setup per
 * call, which is still far cheaper than the process spawn the CLI backend
 * pays for the same call, and it keeps every function free of shared state.
 */
#include "vb_av.h"

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/audio_fifo.h>
#include <libavutil/avstring.h>
#include <libavutil/avutil.h>
#include <libavutil/dict.h>
#include <libavutil/display.h>
#include <libavutil/error.h>
#include <libavutil/hwcontext.h>
#include <libavutil/mathematics.h>
#include <libavutil/mem.h>
#include <libavutil/pixdesc.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
#include <libavutil/channel_layout.h>
#include <libavutil/opt.h>
#include <libavutil/samplefmt.h>

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_MSC_VER)
#define VB_THREAD_LOCAL __declspec(thread)
#else
#define VB_THREAD_LOCAL _Thread_local
#endif

/* ------------------------------------------------------------------------ */
/* Errors                                                                    */
/* ------------------------------------------------------------------------ */

/*
 * The last error libav logged on this thread.
 *
 * libav reports the useful half of most failures through its logger rather
 * than its return codes: "Invalid data found when processing input" is the
 * code, "moov atom not found" is the log line that says why. Capturing it
 * here puts both in the error the user sees, instead of printing the second
 * to a stderr the app does not show.
 */
static VB_THREAD_LOCAL char last_log[256];

static void log_callback(void *avcl, int level, const char *fmt, va_list vl) {
    if (level > AV_LOG_ERROR) {
        return;
    }
    char line[sizeof last_log];
    int print_prefix = 0;
    va_list copy;
    va_copy(copy, vl);
    av_log_format_line2(avcl, level, fmt, copy, line, sizeof line, &print_prefix);
    va_end(copy);

    size_t n = strlen(line);
    while (n > 0 && (line[n - 1] == '\n' || line[n - 1] == '\r')) {
        line[--n] = '\0';
    }
    if (n > 0) {
        memcpy(last_log, line, n + 1);
    }
}

/* Called at every entry point. Installing the callback repeatedly is
 * idempotent, which is cheaper to reason about than a once-guard that would
 * need a different primitive on every platform. */
static void begin_call(void) {
    av_log_set_callback(log_callback);
    last_log[0] = '\0';
}

static void set_err(char *err, size_t err_len, const char *fmt, ...) {
    if (!err || err_len == 0) {
        return;
    }
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(err, err_len, fmt, ap);
    va_end(ap);
}

static void set_av_err(char *err, size_t err_len, const char *what, int code) {
    char reason[AV_ERROR_MAX_STRING_SIZE];
    av_strerror(code, reason, sizeof reason);
    if (last_log[0]) {
        set_err(err, err_len, "%s: %s (%s)", what, reason, last_log);
    } else {
        set_err(err, err_len, "%s: %s", what, reason);
    }
}

/* ------------------------------------------------------------------------ */
/* Opening and decoding                                                      */
/* ------------------------------------------------------------------------ */

static int open_input(const char *path, AVFormatContext **out, char *err, size_t err_len) {
    AVFormatContext *fmt = NULL;
    int ret = avformat_open_input(&fmt, path, NULL, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the file", ret);
        return VB_AV_ERR_OPEN;
    }
    /* Needed for more than MPEG-TS: r_frame_rate, which the probe reports,
     * is only estimated here. ffprobe calls it too, so the two agree. */
    ret = avformat_find_stream_info(fmt, NULL);
    if (ret < 0) {
        avformat_close_input(&fmt);
        set_av_err(err, err_len, "could not read the stream information", ret);
        return VB_AV_ERR_OPEN;
    }
    *out = fmt;
    return VB_AV_OK;
}

/* First stream of a type, skipping embedded cover art: that is stored as a
 * one-frame video stream, and it is not footage. */
static int find_stream(const AVFormatContext *fmt, enum AVMediaType type) {
    for (unsigned i = 0; i < fmt->nb_streams; i++) {
        const AVStream *st = fmt->streams[i];
        if (st->codecpar->codec_type != type) {
            continue;
        }
        if (st->disposition & AV_DISPOSITION_ATTACHED_PIC) {
            continue;
        }
        return (int)i;
    }
    return -1;
}

/* An open video stream and its decoder. */
typedef struct {
    AVFormatContext *fmt;
    AVCodecContext *dec;
    AVStream *st;
    int index;
    AVPacket *pkt;
    AVFrame *frame;
    int draining;
    /* The container's start time, in microseconds. The CLI subtracts it from
     * every timestamp, so time zero is the first frame even in an MPEG-TS
     * file whose clock starts at 1.4 s; this does the same. */
    int64_t origin_us;
    /* The hardware decoder's device, when the stream decodes in hardware. */
    AVBufferRef *hw_device;
    /* The file, to open again in software if the hardware gives up on it. */
    const char *path;
    /* Where decoding started, in the stream's time base: the timestamp the
     * caller sought to, or AV_NOPTS_VALUE for the start of the file. */
    int64_t start_ts;
    /* The last frame handed out, by its timestamp in the stream's time base,
     * when one has been. */
    int64_t last_ts;
    int handed_out;
    /* After a switch to software mid-stream: the frames up to this timestamp
     * were handed out already, and are skipped. AV_NOPTS_VALUE otherwise. */
    int64_t skip_through;
} Video;

static void video_close(Video *v) {
    av_frame_free(&v->frame);
    av_packet_free(&v->pkt);
    avcodec_free_context(&v->dec);
    avformat_close_input(&v->fmt);
    av_buffer_unref(&v->hw_device);
}

/* ------------------------------------------------------------------------ */
/* Hardware decoding                                                         */
/* ------------------------------------------------------------------------ */

/*
 * The Mac decodes H.264 and HEVC on its media engine: 4K HEVC at several
 * times real time with the CPU idle, where the software decoder manages
 * about one and a half on every core it can use. Windows does the same on
 * whatever GPU the machine has, through D3D11. Only those two codecs --
 * the ones cameras and phones record -- and only where the build carries
 * VideoToolbox or D3D11VA; everywhere else, and for any stream the engine
 * refuses, libav decodes in software exactly as before.
 *
 * H.264 and HEVC decoding is bit-exact by specification, so the hardware
 * produces the same samples as the software decoder. It hands them over in
 * a different layout (NV12, P010), which `video_pixels` repacks into the
 * layout the software decoder uses before anything reads them, so every
 * score, sweep frame and still is the same whichever decoder ran.
 *
 * VB_HWACCEL=0 in the environment turns it off, for comparing the two.
 *
 * The engine also refuses some streams it accepted, frame after frame, as
 * bad data: a Meizu's clips with temporal layers (the SVC prefix NAL units
 * before each slice) and some of WeChat's. The software decoder reads those
 * whole, so the stream is opened again in software and decoding carries on
 * where it was (`video_to_software`).
 */
static int hw_wanted(enum AVCodecID codec) {
    if (codec != AV_CODEC_ID_H264 && codec != AV_CODEC_ID_HEVC) {
        return 0;
    }
    const char *env = getenv("VB_HWACCEL");
    return !(env && env[0] == '0');
}

/* The hardware decoders, each with the format its frames come in: one
 * device per platform, so whichever this build carries and the machine
 * opens is the one. D3D11's is the API that takes a device (d3d11va2),
 * not the older one that wants the caller's own decoder context. */
static const struct {
    enum AVHWDeviceType device;
    enum AVPixelFormat format;
} hw_kinds[] = {
    {AV_HWDEVICE_TYPE_VIDEOTOOLBOX, AV_PIX_FMT_VIDEOTOOLBOX},
    {AV_HWDEVICE_TYPE_D3D11VA, AV_PIX_FMT_D3D11},
};

static int is_hw_format(int format) {
    for (size_t i = 0; i < sizeof hw_kinds / sizeof hw_kinds[0]; i++) {
        if (format == hw_kinds[i].format) {
            return 1;
        }
    }
    return 0;
}

static enum AVPixelFormat pick_hw_format(AVCodecContext *ctx, const enum AVPixelFormat *fmts) {
    for (const enum AVPixelFormat *p = fmts; *p != AV_PIX_FMT_NONE; p++) {
        if (is_hw_format(*p)) {
            return *p;
        }
    }
    /* Offered no hardware format -- or offered again after the hardware
     * decoder failed to start, which libav does with it removed. */
    return avcodec_default_get_format(ctx, fmts);
}

/* Attach the hardware decoder to `v->dec`, before it is opened. Failure is
 * not an error: the stream decodes in software. */
static void hw_attach(Video *v, const AVCodec *codec) {
    if (!hw_wanted(codec->id)) {
        return;
    }
    for (size_t k = 0; k < sizeof hw_kinds / sizeof hw_kinds[0]; k++) {
        int offered = 0;
        for (int i = 0;; i++) {
            const AVCodecHWConfig *config = avcodec_get_hw_config(codec, i);
            if (!config) {
                break;
            }
            if (config->device_type == hw_kinds[k].device &&
                (config->methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX)) {
                offered = 1;
                break;
            }
        }
        if (offered && av_hwdevice_ctx_create(&v->hw_device, hw_kinds[k].device, NULL, NULL, 0) >= 0) {
            v->dec->hw_device_ctx = av_buffer_ref(v->hw_device);
            v->dec->get_format = pick_hw_format;
            return;
        }
    }
}

/* NV12 or P010 into the planar layout of the same samples: Y copied (or, for
 * P010, shifted down from the top of each 16-bit word), U and V pulled apart.
 * Exact: every sample keeps its value. */
static void deinterleave(const AVFrame *src, AVFrame *dst, int high_bits) {
    int w = src->width, h = src->height;
    int cw = (w + 1) / 2, ch = (h + 1) / 2;
    if (!high_bits) {
        for (int y = 0; y < h; y++) {
            memcpy(dst->data[0] + (size_t)y * dst->linesize[0],
                   src->data[0] + (size_t)y * src->linesize[0], (size_t)w);
        }
        for (int y = 0; y < ch; y++) {
            const uint8_t *uv = src->data[1] + (size_t)y * src->linesize[1];
            uint8_t *u = dst->data[1] + (size_t)y * dst->linesize[1];
            uint8_t *vv = dst->data[2] + (size_t)y * dst->linesize[2];
            for (int x = 0; x < cw; x++) {
                u[x] = uv[2 * x];
                vv[x] = uv[2 * x + 1];
            }
        }
        return;
    }
    for (int y = 0; y < h; y++) {
        const uint16_t *s = (const uint16_t *)(src->data[0] + (size_t)y * src->linesize[0]);
        uint16_t *d = (uint16_t *)(dst->data[0] + (size_t)y * dst->linesize[0]);
        for (int x = 0; x < w; x++) {
            d[x] = s[x] >> 6;
        }
    }
    for (int y = 0; y < ch; y++) {
        const uint16_t *uv = (const uint16_t *)(src->data[1] + (size_t)y * src->linesize[1]);
        uint16_t *u = (uint16_t *)(dst->data[1] + (size_t)y * dst->linesize[1]);
        uint16_t *vv = (uint16_t *)(dst->data[2] + (size_t)y * dst->linesize[2]);
        for (int x = 0; x < cw; x++) {
            u[x] = uv[2 * x] >> 6;
            vv[x] = uv[2 * x + 1] >> 6;
        }
    }
}

/*
 * Make `v->frame` a software frame in the software decoder's layout, when
 * the hardware decoded it. A no-op for a frame already in memory.
 *
 * Called only for frames whose pixels are read: seeking decodes a dozen
 * frames to reach one, and fetching the eleven nobody looks at would cost
 * most of what the hardware saved.
 */
static int video_pixels(Video *v, char *err, size_t err_len) {
    if (!is_hw_format(v->frame->format)) {
        return VB_AV_OK;
    }
    AVFrame *soft = av_frame_alloc();
    AVFrame *plain = av_frame_alloc();
    if (!soft || !plain) {
        av_frame_free(&soft);
        av_frame_free(&plain);
        set_err(err, err_len, "out of memory fetching a decoded frame");
        return VB_AV_ERR_DECODE;
    }
    int ret = av_hwframe_transfer_data(soft, v->frame, 0);
    if (ret >= 0) {
        ret = av_frame_copy_props(soft, v->frame);
    }
    if (ret < 0) {
        av_frame_free(&soft);
        av_frame_free(&plain);
        set_av_err(err, err_len, "could not fetch the frame from the hardware decoder", ret);
        return VB_AV_ERR_DECODE;
    }

    enum AVPixelFormat want = v->dec->sw_pix_fmt;
    if (want == AV_PIX_FMT_NONE || soft->format == want) {
        av_frame_free(&plain);
        av_frame_unref(v->frame);
        av_frame_move_ref(v->frame, soft);
        av_frame_free(&soft);
        return VB_AV_OK;
    }

    plain->format = want;
    plain->width = soft->width;
    plain->height = soft->height;
    ret = av_frame_get_buffer(plain, 0);
    if (ret >= 0) {
        ret = av_frame_copy_props(plain, soft);
    }
    if (ret >= 0) {
        if (soft->format == AV_PIX_FMT_NV12 && want == AV_PIX_FMT_YUV420P) {
            deinterleave(soft, plain, 0);
        } else if (soft->format == AV_PIX_FMT_P010 && want == AV_PIX_FMT_YUV420P10) {
            deinterleave(soft, plain, 1);
        } else {
            /* Any other pairing -- 4:2:2 from a camera, say -- is the same
             * samples in another packing, which the scaler repacks at the
             * same size. */
            struct SwsContext *sws =
                sws_getContext(soft->width, soft->height, (enum AVPixelFormat)soft->format,
                               plain->width, plain->height, want, SWS_POINT, NULL, NULL, NULL);
            if (!sws || sws_scale(sws, (const uint8_t *const *)soft->data, soft->linesize, 0,
                                  soft->height, plain->data, plain->linesize) <= 0) {
                ret = AVERROR(EINVAL);
            }
            sws_freeContext(sws);
        }
    }
    av_frame_free(&soft);
    if (ret < 0) {
        av_frame_free(&plain);
        set_av_err(err, err_len, "could not repack the hardware-decoded frame", ret);
        return VB_AV_ERR_DECODE;
    }
    av_frame_unref(v->frame);
    av_frame_move_ref(v->frame, plain);
    av_frame_free(&plain);
    return VB_AV_OK;
}

/* Open the first video stream of `path` and its decoder: in hardware where
 * the stream can be and `hardware` allows, in software otherwise. */
static int video_open_as(const char *path, Video *v, int hardware, char *err, size_t err_len) {
    memset(v, 0, sizeof *v);
    v->path = path;
    v->start_ts = AV_NOPTS_VALUE;
    v->last_ts = AV_NOPTS_VALUE;
    v->skip_through = AV_NOPTS_VALUE;
    int rc = open_input(path, &v->fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    v->index = find_stream(v->fmt, AVMEDIA_TYPE_VIDEO);
    if (v->index < 0) {
        set_err(err, err_len, "the file has no video stream");
        video_close(v);
        return VB_AV_ERR_NO_VIDEO;
    }
    v->st = v->fmt->streams[v->index];

    /* Only the picture is needed, so the demuxer can skip everything else. */
    for (unsigned i = 0; i < v->fmt->nb_streams; i++) {
        if ((int)i != v->index) {
            v->fmt->streams[i]->discard = AVDISCARD_ALL;
        }
    }

    const AVCodec *codec = avcodec_find_decoder(v->st->codecpar->codec_id);
    if (!codec) {
        set_err(err, err_len, "this build has no decoder for %s",
                avcodec_get_name(v->st->codecpar->codec_id));
        video_close(v);
        return VB_AV_ERR_UNSUPPORTED;
    }

    v->dec = avcodec_alloc_context3(codec);
    v->pkt = av_packet_alloc();
    v->frame = av_frame_alloc();
    if (!v->dec || !v->pkt || !v->frame) {
        set_err(err, err_len, "out of memory opening the decoder");
        video_close(v);
        return VB_AV_ERR_DECODE;
    }

    int ret = avcodec_parameters_to_context(v->dec, v->st->codecpar);
    if (ret < 0) {
        set_av_err(err, err_len, "could not configure the decoder", ret);
        video_close(v);
        return VB_AV_ERR_DECODE;
    }
    v->dec->pkt_timebase = v->st->time_base;
    /* Zero lets libav choose, which is what the CLI does by default. */
    v->dec->thread_count = 0;
    if (hardware) {
        hw_attach(v, codec);
    }

    ret = avcodec_open2(v->dec, codec, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the decoder", ret);
        video_close(v);
        return VB_AV_ERR_DECODE;
    }

    v->origin_us = v->fmt->start_time == AV_NOPTS_VALUE ? 0 : v->fmt->start_time;
    return VB_AV_OK;
}

static int video_open(const char *path, Video *v, char *err, size_t err_len) {
    return video_open_as(path, v, 1, err, err_len);
}

/* Seek to the keyframe at or before `target`, in the stream's time base, as
 * the place decoding starts from. A failed seek leaves the read position at
 * the start of the file, which is slower but still reaches every frame. */
static void video_seek(Video *v, int64_t target) {
    v->start_ts = target;
    if (av_seek_frame(v->fmt, v->index, target, AVSEEK_FLAG_BACKWARD) >= 0) {
        avcodec_flush_buffers(v->dec);
    }
}

/* The hardware decoder gave up on a stream it had accepted. Open the file
 * again in software and go back to where decoding started -- or, when frames
 * have been handed out, to the last of them, skipping it and those before --
 * so the caller sees the frames it would have seen. */
static int video_to_software(Video *v, char *err, size_t err_len) {
    const char *path = v->path;
    int64_t start = v->start_ts;
    int64_t last = v->last_ts;
    int handed_out = v->handed_out;
    video_close(v);
    int rc = video_open_as(path, v, 0, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    if (handed_out) {
        video_seek(v, last);
        v->skip_through = last;
        v->last_ts = last;
        v->handed_out = 1;
    } else if (start != AV_NOPTS_VALUE) {
        video_seek(v, start);
    }
    v->start_ts = start;
    return VB_AV_OK;
}

/* Decode the next frame into v->frame.
 * Returns 1 for a frame, 0 at the end of the stream, or a negative code. */
static int video_next(Video *v, char *err, size_t err_len) {
    for (;;) {
        int ret = avcodec_receive_frame(v->dec, v->frame);
        if (ret >= 0) {
            int64_t ts = v->frame->best_effort_timestamp;
            if (ts == AV_NOPTS_VALUE) {
                ts = v->frame->pts;
            }
            if (v->skip_through != AV_NOPTS_VALUE) {
                if (ts == AV_NOPTS_VALUE || ts <= v->skip_through) {
                    av_frame_unref(v->frame);
                    continue;
                }
                v->skip_through = AV_NOPTS_VALUE;
            }
            if (ts != AV_NOPTS_VALUE) {
                v->last_ts = ts;
                v->handed_out = 1;
            }
            return 1;
        }
        if (ret == AVERROR_EOF) {
            return 0;
        }
        if (ret != AVERROR(EAGAIN)) {
            if (v->hw_device) {
                int rc = video_to_software(v, err, err_len);
                if (rc != VB_AV_OK) {
                    return rc;
                }
                continue;
            }
            set_av_err(err, err_len, "decoding failed", ret);
            return VB_AV_ERR_DECODE;
        }
        if (v->draining) {
            return 0;
        }

        ret = av_read_frame(v->fmt, v->pkt);
        if (ret < 0) {
            /* End of file, or a read error the CLI would also stop at. Either
             * way the decoder may still hold frames, so drain it. */
            v->draining = 1;
            avcodec_send_packet(v->dec, NULL);
            continue;
        }
        if (v->pkt->stream_index == v->index) {
            /* A corrupt packet costs one frame rather than the whole
             * analysis. The CLI makes the same trade: it logs and carries on.
             * The hardware decoder's errors are not that, since it refuses
             * whole streams the software decoder reads. */
            ret = avcodec_send_packet(v->dec, v->pkt);
            if (ret < 0 && ret != AVERROR(EAGAIN) && v->hw_device) {
                av_packet_unref(v->pkt);
                int rc = video_to_software(v, err, err_len);
                if (rc != VB_AV_OK) {
                    return rc;
                }
                continue;
            }
        }
        av_packet_unref(v->pkt);
    }
}

/* A frame's presentation time in microseconds from the origin, or
 * AV_NOPTS_VALUE when the stream gave it none. */
static int64_t frame_us(const Video *v, const AVFrame *f) {
    int64_t ts = f->best_effort_timestamp;
    if (ts == AV_NOPTS_VALUE) {
        ts = f->pts;
    }
    if (ts == AV_NOPTS_VALUE) {
        return AV_NOPTS_VALUE;
    }
    return av_rescale_q(ts, v->st->time_base, AV_TIME_BASE_Q) - v->origin_us;
}

/*
 * The frame on screen at `at_us`, as a new reference in *out.
 *
 * Mirrors `ffmpeg -ss T -i file -frames:v 1`: seek to the keyframe at or
 * before T, then decode forward and return the first frame at or after T.
 * The CLI's seek is exact, not keyframe-rounded, and so is this one.
 */
static int decode_at(const char *path, int64_t at_us, Video *v, AVFrame **out, char *err,
                     size_t err_len) {
    /* Cleared first, so the caller can close it on every path out. */
    memset(v, 0, sizeof *v);
    *out = NULL;
    if (at_us < 0) {
        set_err(err, err_len, "cannot seek to a negative time (%lld us)", (long long)at_us);
        return VB_AV_ERR_ARG;
    }
    int rc = video_open(path, v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    /* Rounded down so the frame exactly at at_us is never skipped by a
     * one-tick rounding error in the conversion. */
    int64_t target = av_rescale_q_rnd(at_us + v->origin_us, AV_TIME_BASE_Q, v->st->time_base,
                                      AV_ROUND_DOWN);
    video_seek(v, target);

    AVFrame *last = av_frame_alloc();
    if (!last) {
        set_err(err, err_len, "out of memory decoding a frame");
        return VB_AV_ERR_DECODE;
    }
    int have_last = 0;

    for (;;) {
        rc = video_next(v, err, err_len);
        if (rc < 0) {
            av_frame_free(&last);
            return rc;
        }
        if (rc == 0) {
            break;
        }
        int64_t t = frame_us(v, v->frame);
        if (t != AV_NOPTS_VALUE && t >= at_us) {
            av_frame_free(&last);
            rc = video_pixels(v, err, err_len);
            if (rc != VB_AV_OK) {
                return rc;
            }
            *out = av_frame_clone(v->frame);
            if (!*out) {
                set_err(err, err_len, "out of memory decoding a frame");
                return VB_AV_ERR_DECODE;
            }
            return VB_AV_OK;
        }
        av_frame_unref(last);
        if (av_frame_ref(last, v->frame) >= 0) {
            have_last = 1;
        }
    }

    /* Past the last frame's timestamp but still inside the file: the last
     * frame is the one on screen for the rest of its duration. The CLI
     * returns nothing here, which only ever loses a sample from the final
     * frame of a clip. */
    int64_t duration = v->fmt->duration;
    if (have_last && (duration == AV_NOPTS_VALUE || at_us < duration)) {
        av_frame_unref(v->frame);
        av_frame_move_ref(v->frame, last);
        av_frame_free(&last);
        rc = video_pixels(v, err, err_len);
        if (rc != VB_AV_OK) {
            return rc;
        }
        *out = av_frame_clone(v->frame);
        if (!*out) {
            set_err(err, err_len, "out of memory decoding a frame");
            return VB_AV_ERR_DECODE;
        }
        return VB_AV_OK;
    }
    av_frame_free(&last);
    set_err(err, err_len, "no frame at %lld us; that is past the end of the footage",
            (long long)at_us);
    return VB_AV_ERR_PAST_END;
}

/* Scale and convert a decoded frame into caller-provided planes. */
/*
 * A scaler from `src` to `dst_format` at width x height, its ranges set
 * before it starts. Set afterwards they are ignored when nothing is scaled:
 * at the same size and format swscale copies the planes, and a full-size
 * still came out with limited-range samples in a full-range JPEG -- every
 * black 16 levels up.
 */
static struct SwsContext *scaler(const AVFrame *src, enum AVPixelFormat dst_format, int width,
                                 int height, int dst_full_range) {
    struct SwsContext *sws = sws_alloc_context();
    if (!sws) {
        return NULL;
    }
    if (av_opt_set_int(sws, "srcw", src->width, 0) < 0 ||
        av_opt_set_int(sws, "srch", src->height, 0) < 0 ||
        av_opt_set_int(sws, "src_format", src->format, 0) < 0 ||
        av_opt_set_int(sws, "dstw", width, 0) < 0 || av_opt_set_int(sws, "dsth", height, 0) < 0 ||
        av_opt_set_int(sws, "dst_format", dst_format, 0) < 0 ||
        av_opt_set_int(sws, "sws_flags", SWS_BICUBIC, 0) < 0 ||
        av_opt_set_int(sws, "src_range", src->color_range == AVCOL_RANGE_JPEG, 0) < 0 ||
        av_opt_set_int(sws, "dst_range", dst_full_range, 0) < 0 ||
        sws_init_context(sws, NULL, NULL) < 0) {
        sws_freeContext(sws);
        return NULL;
    }
    return sws;
}

/* Honour the range the source declares. Unspecified means limited, as it
 * does everywhere in libav, including the CLI's scale filter. */
static void source_colours(struct SwsContext *sws, const AVFrame *src, int dst_full_range) {
    int *inv_table, *table, src_range, dst_range, brightness, contrast, saturation;
    if (sws_getColorspaceDetails(sws, &inv_table, &src_range, &table, &dst_range, &brightness,
                                 &contrast, &saturation) >= 0) {
        /* And the matrix it declares, as the CLI's scale filter does: an HD
         * phone clip is BT.709, and reading it as BT.601 shifts its colours
         * where RGB comes out. Greyscale is unaffected either way. */
        const int *coefficients = inv_table;
        if (src->colorspace != AVCOL_SPC_UNSPECIFIED && src->colorspace != AVCOL_SPC_RGB) {
            coefficients = sws_getCoefficients(src->colorspace);
        }
        sws_setColorspaceDetails(sws, coefficients, src->color_range == AVCOL_RANGE_JPEG, table,
                                 dst_full_range, brightness, contrast, saturation);
    }
}

static int convert(const AVFrame *src, enum AVPixelFormat dst_format, int width, int height,
                   int dst_full_range, uint8_t *const dst[4], const int dst_stride[4], char *err,
                   size_t err_len) {
    struct SwsContext *sws = scaler(src, dst_format, width, height, dst_full_range);
    if (!sws) {
        const char *from = av_get_pix_fmt_name((enum AVPixelFormat)src->format);
        set_err(err, err_len, "cannot convert %s frames to %s", from ? from : "unknown",
                av_get_pix_fmt_name(dst_format));
        return VB_AV_ERR_DECODE;
    }

    source_colours(sws, src, dst_full_range);

    int rows = sws_scale(sws, (const uint8_t *const *)src->data, src->linesize, 0, src->height,
                         dst, dst_stride);
    sws_freeContext(sws);
    if (rows <= 0) {
        set_err(err, err_len, "scaling the frame produced no output");
        return VB_AV_ERR_DECODE;
    }
    return VB_AV_OK;
}

/* ------------------------------------------------------------------------ */
/* Probe                                                                     */
/* ------------------------------------------------------------------------ */

/* The clockwise turn that shows a stream upright: 0, 90, 180 or 270. The
 * same arithmetic as the CLI's get_rotation, so the two agree on every file,
 * including the phones that write -90 where others write 270. */
static int stream_rotation(const AVStream *st) {
    const AVPacketSideData *sd =
        av_packet_side_data_get(st->codecpar->coded_side_data, st->codecpar->nb_coded_side_data,
                                AV_PKT_DATA_DISPLAYMATRIX);
    if (!sd || sd->size < 9 * sizeof(int32_t)) {
        return 0;
    }
    double theta = -round(av_display_rotation_get((const int32_t *)sd->data));
    theta -= 360 * floor(theta / 360 + 0.9 / 360);
    if (fabs(theta - 90) < 1.0) {
        return 90;
    }
    if (fabs(theta - 180) < 1.0) {
        return 180;
    }
    if (fabs(theta - 270) < 1.0) {
        return 270;
    }
    return 0;
}

int vb_av_probe(const char *path, vb_av_info *out, char *err, size_t err_len) {
    if (!path || !out) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    memset(out, 0, sizeof *out);

    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    int vi = find_stream(fmt, AVMEDIA_TYPE_VIDEO);
    if (vi < 0) {
        avformat_close_input(&fmt);
        set_err(err, err_len, "the file has no video stream");
        return VB_AV_ERR_NO_VIDEO;
    }
    const AVStream *vs = fmt->streams[vi];

    /* The container duration is already in microseconds (AV_TIME_BASE), so
     * this is exact. Prefer it for the same reason the CLI backend does: a
     * stream's own duration is often absent or wrong in fragmented files. */
    if (fmt->duration != AV_NOPTS_VALUE && fmt->duration > 0) {
        out->duration_us = fmt->duration;
    } else if (vs->duration != AV_NOPTS_VALUE && vs->duration > 0) {
        out->duration_us = av_rescale_q(vs->duration, vs->time_base, AV_TIME_BASE_Q);
    }

    /* Kept as an exact rational: 30000/1001 is not representable as a float,
     * and rounding it is how frame-accurate edits stop being frame-accurate. */
    AVRational rate = vs->r_frame_rate;
    if (rate.num > 0 && rate.den > 0) {
        out->fps_num = (uint32_t)rate.num;
        out->fps_den = (uint32_t)rate.den;
    } else {
        out->fps_num = 30;
        out->fps_den = 1;
    }

    out->width = vs->codecpar->width > 0 ? (uint32_t)vs->codecpar->width : 0;
    out->height = vs->codecpar->height > 0 ? (uint32_t)vs->codecpar->height : 0;
    av_strlcpy(out->video_codec, avcodec_get_name(vs->codecpar->codec_id), sizeof out->video_codec);

    int ai = find_stream(fmt, AVMEDIA_TYPE_AUDIO);
    if (ai >= 0) {
        out->has_audio = 1;
        av_strlcpy(out->audio_codec, avcodec_get_name(fmt->streams[ai]->codecpar->codec_id),
                   sizeof out->audio_codec);
    }

    out->rotation = stream_rotation(vs);
    const AVDictionaryEntry *created = av_dict_get(fmt->metadata, "creation_time", NULL, 0);
    if (created && created->value) {
        av_strlcpy(out->created_at, created->value, sizeof out->created_at);
    }
    static const char *const location_tags[] = {"location", "com.apple.quicktime.location.ISO6709",
                                                 "location-eng"};
    for (size_t i = 0; i < sizeof location_tags / sizeof *location_tags; i++) {
        const AVDictionaryEntry *where = av_dict_get(fmt->metadata, location_tags[i], NULL, 0);
        if (where && where->value && where->value[0]) {
            av_strlcpy(out->location, where->value, sizeof out->location);
            break;
        }
    }

    avformat_close_input(&fmt);
    return VB_AV_OK;
}

/* ------------------------------------------------------------------------ */
/* Scene detection                                                           */
/* ------------------------------------------------------------------------ */

typedef struct {
    AVFilterGraph *graph;
    AVFilterContext *src;
    AVFilterContext *sink;
    int width;
    int height;
    int format;
} SceneGraph;

static void scene_graph_free(SceneGraph *g) {
    avfilter_graph_free(&g->graph);
    g->src = NULL;
    g->sink = NULL;
}

/*
 * buffer -> select -> buffersink, the same `select` filter the CLI backend
 * runs, so both report ffmpeg's own scene score rather than two different
 * approximations of it.
 */
static int scene_graph_init(SceneGraph *g, const Video *v, const AVFrame *f, double threshold,
                            char *err, size_t err_len) {
    g->graph = avfilter_graph_alloc();
    if (!g->graph) {
        set_err(err, err_len, "out of memory building the scene filter");
        return VB_AV_ERR_DECODE;
    }

    AVRational tb = v->st->time_base;
    AVRational sar = f->sample_aspect_ratio;
    if (sar.num <= 0 || sar.den <= 0) {
        sar = (AVRational){0, 1};
    }
    char args[256];
    snprintf(args, sizeof args, "video_size=%dx%d:pix_fmt=%d:time_base=%d/%d:pixel_aspect=%d/%d",
             f->width, f->height, f->format, tb.num, tb.den, sar.num, sar.den);

    int ret = avfilter_graph_create_filter(&g->src, avfilter_get_by_name("buffer"), "in", args,
                                           NULL, g->graph);
    if (ret >= 0) {
        ret = avfilter_graph_create_filter(&g->sink, avfilter_get_by_name("buffersink"), "out",
                                           NULL, NULL, g->graph);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not create the scene filter endpoints", ret);
        scene_graph_free(g);
        return VB_AV_ERR_DECODE;
    }

    /* The threshold goes in as a ratio of integers because the printf family
     * formats doubles through the C locale. A host that has set a
     * comma-decimal locale would otherwise hand the filter "gt(scene,0,3)",
     * which the expression parser reads as a different function call. */
    char desc[96];
    long millionths = (long)(threshold * 1e6 + 0.5); /* threshold is clamped to 0..1 */
    snprintf(desc, sizeof desc, "select='gt(scene,%ld/1000000)'", millionths);

    AVFilterInOut *outputs = avfilter_inout_alloc();
    AVFilterInOut *inputs = avfilter_inout_alloc();
    if (!outputs || !inputs) {
        avfilter_inout_free(&outputs);
        avfilter_inout_free(&inputs);
        set_err(err, err_len, "out of memory building the scene filter");
        scene_graph_free(g);
        return VB_AV_ERR_DECODE;
    }
    outputs->name = av_strdup("in");
    outputs->filter_ctx = g->src;
    outputs->pad_idx = 0;
    outputs->next = NULL;
    inputs->name = av_strdup("out");
    inputs->filter_ctx = g->sink;
    inputs->pad_idx = 0;
    inputs->next = NULL;

    ret = avfilter_graph_parse_ptr(g->graph, desc, &inputs, &outputs, NULL);
    avfilter_inout_free(&inputs);
    avfilter_inout_free(&outputs);
    if (ret >= 0) {
        ret = avfilter_graph_config(g->graph, NULL);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not configure the scene filter", ret);
        scene_graph_free(g);
        return VB_AV_ERR_DECODE;
    }

    g->width = f->width;
    g->height = f->height;
    g->format = f->format;
    return VB_AV_OK;
}

/* Pull every frame the graph has ready and report each one as a cut. */
static int scene_graph_drain(SceneGraph *g, const Video *v, AVFrame *out, vb_av_scene_cb cb,
                             void *ctx, char *err, size_t err_len) {
    AVRational tb = av_buffersink_get_time_base(g->sink);
    for (;;) {
        int ret = av_buffersink_get_frame(g->sink, out);
        if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
            return VB_AV_OK;
        }
        if (ret < 0) {
            set_av_err(err, err_len, "the scene filter failed", ret);
            return VB_AV_ERR_DECODE;
        }
        /* A frame with no timestamp cannot become a cut: inventing a time for
         * it would put a shot boundary somewhere the footage never cut. */
        const AVDictionaryEntry *e = av_dict_get(out->metadata, "lavfi.scene_score", NULL, 0);
        if (e && out->pts != AV_NOPTS_VALUE) {
            double score = strtod(e->value, NULL);
            int64_t at = av_rescale_q(out->pts, tb, AV_TIME_BASE_Q) - v->origin_us;
            cb(ctx, at, score);
        }
        av_frame_unref(out);
    }
}

/* Signal end of input and collect whatever the graph still holds. */
static int scene_graph_finish(SceneGraph *g, const Video *v, AVFrame *out, vb_av_scene_cb cb,
                              void *ctx, char *err, size_t err_len) {
    int ret = av_buffersrc_add_frame(g->src, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not flush the scene filter", ret);
        return VB_AV_ERR_DECODE;
    }
    return scene_graph_drain(g, v, out, cb, ctx, err, err_len);
}

int vb_av_detect_scenes(const char *path, double threshold, vb_av_scene_cb cb, void *ctx,
                        char *err, size_t err_len) {
    if (!path || !cb || threshold != threshold /* NaN */) {
        set_err(err, err_len, "bad argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    if (threshold < 0.0) {
        threshold = 0.0;
    } else if (threshold > 1.0) {
        threshold = 1.0;
    }

    Video v;
    int rc = video_open(path, &v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    SceneGraph g = {0};
    AVFrame *out = av_frame_alloc();
    if (!out) {
        video_close(&v);
        set_err(err, err_len, "out of memory detecting scenes");
        return VB_AV_ERR_DECODE;
    }

    while ((rc = video_next(&v, err, err_len)) == 1) {
        rc = video_pixels(&v, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
        AVFrame *f = v.frame;
        f->pts = f->best_effort_timestamp;

        if (!g.graph || f->width != g.width || f->height != g.height || f->format != g.format) {
            /* A mid-stream change of size or format needs a fresh graph, as it
             * does in the CLI. The first frame after it has no predecessor,
             * so it cannot score as a cut: the same trade the CLI makes. */
            if (g.graph) {
                rc = scene_graph_finish(&g, &v, out, cb, ctx, err, err_len);
                scene_graph_free(&g);
                if (rc != VB_AV_OK) {
                    break;
                }
            }
            rc = scene_graph_init(&g, &v, f, threshold, err, err_len);
            if (rc != VB_AV_OK) {
                break;
            }
        }

        int ret = av_buffersrc_add_frame(g.src, f);
        if (ret < 0) {
            set_av_err(err, err_len, "could not feed the scene filter", ret);
            rc = VB_AV_ERR_DECODE;
            break;
        }
        rc = scene_graph_drain(&g, &v, out, cb, ctx, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
    }

    if (rc == 0 && g.graph) {
        rc = scene_graph_finish(&g, &v, out, cb, ctx, err, err_len);
    }

    av_frame_free(&out);
    scene_graph_free(&g);
    video_close(&v);
    return rc < 0 ? rc : VB_AV_OK;
}

/* ------------------------------------------------------------------------ */
/* Frames and stills                                                         */
/* ------------------------------------------------------------------------ */

int vb_av_gray_frame(const char *path, int64_t at_us, uint32_t width, uint32_t height,
                     uint8_t *out, char *err, size_t err_len) {
    if (!path || !out || width == 0 || height == 0 || width > 16384 || height > 16384) {
        set_err(err, err_len, "bad argument: a %ux%u frame", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();

    Video v;
    AVFrame *frame = NULL;
    int rc = decode_at(path, at_us, &v, &frame, err, err_len);
    if (rc == VB_AV_OK) {
        uint8_t *planes[4] = {out, NULL, NULL, NULL};
        int strides[4] = {(int)width, 0, 0, 0};
        /* Full-range grey, as the CLI produces: a limited-range source is
         * expanded so black is 0 and white 255. Measured, not assumed --
         * keeping the source range leaves every frame about eight levels
         * off the CLI's and compresses the exposure score's scale. */
        rc = convert(frame, AV_PIX_FMT_GRAY8, (int)width, (int)height, 1, planes, strides, err,
                     err_len);
    }
    av_frame_free(&frame);
    video_close(&v);
    return rc;
}

/* Turn one plane of `px`-byte samples, w x h with `src_stride`, clockwise by
 * `degrees` (90, 180 or 270) into `dst`. */
static void turn_plane(const uint8_t *src, int src_stride, int w, int h, int px, int degrees,
                       uint8_t *dst, int dst_stride) {
    int dw = (degrees == 180) ? w : h;
    int dh = (degrees == 180) ? h : w;
    for (int y = 0; y < dh; y++) {
        for (int x = 0; x < dw; x++) {
            int sx, sy;
            if (degrees == 90) {
                sx = y;
                sy = h - 1 - x;
            } else if (degrees == 270) {
                sx = w - 1 - y;
                sy = x;
            } else {
                sx = w - 1 - x;
                sy = h - 1 - y;
            }
            memcpy(dst + (size_t)y * dst_stride + (size_t)x * px,
                   src + (size_t)sy * src_stride + (size_t)sx * px, (size_t)px);
        }
    }
}

/* The decoded frame turned upright, as a new frame, or NULL when its format
 * is not one this can turn (then the caller turns the scaled image instead).
 * Turning before scaling is what the CLI's autorotate does, and doing the
 * same keeps the two backends' pixels together. */
static AVFrame *turned(const AVFrame *src, int degrees) {
    const AVPixFmtDescriptor *desc = av_pix_fmt_desc_get((enum AVPixelFormat)src->format);
    if (!desc || (desc->flags & (AV_PIX_FMT_FLAG_BITSTREAM | AV_PIX_FMT_FLAG_PAL |
                                 AV_PIX_FMT_FLAG_HWACCEL)) ||
        !(desc->flags & AV_PIX_FMT_FLAG_PLANAR) || desc->log2_chroma_w != desc->log2_chroma_h) {
        return NULL;
    }
    int px = (desc->comp[0].depth + 7) / 8;
    for (int c = 0; c < desc->nb_components; c++) {
        if ((desc->comp[c].depth + 7) / 8 != px || desc->comp[c].step != px) {
            return NULL;
        }
    }
    AVFrame *out = av_frame_alloc();
    if (!out) {
        return NULL;
    }
    int quarter = degrees == 90 || degrees == 270;
    out->format = src->format;
    out->width = quarter ? src->height : src->width;
    out->height = quarter ? src->width : src->height;
    if (av_frame_get_buffer(out, 0) < 0 || av_frame_copy_props(out, src) < 0) {
        av_frame_free(&out);
        return NULL;
    }
    if (quarter && out->sample_aspect_ratio.num > 0) {
        out->sample_aspect_ratio = av_inv_q(src->sample_aspect_ratio);
    }
    int planes = av_pix_fmt_count_planes((enum AVPixelFormat)src->format);
    for (int p = 0; p < planes; p++) {
        int chroma = (p == 1 || p == 2);
        int w = chroma ? AV_CEIL_RSHIFT(src->width, desc->log2_chroma_w) : src->width;
        int h = chroma ? AV_CEIL_RSHIFT(src->height, desc->log2_chroma_h) : src->height;
        turn_plane(src->data[p], src->linesize[p], w, h, px, degrees, out->data[p],
                   out->linesize[p]);
    }
    return out;
}

int vb_av_sample_frames(const char *path, int64_t min_gap_us, uint32_t width, uint32_t height,
                        vb_av_frame_cb cb, void *ctx, size_t *count, char *err,
                        size_t err_len) {
    if (!path || !cb || !count || width == 0 || height == 0 || width > 4096 || height > 4096 ||
        min_gap_us < 0) {
        set_err(err, err_len, "bad argument: a %ux%u sweep", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *count = 0;

    Video v;
    int rc = video_open(path, &v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    /* Turned upright as the CLI's autorotate turns it: the decoded frame is
     * turned, then scaled. */
    int rotation = stream_rotation(v.st);
    uint8_t *rgb = av_malloc((size_t)width * height * 3);
    if (!rgb) {
        video_close(&v);
        set_err(err, err_len, "out of memory sweeping the frames");
        return VB_AV_ERR_DECODE;
    }

    int have_last = 0;
    int64_t last_us = 0;
    while ((rc = video_next(&v, err, err_len)) == 1) {
        int64_t t = frame_us(&v, v.frame);
        if (t == AV_NOPTS_VALUE) {
            continue;
        }
        /* The rule the CLI's select expression applies: the first frame,
         * then any frame at least the gap after the last one taken. */
        if (have_last && t - last_us < min_gap_us) {
            continue;
        }
        rc = video_pixels(&v, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
        AVFrame *upright = rotation ? turned(v.frame, rotation) : NULL;
        if (rotation && !upright) {
            set_err(err, err_len, "cannot turn %s frames upright",
                    av_get_pix_fmt_name((enum AVPixelFormat)v.frame->format));
            rc = VB_AV_ERR_UNSUPPORTED;
            break;
        }
        uint8_t *planes[4] = {rgb, NULL, NULL, NULL};
        int strides[4] = {(int)width * 3, 0, 0, 0};
        rc = convert(upright ? upright : v.frame, AV_PIX_FMT_RGB24, (int)width, (int)height, 1,
                     planes, strides, err, err_len);
        av_frame_free(&upright);
        if (rc != VB_AV_OK) {
            break;
        }
        have_last = 1;
        last_us = t;
        (*count)++;
        /* A frame before the container's start is clamped to it, as the CLI
         * reports it. */
        if (!cb(ctx, t < 0 ? 0 : t, rgb)) {
            rc = 0;
            break;
        }
    }

    av_free(rgb);
    video_close(&v);
    return rc < 0 ? rc : VB_AV_OK;
}

/*
 * Scene detection and the sweep from one decode.
 *
 * The two used to decode the whole file once each; on 4K footage the decode
 * is nearly all of the cost, so doing both from the same frames halves the
 * analysis. Each does exactly what its own entry point does -- the same
 * filter over every frame, the same sampling rule -- so the cuts and the
 * frames are the ones the two separate calls would return.
 */
int vb_av_scan(const char *path, double threshold, vb_av_scene_cb scene_cb, void *scene_ctx,
               int64_t min_gap_us, uint32_t width, uint32_t height, vb_av_frame_cb frame_cb,
               void *frame_ctx, size_t *count, char *err, size_t err_len) {
    if (!path || !scene_cb || !frame_cb || !count || threshold != threshold /* NaN */ ||
        width == 0 || height == 0 || width > 4096 || height > 4096 || min_gap_us < 0) {
        set_err(err, err_len, "bad argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *count = 0;
    if (threshold < 0.0) {
        threshold = 0.0;
    } else if (threshold > 1.0) {
        threshold = 1.0;
    }

    Video v;
    int rc = video_open(path, &v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    int rotation = stream_rotation(v.st);
    uint8_t *rgb = av_malloc((size_t)width * height * 3);
    AVFrame *out = av_frame_alloc();
    if (!rgb || !out) {
        av_free(rgb);
        av_frame_free(&out);
        video_close(&v);
        set_err(err, err_len, "out of memory scanning the video");
        return VB_AV_ERR_DECODE;
    }

    SceneGraph g = {0};
    int have_last = 0;
    int64_t last_us = 0;
    int stopped = 0;
    while ((rc = video_next(&v, err, err_len)) == 1) {
        rc = video_pixels(&v, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
        AVFrame *f = v.frame;

        /* The sweep first: the scene filter takes the frame over. */
        int64_t t = frame_us(&v, f);
        if (t != AV_NOPTS_VALUE && !(have_last && t - last_us < min_gap_us)) {
            AVFrame *upright = rotation ? turned(f, rotation) : NULL;
            if (rotation && !upright) {
                set_err(err, err_len, "cannot turn %s frames upright",
                        av_get_pix_fmt_name((enum AVPixelFormat)f->format));
                rc = VB_AV_ERR_UNSUPPORTED;
                break;
            }
            uint8_t *planes[4] = {rgb, NULL, NULL, NULL};
            int strides[4] = {(int)width * 3, 0, 0, 0};
            rc = convert(upright ? upright : f, AV_PIX_FMT_RGB24, (int)width, (int)height, 1,
                         planes, strides, err, err_len);
            av_frame_free(&upright);
            if (rc != VB_AV_OK) {
                break;
            }
            have_last = 1;
            last_us = t;
            (*count)++;
            if (!frame_cb(frame_ctx, t < 0 ? 0 : t, rgb)) {
                stopped = 1;
                rc = 0;
                break;
            }
        }

        f->pts = f->best_effort_timestamp;
        if (!g.graph || f->width != g.width || f->height != g.height || f->format != g.format) {
            if (g.graph) {
                rc = scene_graph_finish(&g, &v, out, scene_cb, scene_ctx, err, err_len);
                scene_graph_free(&g);
                if (rc != VB_AV_OK) {
                    break;
                }
            }
            rc = scene_graph_init(&g, &v, f, threshold, err, err_len);
            if (rc != VB_AV_OK) {
                break;
            }
        }
        int ret = av_buffersrc_add_frame(g.src, f);
        if (ret < 0) {
            set_av_err(err, err_len, "could not feed the scene filter", ret);
            rc = VB_AV_ERR_DECODE;
            break;
        }
        rc = scene_graph_drain(&g, &v, out, scene_cb, scene_ctx, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
    }

    if (rc == 0 && !stopped && g.graph) {
        rc = scene_graph_finish(&g, &v, out, scene_cb, scene_ctx, err, err_len);
    }

    av_frame_free(&out);
    scene_graph_free(&g);
    av_free(rgb);
    video_close(&v);
    return rc < 0 ? rc : VB_AV_OK;
}

/*
 * The sweep's rule over one stretch of the file: seek to the keyframe at or
 * before `from_us`, decode forward, and hand over an upright colour frame
 * for the first frame at or after `from_us` and then every `min_gap_us`,
 * until `to_us`. For reading a few seconds densely -- body poses where a
 * dance cut may join -- without seeking once per frame.
 */
int vb_av_sample_range(const char *path, int64_t from_us, int64_t to_us, int64_t min_gap_us,
                       uint32_t width, uint32_t height, vb_av_frame_cb cb, void *ctx,
                       size_t *count, char *err, size_t err_len) {
    if (!path || !cb || !count || width == 0 || height == 0 || width > 4096 || height > 4096 ||
        min_gap_us < 0 || from_us < 0 || to_us < from_us) {
        set_err(err, err_len, "bad argument: a %ux%u range", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *count = 0;

    Video v;
    int rc = video_open(path, &v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    int64_t target = av_rescale_q_rnd(from_us + v.origin_us, AV_TIME_BASE_Q, v.st->time_base,
                                      AV_ROUND_DOWN);
    video_seek(&v, target);
    int rotation = stream_rotation(v.st);
    uint8_t *rgb = av_malloc((size_t)width * height * 3);
    if (!rgb) {
        video_close(&v);
        set_err(err, err_len, "out of memory reading a range");
        return VB_AV_ERR_DECODE;
    }

    int have_last = 0;
    int64_t last_us = 0;
    while ((rc = video_next(&v, err, err_len)) == 1) {
        int64_t t = frame_us(&v, v.frame);
        if (t == AV_NOPTS_VALUE || t < from_us) {
            continue;
        }
        if (t > to_us) {
            rc = 0;
            break;
        }
        if (have_last && t - last_us < min_gap_us) {
            continue;
        }
        rc = video_pixels(&v, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
        AVFrame *upright = rotation ? turned(v.frame, rotation) : NULL;
        if (rotation && !upright) {
            set_err(err, err_len, "cannot turn %s frames upright",
                    av_get_pix_fmt_name((enum AVPixelFormat)v.frame->format));
            rc = VB_AV_ERR_UNSUPPORTED;
            break;
        }
        uint8_t *planes[4] = {rgb, NULL, NULL, NULL};
        int strides[4] = {(int)width * 3, 0, 0, 0};
        rc = convert(upright ? upright : v.frame, AV_PIX_FMT_RGB24, (int)width, (int)height, 1,
                     planes, strides, err, err_len);
        av_frame_free(&upright);
        if (rc != VB_AV_OK) {
            break;
        }
        have_last = 1;
        last_us = t;
        (*count)++;
        if (!cb(ctx, t, rgb)) {
            rc = 0;
            break;
        }
    }

    av_free(rgb);
    video_close(&v);
    return rc < 0 ? rc : VB_AV_OK;
}

int vb_av_rgb_frame(const char *path, int64_t at_us, uint32_t width, uint32_t height,
                    uint8_t *out, char *err, size_t err_len) {
    if (!path || !out || width == 0 || height == 0 || width > 16384 || height > 16384) {
        set_err(err, err_len, "bad argument: a %ux%u frame", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();

    Video v;
    AVFrame *frame = NULL;
    int rc = decode_at(path, at_us, &v, &frame, err, err_len);
    if (rc == VB_AV_OK) {
        /* Upright as the sweep makes it, and as the CLI's autorotate does:
         * the decoded frame is turned, then scaled. */
        int rotation = stream_rotation(v.st);
        AVFrame *upright = rotation ? turned(frame, rotation) : NULL;
        if (rotation && !upright) {
            set_err(err, err_len, "cannot turn %s frames upright",
                    av_get_pix_fmt_name((enum AVPixelFormat)frame->format));
            rc = VB_AV_ERR_UNSUPPORTED;
        } else {
            uint8_t *planes[4] = {out, NULL, NULL, NULL};
            int strides[4] = {(int)width * 3, 0, 0, 0};
            rc = convert(upright ? upright : frame, AV_PIX_FMT_RGB24, (int)width, (int)height, 1,
                         planes, strides, err, err_len);
        }
        av_frame_free(&upright);
    }
    av_frame_free(&frame);
    video_close(&v);
    return rc;
}

/* Write an encoded packet out verbatim: a JPEG or PNG encoder's packet is
 * already a complete image file. avio rather than stdio because avio accepts
 * UTF-8 paths on Windows, where fopen does not. */
static int write_packet(const char *dest, const AVPacket *pkt, char *err, size_t err_len) {
    AVIOContext *io = NULL;
    int ret = avio_open(&io, dest, AVIO_FLAG_WRITE);
    if (ret < 0) {
        set_av_err(err, err_len, "could not create the still", ret);
        return VB_AV_ERR_IO;
    }
    avio_write(io, pkt->data, pkt->size);
    ret = avio_closep(&io);
    if (ret < 0) {
        set_av_err(err, err_len, "could not write the still", ret);
        return VB_AV_ERR_IO;
    }
    return VB_AV_OK;
}

static int encode_still(const AVFrame *frame, const char *dest, enum AVCodecID codec_id,
                        enum AVPixelFormat pix_fmt, int width, int height, char *err,
                        size_t err_len) {
    const AVCodec *codec = avcodec_find_encoder(codec_id);
    if (!codec) {
        set_err(err, err_len, "this build has no %s encoder", avcodec_get_name(codec_id));
        return VB_AV_ERR_UNSUPPORTED;
    }

    int rc = VB_AV_ERR_DECODE;
    int ret;
    AVCodecContext *enc = avcodec_alloc_context3(codec);
    AVFrame *pic = av_frame_alloc();
    AVPacket *pkt = av_packet_alloc();
    if (!enc || !pic || !pkt) {
        set_err(err, err_len, "out of memory encoding the still");
        goto done;
    }

    enc->width = width;
    enc->height = height;
    enc->pix_fmt = pix_fmt;
    enc->time_base = (AVRational){1, 25};
    enc->sample_aspect_ratio = frame->sample_aspect_ratio;
    /* Full range, which JPEG expects. Declaring it lets MJPEG take plain
     * yuv420p rather than the deprecated yuvj420p. */
    enc->color_range = AVCOL_RANGE_JPEG;
    if (codec_id == AV_CODEC_ID_MJPEG) {
        /* The CLI backend passes `-q:v 3`; this is the same setting. */
        enc->flags |= AV_CODEC_FLAG_QSCALE;
        enc->global_quality = FF_QP2LAMBDA * 3;
    }

    ret = avcodec_open2(enc, codec, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the still encoder", ret);
        goto done;
    }

    pic->format = pix_fmt;
    pic->width = width;
    pic->height = height;
    pic->color_range = AVCOL_RANGE_JPEG;
    ret = av_frame_get_buffer(pic, 0);
    if (ret < 0) {
        set_av_err(err, err_len, "could not allocate the still", ret);
        goto done;
    }
    rc = convert(frame, pix_fmt, width, height, 1, pic->data, pic->linesize, err, err_len);
    if (rc != VB_AV_OK) {
        goto done;
    }
    rc = VB_AV_ERR_DECODE;
    pic->pts = 0;
    pic->quality = enc->global_quality;

    ret = avcodec_send_frame(enc, pic);
    if (ret >= 0) {
        ret = avcodec_send_frame(enc, NULL);
    }
    if (ret >= 0) {
        ret = avcodec_receive_packet(enc, pkt);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not encode the still", ret);
        goto done;
    }
    rc = write_packet(dest, pkt, err, err_len);

done:
    av_packet_free(&pkt);
    av_frame_free(&pic);
    avcodec_free_context(&enc);
    return rc;
}

/* The encoder and pixel format for a still, from the extension of `dest`. */
static int still_format(const char *dest, enum AVCodecID *codec_id, enum AVPixelFormat *pix_fmt,
                        char *err, size_t err_len) {
    const char *ext = strrchr(dest, '.');
    if (ext && (!av_strcasecmp(ext, ".jpg") || !av_strcasecmp(ext, ".jpeg"))) {
        *codec_id = AV_CODEC_ID_MJPEG;
        *pix_fmt = AV_PIX_FMT_YUV420P;
        return VB_AV_OK;
    }
    if (ext && !av_strcasecmp(ext, ".png")) {
        *codec_id = AV_CODEC_ID_PNG;
        *pix_fmt = AV_PIX_FMT_RGB24;
        return VB_AV_OK;
    }
    set_err(err, err_len, "cannot tell the still format from %s; use .jpg or .png", dest);
    return VB_AV_ERR_UNSUPPORTED;
}

int vb_av_extract_still(const char *path, int64_t at_us, const char *dest, char *err,
                        size_t err_len) {
    if (!path || !dest) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();

    enum AVCodecID codec_id;
    enum AVPixelFormat pix_fmt;
    int rc = still_format(dest, &codec_id, &pix_fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    Video v;
    AVFrame *frame = NULL;
    rc = decode_at(path, at_us, &v, &frame, err, err_len);
    if (rc == VB_AV_OK) {
        /* Upright, as the CLI's autorotate writes it: a phone held upright
         * gives a tall still, not one on its side. Every model that reads
         * stills -- labels, faces, the judge, captions -- reads it as it is. */
        int rotation = stream_rotation(v.st);
        AVFrame *upright = rotation ? turned(frame, rotation) : NULL;
        if (rotation && !upright) {
            set_err(err, err_len, "cannot turn %s frames upright",
                    av_get_pix_fmt_name((enum AVPixelFormat)frame->format));
            rc = VB_AV_ERR_UNSUPPORTED;
        } else {
            const AVFrame *pic = upright ? upright : frame;
            rc = encode_still(pic, dest, codec_id, pix_fmt, pic->width, pic->height, err,
                              err_len);
        }
        av_frame_free(&upright);
    }
    av_frame_free(&frame);
    video_close(&v);
    return rc;
}

/* ------------------------------------------------------------------------ */
/* Photographs                                                               */
/* ------------------------------------------------------------------------ */

/*
 * A photograph is one picture. JPEG, PNG and WebP arrive through the image
 * demuxers as a one-frame video stream. HEIF -- what an iPhone takes -- is a
 * grid of HEVC tiles, each a stream of its own, joined by a tile-grid stream
 * group; the CLI assembles the grid itself, and this assembles it the same
 * way: every tile decoded, pasted at its offset, the canvas cropped to the
 * picture.
 *
 * Nothing here decides which way up a photograph is. That comes from its
 * EXIF, which the Rust side reads for both backends and passes in as
 * `rotation`; the CLI backend runs with its own autorotate off for the same
 * reason.
 */

/* The tile grid of a HEIF picture, or NULL for anything else. */
static const AVStreamGroup *tile_grid(const AVFormatContext *fmt) {
    for (unsigned i = 0; i < fmt->nb_stream_groups; i++) {
        const AVStreamGroup *g = fmt->stream_groups[i];
        if (g->type == AV_STREAM_GROUP_PARAMS_TILE_GRID && g->params.tile_grid &&
            g->params.tile_grid->nb_tiles > 0 && g->nb_streams > 0) {
            return g;
        }
    }
    return NULL;
}

/* Copy `tile` into `canvas` with its top left at (x, y), clipped to the
 * canvas. Planar formats only, which is what the software decoders give. */
static int paste_tile(const AVFrame *tile, AVFrame *canvas, int x, int y) {
    const AVPixFmtDescriptor *desc = av_pix_fmt_desc_get((enum AVPixelFormat)tile->format);
    if (!desc || !(desc->flags & AV_PIX_FMT_FLAG_PLANAR) ||
        (desc->flags & (AV_PIX_FMT_FLAG_HWACCEL | AV_PIX_FMT_FLAG_BITSTREAM))) {
        return AVERROR(ENOSYS);
    }
    int planes = av_pix_fmt_count_planes((enum AVPixelFormat)tile->format);
    for (int p = 0; p < planes; p++) {
        int px = 0;
        for (int c = 0; c < desc->nb_components; c++) {
            if (desc->comp[c].plane == p) {
                px = desc->comp[c].step;
            }
        }
        int chroma = (p == 1 || p == 2);
        int sw = chroma ? desc->log2_chroma_w : 0;
        int sh = chroma ? desc->log2_chroma_h : 0;
        int cx = x >> sw, cy = y >> sh;
        int w = AV_CEIL_RSHIFT(tile->width, sw), h = AV_CEIL_RSHIFT(tile->height, sh);
        int cw = AV_CEIL_RSHIFT(canvas->width, sw), ch = AV_CEIL_RSHIFT(canvas->height, sh);
        if (px <= 0 || cx >= cw || cy >= ch) {
            continue;
        }
        w = FFMIN(w, cw - cx);
        h = FFMIN(h, ch - cy);
        for (int row = 0; row < h; row++) {
            memcpy(canvas->data[p] + (size_t)(cy + row) * canvas->linesize[p] + (size_t)cx * px,
                   tile->data[p] + (size_t)row * tile->linesize[p], (size_t)w * px);
        }
    }
    return 0;
}

/* Decode a HEIF tile grid and assemble it into *out. */
static int grid_decode(AVFormatContext *fmt, const AVStreamGroup *g, AVFrame **out, char *err,
                       size_t err_len) {
    const AVStreamGroupTileGrid *grid = g->params.tile_grid;
    const AVStream *first = g->streams[grid->offsets[0].idx];
    const AVCodec *codec = avcodec_find_decoder(first->codecpar->codec_id);
    if (!codec) {
        set_err(err, err_len, "this build has no decoder for %s",
                avcodec_get_name(first->codecpar->codec_id));
        return VB_AV_ERR_UNSUPPORTED;
    }

    /* Only the grid's own streams are read: a HEIF also carries a thumbnail,
     * and sometimes depth and alpha pictures, as streams of their own. */
    for (unsigned i = 0; i < fmt->nb_streams; i++) {
        fmt->streams[i]->discard = AVDISCARD_ALL;
    }
    for (unsigned t = 0; t < grid->nb_tiles; t++) {
        g->streams[grid->offsets[t].idx]->discard = AVDISCARD_DEFAULT;
    }

    int rc = VB_AV_ERR_DECODE;
    int ret;
    unsigned placed = 0;
    AVFrame *canvas = NULL;
    AVCodecContext *dec = avcodec_alloc_context3(codec);
    AVPacket *pkt = av_packet_alloc();
    AVFrame *tile = av_frame_alloc();
    if (!dec || !pkt || !tile) {
        set_err(err, err_len, "out of memory decoding the picture");
        goto done;
    }
    ret = avcodec_parameters_to_context(dec, first->codecpar);
    if (ret >= 0) {
        dec->thread_count = 0;
        ret = avcodec_open2(dec, codec, NULL);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the picture's decoder", ret);
        goto done;
    }

    while (av_read_frame(fmt, pkt) >= 0) {
        int stream = pkt->stream_index;
        int wanted = 0;
        for (unsigned t = 0; t < grid->nb_tiles && !wanted; t++) {
            wanted = (int)g->streams[grid->offsets[t].idx]->index == stream;
        }
        if (!wanted) {
            av_packet_unref(pkt);
            continue;
        }
        /* Each tile is one intra picture: sent, drained, and the decoder
         * reset for the next, so none is held back for reordering. */
        ret = avcodec_send_packet(dec, pkt);
        av_packet_unref(pkt);
        if (ret >= 0) {
            ret = avcodec_send_packet(dec, NULL);
        }
        if (ret >= 0) {
            ret = avcodec_receive_frame(dec, tile);
        }
        avcodec_flush_buffers(dec);
        if (ret < 0) {
            set_av_err(err, err_len, "could not decode a tile of the picture", ret);
            goto done;
        }
        if (!canvas) {
            canvas = av_frame_alloc();
            if (!canvas) {
                set_err(err, err_len, "out of memory assembling the picture");
                goto done;
            }
            canvas->format = tile->format;
            canvas->width = grid->coded_width;
            canvas->height = grid->coded_height;
            ret = av_frame_get_buffer(canvas, 0);
            if (ret >= 0) {
                ret = av_frame_copy_props(canvas, tile);
            }
            if (ret < 0) {
                set_av_err(err, err_len, "could not allocate the picture", ret);
                goto done;
            }
        } else if (tile->format != canvas->format) {
            set_err(err, err_len, "the picture's tiles differ in format");
            goto done;
        }
        /* A tile stream may appear at more than one place in the grid. */
        for (unsigned t = 0; t < grid->nb_tiles; t++) {
            if ((int)g->streams[grid->offsets[t].idx]->index != stream) {
                continue;
            }
            if (paste_tile(tile, canvas, grid->offsets[t].horizontal,
                           grid->offsets[t].vertical) < 0) {
                set_err(err, err_len, "cannot assemble %s tiles",
                        av_get_pix_fmt_name((enum AVPixelFormat)tile->format));
                goto done;
            }
            placed++;
        }
        av_frame_unref(tile);
    }
    if (!canvas || placed < grid->nb_tiles) {
        set_err(err, err_len, "the picture has %u of its %u tiles", placed, grid->nb_tiles);
        goto done;
    }

    /* The grid is coded in whole tiles; the picture is the part of it the
     * group names. */
    canvas->crop_left = (size_t)FFMAX(grid->horizontal_offset, 0);
    canvas->crop_top = (size_t)FFMAX(grid->vertical_offset, 0);
    canvas->crop_right =
        (size_t)FFMAX(grid->coded_width - grid->width - grid->horizontal_offset, 0);
    canvas->crop_bottom =
        (size_t)FFMAX(grid->coded_height - grid->height - grid->vertical_offset, 0);
    ret = av_frame_apply_cropping(canvas, AV_FRAME_CROP_UNALIGNED);
    if (ret < 0) {
        set_av_err(err, err_len, "could not crop the picture", ret);
        goto done;
    }
    rc = VB_AV_OK;

done:
    av_frame_free(&tile);
    av_packet_free(&pkt);
    avcodec_free_context(&dec);
    if (rc != VB_AV_OK) {
        av_frame_free(&canvas);
        return rc;
    }
    *out = canvas;
    return VB_AV_OK;
}

/* A photograph's one picture, as stored (not turned), into *out. */
static int picture_decode(const char *path, AVFrame **out, char *err, size_t err_len) {
    *out = NULL;
    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    const AVStreamGroup *g = tile_grid(fmt);
    if (g) {
        rc = grid_decode(fmt, g, out, err, err_len);
        avformat_close_input(&fmt);
        return rc;
    }
    avformat_close_input(&fmt);

    /* Anything else is a one-frame video stream: its first frame. */
    Video v;
    rc = video_open(path, &v, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    rc = video_next(&v, err, err_len);
    if (rc == 0) {
        set_err(err, err_len, "the file holds no picture");
        rc = VB_AV_ERR_DECODE;
    } else if (rc > 0) {
        rc = video_pixels(&v, err, err_len);
        if (rc == VB_AV_OK) {
            *out = av_frame_clone(v.frame);
            if (!*out) {
                set_err(err, err_len, "out of memory decoding the picture");
                rc = VB_AV_ERR_DECODE;
            }
        }
    }
    video_close(&v);
    return rc;
}

/* The picture turned upright by `rotation`, as a frame the caller frees:
 * `frame` itself when there is nothing to turn. */
static int picture_upright(AVFrame *frame, int rotation, AVFrame **out, char *err,
                           size_t err_len) {
    if (rotation != 90 && rotation != 180 && rotation != 270) {
        *out = frame;
        return VB_AV_OK;
    }
    *out = turned(frame, rotation);
    if (!*out) {
        set_err(err, err_len, "cannot turn %s pictures upright",
                av_get_pix_fmt_name((enum AVPixelFormat)frame->format));
        return VB_AV_ERR_UNSUPPORTED;
    }
    av_frame_free(&frame);
    return VB_AV_OK;
}

int vb_av_picture_info(const char *path, uint32_t *width, uint32_t *height, char *codec,
                       size_t codec_len, char *err, size_t err_len) {
    if (!path || !width || !height || !codec || codec_len == 0) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *width = *height = 0;
    codec[0] = '\0';

    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    const AVStreamGroup *g = tile_grid(fmt);
    if (g) {
        const AVStreamGroupTileGrid *grid = g->params.tile_grid;
        *width = (uint32_t)FFMAX(grid->width, 0);
        *height = (uint32_t)FFMAX(grid->height, 0);
        av_strlcpy(codec, avcodec_get_name(g->streams[grid->offsets[0].idx]->codecpar->codec_id),
                   codec_len);
    } else {
        int vi = find_stream(fmt, AVMEDIA_TYPE_VIDEO);
        if (vi < 0) {
            avformat_close_input(&fmt);
            set_err(err, err_len, "the file holds no picture");
            return VB_AV_ERR_NO_VIDEO;
        }
        const AVCodecParameters *par = fmt->streams[vi]->codecpar;
        *width = par->width > 0 ? (uint32_t)par->width : 0;
        *height = par->height > 0 ? (uint32_t)par->height : 0;
        av_strlcpy(codec, avcodec_get_name(par->codec_id), codec_len);
    }
    avformat_close_input(&fmt);
    return VB_AV_OK;
}

int vb_av_picture(const char *path, int32_t rotation, uint32_t width, uint32_t height,
                  int32_t gray, uint8_t *out, char *err, size_t err_len) {
    if (!path || !out || width == 0 || height == 0 || width > 16384 || height > 16384) {
        set_err(err, err_len, "bad argument: a %ux%u picture", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();

    AVFrame *frame = NULL;
    int rc = picture_decode(path, &frame, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    AVFrame *upright = NULL;
    rc = picture_upright(frame, rotation, &upright, err, err_len);
    if (rc != VB_AV_OK) {
        av_frame_free(&frame);
        return rc;
    }
    uint8_t *planes[4] = {out, NULL, NULL, NULL};
    int strides[4] = {gray ? (int)width : (int)width * 3, 0, 0, 0};
    rc = convert(upright, gray ? AV_PIX_FMT_GRAY8 : AV_PIX_FMT_RGB24, (int)width, (int)height, 1,
                 planes, strides, err, err_len);
    av_frame_free(&upright);
    return rc;
}

int vb_av_picture_still(const char *path, int32_t rotation, uint32_t width, uint32_t height,
                        const char *dest, char *err, size_t err_len) {
    if (!path || !dest || width == 0 || height == 0 || width > 16384 || height > 16384) {
        set_err(err, err_len, "bad argument: a %ux%u still", width, height);
        return VB_AV_ERR_ARG;
    }
    begin_call();

    enum AVCodecID codec_id;
    enum AVPixelFormat pix_fmt;
    int rc = still_format(dest, &codec_id, &pix_fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    AVFrame *frame = NULL;
    rc = picture_decode(path, &frame, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    AVFrame *upright = NULL;
    rc = picture_upright(frame, rotation, &upright, err, err_len);
    if (rc != VB_AV_OK) {
        av_frame_free(&frame);
        return rc;
    }
    rc = encode_still(upright, dest, codec_id, pix_fmt, (int)width, (int)height, err, err_len);
    av_frame_free(&upright);
    return rc;
}

/* ------------------------------------------------------------------------ */
/* Audio                                                                     */
/* ------------------------------------------------------------------------ */

/*
 * Mono float samples at a rate the caller chose.
 *
 * Everything upstream that listens -- beat detection, and speech recognition
 * when a model is installed -- wants one channel at a low rate, so the
 * downmix and the resample happen here rather than in three callers. The
 * conversion is swresample's, which is the same one the CLI backend's
 * `-ac 1 -ar N` performs, so the two backends hear the same thing.
 */
int vb_av_decode_audio(const char *path, uint32_t sample_rate, int64_t max_us, float **out,
                       size_t *count, int64_t *start_us, int64_t *duration_us, char *err,
                       size_t err_len) {
    if (!path || !out || !count) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    if (sample_rate == 0) {
        set_err(err, err_len, "a sample rate of zero decodes nothing");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *out = NULL;
    *count = 0;
    if (start_us) {
        *start_us = 0;
    }
    if (duration_us) {
        *duration_us = 0;
    }

    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }

    int ai = find_stream(fmt, AVMEDIA_TYPE_AUDIO);
    if (ai < 0) {
        avformat_close_input(&fmt);
        set_err(err, err_len, "the file has no audio stream");
        return VB_AV_ERR_NO_AUDIO;
    }
    AVStream *st = fmt->streams[ai];
    if (duration_us) {
        if (fmt->duration != AV_NOPTS_VALUE && fmt->duration > 0) {
            *duration_us = fmt->duration;
        } else if (st->duration != AV_NOPTS_VALUE && st->duration > 0) {
            *duration_us = av_rescale_q(st->duration, st->time_base, AV_TIME_BASE_Q);
        }
    }
    for (unsigned i = 0; i < fmt->nb_streams; i++) {
        if ((int)i != ai) {
            fmt->streams[i]->discard = AVDISCARD_ALL;
        }
    }

    AVCodecContext *dec = NULL;
    AVPacket *pkt = NULL;
    AVFrame *frame = NULL;
    SwrContext *swr = NULL;
    AVChannelLayout mono = AV_CHANNEL_LAYOUT_MONO;
    float *buffer = NULL;
    size_t filled = 0;
    size_t capacity = 0;
    int64_t origin_us = fmt->start_time == AV_NOPTS_VALUE ? 0 : fmt->start_time;
    int64_t first_us = AV_NOPTS_VALUE;
    /* Zero means the whole file. */
    size_t wanted = max_us > 0 ? (size_t)av_rescale(max_us, sample_rate, 1000000) : 0;
    int ret;

    const AVCodec *codec = avcodec_find_decoder(st->codecpar->codec_id);
    if (!codec) {
        set_err(err, err_len, "this build has no decoder for %s",
                avcodec_get_name(st->codecpar->codec_id));
        rc = VB_AV_ERR_UNSUPPORTED;
        goto done;
    }

    dec = avcodec_alloc_context3(codec);
    pkt = av_packet_alloc();
    frame = av_frame_alloc();
    if (!dec || !pkt || !frame) {
        set_err(err, err_len, "out of memory opening the audio decoder");
        rc = VB_AV_ERR_DECODE;
        goto done;
    }
    ret = avcodec_parameters_to_context(dec, st->codecpar);
    if (ret < 0) {
        set_av_err(err, err_len, "could not configure the audio decoder", ret);
        rc = VB_AV_ERR_DECODE;
        goto done;
    }
    dec->pkt_timebase = st->time_base;
    ret = avcodec_open2(dec, codec, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the audio decoder", ret);
        rc = VB_AV_ERR_DECODE;
        goto done;
    }

    /* A stream that declares no layout still has a channel count, and
     * guessing the usual layout for that count is what every ffmpeg tool
     * does rather than refusing the file. */
    if (dec->ch_layout.nb_channels <= 0) {
        set_err(err, err_len, "the audio stream declares no channels");
        rc = VB_AV_ERR_DECODE;
        goto done;
    }
    if (dec->ch_layout.order == AV_CHANNEL_ORDER_UNSPEC) {
        av_channel_layout_default(&dec->ch_layout, dec->ch_layout.nb_channels);
    }

    ret = swr_alloc_set_opts2(&swr, &mono, AV_SAMPLE_FMT_FLT, (int)sample_rate, &dec->ch_layout,
                              dec->sample_fmt, dec->sample_rate, 0, NULL);
    if (ret >= 0) {
        ret = swr_init(swr);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not set up the audio conversion", ret);
        rc = VB_AV_ERR_DECODE;
        goto done;
    }

    for (;;) {
        ret = av_read_frame(fmt, pkt);
        if (ret < 0) {
            avcodec_send_packet(dec, NULL); /* drain */
        } else if (pkt->stream_index == ai) {
            (void)avcodec_send_packet(dec, pkt);
        }
        int at_end = ret < 0;
        av_packet_unref(pkt);

        for (;;) {
            ret = avcodec_receive_frame(dec, frame);
            if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
                break;
            }
            if (ret < 0) {
                set_av_err(err, err_len, "decoding the audio failed", ret);
                rc = VB_AV_ERR_DECODE;
                goto done;
            }
            if (first_us == AV_NOPTS_VALUE) {
                int64_t ts = frame->best_effort_timestamp;
                if (ts == AV_NOPTS_VALUE) {
                    ts = frame->pts;
                }
                first_us = ts == AV_NOPTS_VALUE
                               ? 0
                               : av_rescale_q(ts, st->time_base, AV_TIME_BASE_Q) - origin_us;
                if (first_us < 0) {
                    first_us = 0;
                }
            }

            /* Room for what swresample holds back plus this frame, which is
             * what its own documentation prescribes. */
            int64_t delay = swr_get_delay(swr, dec->sample_rate);
            int room = (int)av_rescale_rnd(delay + frame->nb_samples, sample_rate,
                                           dec->sample_rate, AV_ROUND_UP);
            if (filled + (size_t)room > capacity) {
                size_t grown = (capacity ? capacity * 2 : 1 << 16);
                while (grown < filled + (size_t)room) {
                    grown *= 2;
                }
                float *bigger = av_realloc(buffer, grown * sizeof(float));
                if (!bigger) {
                    set_err(err, err_len, "out of memory decoding the audio");
                    rc = VB_AV_ERR_DECODE;
                    goto done;
                }
                buffer = bigger;
                capacity = grown;
            }

            uint8_t *dst = (uint8_t *)(buffer + filled);
            int got = swr_convert(swr, &dst, room, (const uint8_t **)frame->data,
                                  frame->nb_samples);
            av_frame_unref(frame);
            if (got < 0) {
                set_av_err(err, err_len, "converting the audio failed", got);
                rc = VB_AV_ERR_DECODE;
                goto done;
            }
            filled += (size_t)got;
            if (wanted && filled >= wanted) {
                filled = wanted;
                goto flushed;
            }
        }
        if (at_end) {
            break;
        }
    }

    /* Whatever swresample is still holding back. */
    if (capacity < filled + 4096) {
        float *bigger = av_realloc(buffer, (filled + 4096) * sizeof(float));
        if (bigger) {
            buffer = bigger;
            capacity = filled + 4096;
        }
    }
    if (capacity > filled) {
        uint8_t *tail = (uint8_t *)(buffer + filled);
        int rest = swr_convert(swr, &tail, (int)(capacity - filled), NULL, 0);
        if (rest > 0) {
            filled += (size_t)rest;
        }
    }

flushed:
    if (filled == 0) {
        set_err(err, err_len, "the audio stream decoded to nothing");
        rc = VB_AV_ERR_DECODE;
        goto done;
    }
    *out = buffer;
    *count = filled;
    if (start_us) {
        *start_us = first_us == AV_NOPTS_VALUE ? 0 : first_us;
    }
    buffer = NULL;
    rc = VB_AV_OK;

done:
    av_freep(&buffer);
    swr_free(&swr);
    av_frame_free(&frame);
    av_packet_free(&pkt);
    avcodec_free_context(&dec);
    avformat_close_input(&fmt);
    return rc;
}

void vb_av_free_samples(float *samples) {
    av_free(samples);
}

/* ------------------------------------------------------------------------ */
/* The renderer: frames and sound in order, and a finished piece written     */
/* ------------------------------------------------------------------------ */

/*
 * The built-in editor's export composes every frame itself and needs three
 * things of a backend: a clip's frames in order from a moment, a file's
 * sound in order at a speed, and somewhere to write the piece. The CLI
 * backend does each through a process with a pipe; these do the same in
 * process, which is what a phone needs, under the same rules:
 *
 * - Frames start at the first one at or after the moment, as an exact seek
 *   finds it, turned upright and then scaled, as rgb_frame's are.
 * - Sound is trimmed to the stretch sample by sample, then run through the
 *   same filter description the CLI is given (atempo, from the Rust side's
 *   `tempo_filter`), then converted to the rate and channels asked for by
 *   libswresample, which is what the CLI's own conversion is.
 * - The piece is H.264 or HEVC from the platform's encoder and AAC from
 *   libav's own, in an MP4 with its index at the front, the picture
 *   converted from RGB to BT.709 limited range and labelled so -- the
 *   CLI's `scale=out_color_matrix=bt709:out_range=tv,format=yuv420p`.
 */

struct vb_av_reader {
    Video v;
    /* v.path points here: the reader outlives the caller's string, and the
     * decoder may need the file again to carry on in software. */
    char *path;
    int64_t from_us;
    uint32_t width;
    uint32_t height;
    int rotation;
    struct SwsContext *sws;
    int sws_format, sws_width, sws_height, sws_space, sws_range;
    /* Each frame in RGB24 before it is given its alpha (reader_convert). */
    uint8_t *rgb;
    /* The last frame before the moment, in case none comes after it. */
    AVFrame *before;
    int have_before;
    int started;
    int done;
};

void vb_av_reader_close(vb_av_reader *r) {
    if (!r) {
        return;
    }
    av_frame_free(&r->before);
    sws_freeContext(r->sws);
    av_free(r->rgb);
    if (r->v.fmt || r->v.dec) {
        video_close(&r->v);
    }
    av_free(r->path);
    av_free(r);
}

int vb_av_reader_open(const char *path, int64_t from_us, uint32_t width, uint32_t height,
                      vb_av_reader **out, char *err, size_t err_len) {
    if (!path || !out || width == 0 || height == 0 || width > 16384 || height > 16384 ||
        from_us < 0) {
        set_err(err, err_len, "bad argument: a %ux%u reader from %lld us", width, height,
                (long long)from_us);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *out = NULL;
    vb_av_reader *r = av_mallocz(sizeof *r);
    if (!r) {
        set_err(err, err_len, "out of memory opening a reader");
        return VB_AV_ERR_DECODE;
    }
    r->path = av_strdup(path);
    r->before = av_frame_alloc();
    r->rgb = av_malloc((size_t)width * height * 3);
    if (!r->path || !r->before || !r->rgb) {
        vb_av_reader_close(r);
        set_err(err, err_len, "out of memory opening a reader");
        return VB_AV_ERR_DECODE;
    }
    int rc = video_open(r->path, &r->v, err, err_len);
    if (rc != VB_AV_OK) {
        vb_av_reader_close(r);
        return rc;
    }
    r->from_us = from_us;
    r->width = width;
    r->height = height;
    r->rotation = stream_rotation(r->v.st);
    r->sws_format = -1;
    if (from_us > 0) {
        /* Rounded down, as decode_at's is, so the frame exactly at the
         * moment is never skipped by a rounding error. */
        int64_t target = av_rescale_q_rnd(from_us + r->v.origin_us, AV_TIME_BASE_Q,
                                          r->v.st->time_base, AV_ROUND_DOWN);
        video_seek(&r->v, target);
    }
    *out = r;
    return VB_AV_OK;
}

/* `frame` upright and scaled into `rgba`, through a scaler kept while the
 * frames keep their format.
 *
 * Scaled to RGB24, as every one-frame read is, and given its alpha after:
 * on x86 swscale's RGBA came out up to two levels from its RGB24 (0.64 on
 * average, the fixture's frames), which put the editor's stopped preview,
 * drawn from rgb_frame, off the frames its export reads here. */
static int reader_convert(vb_av_reader *r, const AVFrame *frame, uint8_t *rgba, char *err,
                          size_t err_len) {
    AVFrame *upright = r->rotation ? turned(frame, r->rotation) : NULL;
    if (r->rotation && !upright) {
        set_err(err, err_len, "cannot turn %s frames upright",
                av_get_pix_fmt_name((enum AVPixelFormat)frame->format));
        return VB_AV_ERR_UNSUPPORTED;
    }
    const AVFrame *src = upright ? upright : frame;
    if (!r->sws || r->sws_format != src->format || r->sws_width != src->width ||
        r->sws_height != src->height || r->sws_space != (int)src->colorspace ||
        r->sws_range != (int)src->color_range) {
        sws_freeContext(r->sws);
        r->sws = scaler(src, AV_PIX_FMT_RGB24, (int)r->width, (int)r->height, 1);
        if (!r->sws) {
            av_frame_free(&upright);
            const char *from = av_get_pix_fmt_name((enum AVPixelFormat)src->format);
            set_err(err, err_len, "cannot convert %s frames to rgb24", from ? from : "unknown");
            return VB_AV_ERR_DECODE;
        }
        source_colours(r->sws, src, 1);
        r->sws_format = src->format;
        r->sws_width = src->width;
        r->sws_height = src->height;
        r->sws_space = (int)src->colorspace;
        r->sws_range = (int)src->color_range;
    }
    uint8_t *planes[4] = {r->rgb, NULL, NULL, NULL};
    int strides[4] = {(int)r->width * 3, 0, 0, 0};
    int rows = sws_scale(r->sws, (const uint8_t *const *)src->data, src->linesize, 0,
                         src->height, planes, strides);
    av_frame_free(&upright);
    if (rows <= 0) {
        set_err(err, err_len, "scaling the frame produced no output");
        return VB_AV_ERR_DECODE;
    }
    const size_t pixels = (size_t)r->width * r->height;
    for (size_t i = 0; i < pixels; i++) {
        rgba[4 * i] = r->rgb[3 * i];
        rgba[4 * i + 1] = r->rgb[3 * i + 1];
        rgba[4 * i + 2] = r->rgb[3 * i + 2];
        rgba[4 * i + 3] = 255;
    }
    return VB_AV_OK;
}

int vb_av_reader_next(vb_av_reader *r, uint8_t *rgba, int64_t *at_us, char *err,
                      size_t err_len) {
    if (!r || !rgba || !at_us) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    if (r->done) {
        return 0;
    }
    for (;;) {
        int rc = video_next(&r->v, err, err_len);
        if (rc < 0) {
            r->done = 1;
            return rc;
        }
        if (rc == 0) {
            r->done = 1;
            /* Nothing at or after the moment, but a frame before it: that
             * one is on screen there, as rgb_frame finds it. */
            if (r->started || !r->have_before) {
                return 0;
            }
            av_frame_unref(r->v.frame);
            av_frame_move_ref(r->v.frame, r->before);
            r->have_before = 0;
        } else {
            int64_t t = frame_us(&r->v, r->v.frame);
            if (t == AV_NOPTS_VALUE) {
                continue;
            }
            if (!r->started && t < r->from_us) {
                /* Decoded to get there and kept by reference only: its
                 * pixels are fetched if it turns out to be needed. */
                av_frame_unref(r->before);
                r->have_before = av_frame_ref(r->before, r->v.frame) >= 0;
                continue;
            }
        }
        r->started = 1;
        av_frame_unref(r->before);
        r->have_before = 0;
        int64_t t = frame_us(&r->v, r->v.frame);
        rc = video_pixels(&r->v, err, err_len);
        if (rc == VB_AV_OK) {
            rc = reader_convert(r, r->v.frame, rgba, err, err_len);
        }
        if (rc != VB_AV_OK) {
            r->done = 1;
            return rc;
        }
        *at_us = (t == AV_NOPTS_VALUE || t < 0) ? 0 : t;
        return 1;
    }
}

struct vb_av_sound {
    AVFormatContext *fmt;
    AVCodecContext *dec;
    AVStream *st;
    int index;
    AVPacket *pkt;
    AVFrame *frame;
    AVFrame *out;
    int out_pos;
    AVFilterGraph *graph;
    AVFilterContext *src;
    AVFilterContext *sink;
    int64_t origin_us;
    int64_t from_us;
    int64_t to_us;
    uint32_t channels;
    /* Samples given to the graph so far: the next one's timestamp. */
    int64_t fed;
    /* Where the next decoded frame starts, in samples, should it have no
     * time of its own. */
    int64_t next_sample;
    int draining;
    int input_done;
    int eof;
};

void vb_av_sound_close(vb_av_sound *s) {
    if (!s) {
        return;
    }
    avfilter_graph_free(&s->graph);
    av_frame_free(&s->frame);
    av_frame_free(&s->out);
    av_packet_free(&s->pkt);
    avcodec_free_context(&s->dec);
    avformat_close_input(&s->fmt);
    av_free(s);
}

/* abuffer, then the caller's filters, then the format asked for. */
static int sound_graph(vb_av_sound *s, const char *filters, uint32_t sample_rate,
                       uint32_t channels, char *err, size_t err_len) {
    char layout[128];
    if (av_channel_layout_describe(&s->dec->ch_layout, layout, sizeof layout) < 0) {
        set_err(err, err_len, "the audio stream's channels cannot be described");
        return VB_AV_ERR_DECODE;
    }
    AVChannelLayout want;
    av_channel_layout_default(&want, (int)channels);
    char want_layout[128];
    int described = av_channel_layout_describe(&want, want_layout, sizeof want_layout);
    av_channel_layout_uninit(&want);
    if (described < 0) {
        set_err(err, err_len, "%u channels cannot be described", channels);
        return VB_AV_ERR_ARG;
    }

    char args[512];
    snprintf(args, sizeof args, "time_base=1/%d:sample_rate=%d:sample_fmt=%s:channel_layout=%s",
             s->dec->sample_rate, s->dec->sample_rate, av_get_sample_fmt_name(s->dec->sample_fmt),
             layout);
    char desc[1024];
    snprintf(desc, sizeof desc, "%s%saformat=sample_fmts=flt:sample_rates=%u:channel_layouts=%s",
             filters && filters[0] ? filters : "", filters && filters[0] ? "," : "", sample_rate,
             want_layout);

    s->graph = avfilter_graph_alloc();
    const AVFilter *abuffer = avfilter_get_by_name("abuffer");
    const AVFilter *abuffersink = avfilter_get_by_name("abuffersink");
    if (!s->graph || !abuffer || !abuffersink) {
        set_err(err, err_len, "this build cannot filter sound");
        return VB_AV_ERR_UNSUPPORTED;
    }
    /* One thread: the graph is tiny and runs on the renderer's own. */
    s->graph->nb_threads = 1;
    int ret = avfilter_graph_create_filter(&s->src, abuffer, "in", args, NULL, s->graph);
    if (ret >= 0) {
        ret = avfilter_graph_create_filter(&s->sink, abuffersink, "out", NULL, NULL, s->graph);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not set up the sound's filters", ret);
        return VB_AV_ERR_DECODE;
    }

    AVFilterInOut *outputs = avfilter_inout_alloc();
    AVFilterInOut *inputs = avfilter_inout_alloc();
    if (!outputs || !inputs) {
        avfilter_inout_free(&outputs);
        avfilter_inout_free(&inputs);
        set_err(err, err_len, "out of memory setting up the sound's filters");
        return VB_AV_ERR_DECODE;
    }
    outputs->name = av_strdup("in");
    outputs->filter_ctx = s->src;
    outputs->pad_idx = 0;
    outputs->next = NULL;
    inputs->name = av_strdup("out");
    inputs->filter_ctx = s->sink;
    inputs->pad_idx = 0;
    inputs->next = NULL;
    ret = avfilter_graph_parse_ptr(s->graph, desc, &inputs, &outputs, NULL);
    avfilter_inout_free(&outputs);
    avfilter_inout_free(&inputs);
    if (ret >= 0) {
        ret = avfilter_graph_config(s->graph, NULL);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not set up the sound's filters", ret);
        return ret == AVERROR_FILTER_NOT_FOUND ? VB_AV_ERR_UNSUPPORTED : VB_AV_ERR_DECODE;
    }
    return VB_AV_OK;
}

int vb_av_sound_open(const char *path, int64_t from_us, int64_t to_us, uint32_t sample_rate,
                     uint32_t channels, const char *filters, vb_av_sound **out, char *err,
                     size_t err_len) {
    if (!path || !out || sample_rate == 0 || channels == 0 || channels > 8 || from_us < 0) {
        set_err(err, err_len, "bad argument: %u channels at %u Hz from %lld us", channels,
                sample_rate, (long long)from_us);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *out = NULL;
    vb_av_sound *s = av_mallocz(sizeof *s);
    if (!s) {
        set_err(err, err_len, "out of memory opening the sound");
        return VB_AV_ERR_DECODE;
    }
    s->from_us = from_us;
    s->to_us = to_us;
    s->channels = channels;

    int rc = open_input(path, &s->fmt, err, err_len);
    if (rc != VB_AV_OK) {
        vb_av_sound_close(s);
        return rc;
    }
    s->index = find_stream(s->fmt, AVMEDIA_TYPE_AUDIO);
    if (s->index < 0) {
        vb_av_sound_close(s);
        set_err(err, err_len, "the file has no audio stream");
        return VB_AV_ERR_NO_AUDIO;
    }
    s->st = s->fmt->streams[s->index];
    for (unsigned i = 0; i < s->fmt->nb_streams; i++) {
        if ((int)i != s->index) {
            s->fmt->streams[i]->discard = AVDISCARD_ALL;
        }
    }
    const AVCodec *codec = avcodec_find_decoder(s->st->codecpar->codec_id);
    if (!codec) {
        set_err(err, err_len, "this build has no decoder for %s",
                avcodec_get_name(s->st->codecpar->codec_id));
        vb_av_sound_close(s);
        return VB_AV_ERR_UNSUPPORTED;
    }
    s->dec = avcodec_alloc_context3(codec);
    s->pkt = av_packet_alloc();
    s->frame = av_frame_alloc();
    s->out = av_frame_alloc();
    if (!s->dec || !s->pkt || !s->frame || !s->out) {
        vb_av_sound_close(s);
        set_err(err, err_len, "out of memory opening the audio decoder");
        return VB_AV_ERR_DECODE;
    }
    int ret = avcodec_parameters_to_context(s->dec, s->st->codecpar);
    if (ret >= 0) {
        s->dec->pkt_timebase = s->st->time_base;
        ret = avcodec_open2(s->dec, codec, NULL);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the audio decoder", ret);
        vb_av_sound_close(s);
        return VB_AV_ERR_DECODE;
    }
    if (s->dec->ch_layout.nb_channels <= 0) {
        set_err(err, err_len, "the audio stream declares no channels");
        vb_av_sound_close(s);
        return VB_AV_ERR_DECODE;
    }
    if (s->dec->ch_layout.order == AV_CHANNEL_ORDER_UNSPEC) {
        av_channel_layout_default(&s->dec->ch_layout, s->dec->ch_layout.nb_channels);
    }
    rc = sound_graph(s, filters, sample_rate, channels, err, err_len);
    if (rc != VB_AV_OK) {
        vb_av_sound_close(s);
        return rc;
    }

    s->origin_us = s->fmt->start_time == AV_NOPTS_VALUE ? 0 : s->fmt->start_time;
    /* Decoding starts a quarter of a second early, and the trim below
     * drops what comes before the moment. Straight after a flush the AAC
     * decoder swallows its first packet, and an MP3's first frame lacks its
     * bit reservoir: a seek to the moment itself lost up to a frame there.
     * The CLI never meets this, since it seeks on the video's keyframes,
     * well before. */
    const int64_t lead_in = 250000;
    if (from_us > lead_in) {
        int64_t target = av_rescale_q_rnd(from_us - lead_in + s->origin_us, AV_TIME_BASE_Q,
                                          s->st->time_base, AV_ROUND_DOWN);
        if (av_seek_frame(s->fmt, s->index, target, AVSEEK_FLAG_BACKWARD) >= 0) {
            avcodec_flush_buffers(s->dec);
        }
    }
    *out = s;
    return VB_AV_OK;
}

/* No more sound goes into the graph. */
static int sound_end_input(vb_av_sound *s, char *err, size_t err_len) {
    if (s->input_done) {
        return VB_AV_OK;
    }
    s->input_done = 1;
    int ret = av_buffersrc_add_frame(s->src, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not finish the sound", ret);
        return VB_AV_ERR_DECODE;
    }
    return VB_AV_OK;
}

/* The decoded frame's samples that fall in the stretch, into the graph.
 * Returns 1 when some went in, 0 when none did, or an error. */
static int sound_take(vb_av_sound *s, char *err, size_t err_len) {
    AVFrame *f = s->frame;
    int rate = f->sample_rate > 0 ? f->sample_rate : s->dec->sample_rate;
    AVRational per_sample = {1, rate};
    int64_t ts = f->best_effort_timestamp;
    if (ts == AV_NOPTS_VALUE) {
        ts = f->pts;
    }
    /* Counted in samples at the stream's own rate, where a frame's start is
     * exact: in microseconds, a rounding there moved the trim by a sample.
     * A frame with no time follows the one before it. */
    int64_t start = ts == AV_NOPTS_VALUE
                        ? s->next_sample
                        : av_rescale_q(ts, s->st->time_base, per_sample) -
                              av_rescale_q(s->origin_us, AV_TIME_BASE_Q, per_sample);
    int n = f->nb_samples;
    s->next_sample = start + n;
    /* Sample k plays at k / rate: kept from the first at or after the
     * start of the stretch, up to the last before its end. */
    int64_t from = av_rescale_rnd(s->from_us, rate, 1000000, AV_ROUND_UP);
    int64_t first = from > start ? from - start : 0;
    int64_t last = n;
    if (s->to_us >= 0) {
        int64_t to = av_rescale_rnd(s->to_us, rate, 1000000, AV_ROUND_UP);
        if (start >= to) {
            av_frame_unref(f);
            int rc = sound_end_input(s, err, err_len);
            return rc == VB_AV_OK ? 0 : rc;
        }
        if (to - start < last) {
            last = to - start;
        }
    }
    if (first >= last) {
        av_frame_unref(f);
        return 0;
    }
    int count = (int)(last - first);
    int ret;
    if (first == 0 && count == n) {
        f->pts = s->fed;
        ret = av_buffersrc_add_frame(s->src, f);
    } else {
        AVFrame *part = av_frame_alloc();
        if (!part) {
            av_frame_unref(f);
            set_err(err, err_len, "out of memory trimming the sound");
            return VB_AV_ERR_DECODE;
        }
        part->format = f->format;
        part->sample_rate = rate;
        part->nb_samples = count;
        ret = av_channel_layout_copy(&part->ch_layout, &f->ch_layout);
        if (ret >= 0) {
            ret = av_frame_get_buffer(part, 0);
        }
        if (ret >= 0) {
            ret = av_samples_copy(part->extended_data, f->extended_data, 0, (int)first, count,
                                  f->ch_layout.nb_channels, (enum AVSampleFormat)f->format);
        }
        if (ret >= 0) {
            part->pts = s->fed;
            ret = av_buffersrc_add_frame(s->src, part);
        }
        av_frame_free(&part);
    }
    av_frame_unref(f);
    if (ret < 0) {
        set_av_err(err, err_len, "could not filter the sound", ret);
        return VB_AV_ERR_DECODE;
    }
    s->fed += count;
    if (s->to_us >= 0 && last < n) {
        int rc = sound_end_input(s, err, err_len);
        if (rc != VB_AV_OK) {
            return rc;
        }
    }
    return 1;
}

/* Decode until some sound has gone into the graph, or there is no more. */
static int sound_feed(vb_av_sound *s, char *err, size_t err_len) {
    while (!s->input_done) {
        int ret = avcodec_receive_frame(s->dec, s->frame);
        if (ret >= 0) {
            int rc = sound_take(s, err, err_len);
            if (rc != 0) {
                return rc < 0 ? rc : VB_AV_OK;
            }
            continue;
        }
        if (ret == AVERROR_EOF) {
            return sound_end_input(s, err, err_len);
        }
        if (ret != AVERROR(EAGAIN)) {
            set_av_err(err, err_len, "decoding the audio failed", ret);
            return VB_AV_ERR_DECODE;
        }
        if (s->draining) {
            return sound_end_input(s, err, err_len);
        }
        ret = av_read_frame(s->fmt, s->pkt);
        if (ret < 0) {
            s->draining = 1;
            avcodec_send_packet(s->dec, NULL);
            continue;
        }
        if (s->pkt->stream_index == s->index) {
            /* A corrupt packet costs its few milliseconds of sound, as it
             * does in the CLI. */
            (void)avcodec_send_packet(s->dec, s->pkt);
        }
        av_packet_unref(s->pkt);
    }
    return VB_AV_OK;
}

int vb_av_sound_read(vb_av_sound *s, float *out, size_t max_frames, size_t *got, char *err,
                     size_t err_len) {
    if (!s || !out || !got) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *got = 0;
    while (*got < max_frames) {
        if (s->out->nb_samples > 0 && s->out_pos < s->out->nb_samples) {
            size_t want = max_frames - *got;
            size_t have = (size_t)(s->out->nb_samples - s->out_pos);
            size_t n = want < have ? want : have;
            const float *from = (const float *)s->out->data[0] + (size_t)s->out_pos * s->channels;
            memcpy(out + *got * s->channels, from, n * s->channels * sizeof(float));
            s->out_pos += (int)n;
            *got += n;
            continue;
        }
        av_frame_unref(s->out);
        s->out_pos = 0;
        if (s->eof) {
            break;
        }
        int ret = av_buffersink_get_frame(s->sink, s->out);
        if (ret >= 0) {
            continue;
        }
        if (ret == AVERROR_EOF) {
            s->eof = 1;
            break;
        }
        if (ret != AVERROR(EAGAIN)) {
            set_av_err(err, err_len, "filtering the sound failed", ret);
            return VB_AV_ERR_DECODE;
        }
        if (s->input_done) {
            /* Everything is in and the graph wants more: it has nothing. */
            s->eof = 1;
            break;
        }
        int rc = sound_feed(s, err, err_len);
        if (rc != VB_AV_OK) {
            return rc;
        }
    }
    return VB_AV_OK;
}

struct vb_av_writer {
    AVFormatContext *oc;
    AVCodecContext *venc;
    AVCodecContext *aenc;
    AVStream *vst;
    AVStream *ast;
    struct SwsContext *sws;
    AVFrame *vframe;
    AVFrame *aframe;
    AVAudioFifo *fifo;
    float **planes;
    size_t planes_room;
    AVPacket *pkt;
    int64_t vpts;
    int64_t apts;
    uint32_t width;
    uint32_t height;
    uint32_t channels;
    int frame_size;
    int header_written;
};

/* The platform's encoder for H.264 or HEVC: the Mac's and iPhone's media
 * engine, Android's MediaCodec, Windows' Media Foundation. libav's own
 * encoders for these are GPL (x264, x265) and never in a build that ships;
 * OpenH264 is the BSD fallback where a build carries it. */
static const AVCodec *video_encoder(int hevc) {
    static const char *const h264[] = {"h264_videotoolbox", "h264_mediacodec", "h264_mf",
                                       "libopenh264", NULL};
    static const char *const h265[] = {"hevc_videotoolbox", "hevc_mediacodec", "hevc_mf", NULL};
    for (const char *const *name = hevc ? h265 : h264; *name; name++) {
        const AVCodec *codec = avcodec_find_encoder_by_name(*name);
        if (codec) {
            return codec;
        }
    }
    return NULL;
}

/* Media Foundation's encoders: the GPU's, which take NV12 alone, and
 * Microsoft's software H.264, which takes NV12 or planar 4:2:0. */
static int is_mf(const AVCodec *codec) {
    return codec->wrapper_name && strcmp(codec->wrapper_name, "mf") == 0;
}

/* The picture format the encoder takes that a converted RGB frame goes
 * into best: 4:2:0, planar where it can be -- NV12 for Media Foundation,
 * whose GPU encoders refuse anything else. */
static enum AVPixelFormat encoder_pix_fmt(const AVCodec *codec, AVCodecContext *ctx) {
    const enum AVPixelFormat *fmts = NULL;
#if LIBAVCODEC_VERSION_INT >= AV_VERSION_INT(61, 13, 100)
    int count = 0;
    if (avcodec_get_supported_config(ctx, codec, AV_CODEC_CONFIG_PIX_FORMAT, 0,
                                     (const void **)&fmts, &count) < 0) {
        fmts = NULL;
    }
#else
    (void)ctx;
    fmts = codec->pix_fmts;
#endif
    if (!fmts) {
        return AV_PIX_FMT_YUV420P;
    }
    const enum AVPixelFormat first = is_mf(codec) ? AV_PIX_FMT_NV12 : AV_PIX_FMT_YUV420P;
    const enum AVPixelFormat second = is_mf(codec) ? AV_PIX_FMT_YUV420P : AV_PIX_FMT_NV12;
    for (const enum AVPixelFormat *p = fmts; *p != AV_PIX_FMT_NONE; p++) {
        if (*p == first) {
            return *p;
        }
    }
    for (const enum AVPixelFormat *p = fmts; *p != AV_PIX_FMT_NONE; p++) {
        if (*p == second) {
            return *p;
        }
    }
    return AV_PIX_FMT_YUV420P;
}

#ifndef AV_PROFILE_H264_HIGH
#define AV_PROFILE_H264_HIGH FF_PROFILE_H264_HIGH
#endif

/* Set up the picture's encoder in `w->venc` and open it. `hardware` says
 * whether a Media Foundation encoder may be the GPU's; the others ignore
 * it. Returns libav's code. */
static int open_video_encoder(vb_av_writer *w, const AVCodec *vcodec,
                              const vb_av_encode_settings *settings, int global_header,
                              int hardware) {
    w->venc = avcodec_alloc_context3(vcodec);
    if (!w->venc) {
        return AVERROR(ENOMEM);
    }
    w->venc->width = (int)settings->width;
    w->venc->height = (int)settings->height;
    w->venc->time_base = (AVRational){(int)settings->fps_den, (int)settings->fps_num};
    w->venc->framerate = (AVRational){(int)settings->fps_num, (int)settings->fps_den};
    w->venc->pix_fmt = encoder_pix_fmt(vcodec, w->venc);
    w->venc->bit_rate = (int64_t)settings->video_bitrate;
    w->venc->colorspace = AVCOL_SPC_BT709;
    w->venc->color_primaries = AVCOL_PRI_BT709;
    w->venc->color_trc = AVCOL_TRC_BT709;
    w->venc->color_range = AVCOL_RANGE_MPEG;
    /* A keyframe every two seconds or so: a phone scrubs the piece well. */
    w->venc->gop_size = (int)((settings->fps_num * 2 + settings->fps_den - 1) / settings->fps_den);
    if (global_header) {
        w->venc->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    }
    if (is_mf(vcodec) && !settings->hevc) {
        /* Left to itself Media Foundation writes Constrained Baseline,
         * hardware and software alike. */
        w->venc->profile = AV_PROFILE_H264_HIGH;
    }
    AVDictionary *vopts = NULL;
    /* The media engine where there is one, the system's own software
     * encoder where there is not -- a virtual Mac, say. */
    av_dict_set(&vopts, "allow_sw", "1", 0);
    if (is_mf(vcodec)) {
        av_dict_set(&vopts, "hw_encoding", hardware ? "1" : "0", 0);
    }
    int ret = avcodec_open2(w->venc, vcodec, &vopts);
    av_dict_free(&vopts);
    return ret;
}

/* Whether `codec` opens. Media Foundation's encoders are in every Windows
 * build but open only where Windows has one to give -- HEVC only on a GPU
 * that encodes it, there being no software HEVC encoder -- so they are
 * tried, the GPU's first as the writer does; any other encoder the build
 * carries opens. */
static int encoder_opens(const AVCodec *codec, int hevc) {
    if (!is_mf(codec)) {
        return 1;
    }
    const vb_av_encode_settings probe = {
        .video_bitrate = 1000000,
        .audio_bitrate = 128000,
        .width = 256,
        .height = 144,
        .fps_num = 30,
        .fps_den = 1,
        .sample_rate = 48000,
        .channels = 2,
        .hevc = hevc,
    };
    vb_av_writer w = {0};
    int opens = 0;
    for (int hardware = 1; hardware >= 0 && !opens; hardware--) {
        opens = open_video_encoder(&w, codec, &probe, 1, hardware) >= 0;
        avcodec_free_context(&w.venc);
    }
    return opens;
}

int vb_av_can_encode(int32_t hevc) {
    begin_call();
    const AVCodec *codec = video_encoder(hevc);
    return codec && encoder_opens(codec, hevc) && avcodec_find_encoder_by_name("aac") &&
           av_guess_format("mp4", NULL, NULL);
}

static void writer_free(vb_av_writer *w) {
    if (!w) {
        return;
    }
    if (w->oc && !(w->oc->oformat->flags & AVFMT_NOFILE)) {
        avio_closep(&w->oc->pb);
    }
    avformat_free_context(w->oc);
    avcodec_free_context(&w->venc);
    avcodec_free_context(&w->aenc);
    sws_freeContext(w->sws);
    av_frame_free(&w->vframe);
    av_frame_free(&w->aframe);
    if (w->fifo) {
        av_audio_fifo_free(w->fifo);
    }
    if (w->planes) {
        for (uint32_t c = 0; c < w->channels; c++) {
            av_free(w->planes[c]);
        }
        av_free(w->planes);
    }
    av_packet_free(&w->pkt);
    av_free(w);
}

void vb_av_writer_abort(vb_av_writer *w) {
    writer_free(w);
}

/* Send `frame` (NULL to flush) and write every packet that comes out. */
static int writer_encode(vb_av_writer *w, AVCodecContext *enc, AVStream *st, AVFrame *frame,
                         char *err, size_t err_len) {
    int ret = avcodec_send_frame(enc, frame);
    if (ret < 0 && !(frame == NULL && ret == AVERROR_EOF)) {
        set_av_err(err, err_len, "the encoder refused a frame", ret);
        return VB_AV_ERR_IO;
    }
    for (;;) {
        ret = avcodec_receive_packet(enc, w->pkt);
        if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
            return VB_AV_OK;
        }
        if (ret < 0) {
            set_av_err(err, err_len, "encoding failed", ret);
            return VB_AV_ERR_IO;
        }
        av_packet_rescale_ts(w->pkt, enc->time_base, st->time_base);
        w->pkt->stream_index = st->index;
        ret = av_interleaved_write_frame(w->oc, w->pkt);
        if (ret < 0) {
            set_av_err(err, err_len, "could not write the video", ret);
            return VB_AV_ERR_IO;
        }
    }
}

int vb_av_writer_open(const char *dest, const vb_av_encode_settings *settings,
                      vb_av_writer **out, char *err, size_t err_len) {
    if (!dest || !settings || !out || settings->width == 0 || settings->height == 0 ||
        settings->width % 2 || settings->height % 2 || settings->width > 8192 ||
        settings->height > 8192 || settings->fps_num == 0 || settings->fps_den == 0 ||
        settings->sample_rate == 0 || settings->channels == 0 || settings->channels > 8) {
        set_err(err, err_len, "bad argument: cannot write %ux%u", settings ? settings->width : 0,
                settings ? settings->height : 0);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *out = NULL;
    const AVCodec *vcodec = video_encoder(settings->hevc);
    const AVCodec *acodec = avcodec_find_encoder_by_name("aac");
    if (!vcodec || !acodec) {
        set_err(err, err_len, "this build has no %s encoder",
                !vcodec ? (settings->hevc ? "HEVC" : "H.264") : "AAC");
        return VB_AV_ERR_UNSUPPORTED;
    }

    vb_av_writer *w = av_mallocz(sizeof *w);
    if (!w) {
        set_err(err, err_len, "out of memory opening the encoder");
        return VB_AV_ERR_DECODE;
    }
    w->width = settings->width;
    w->height = settings->height;
    w->channels = settings->channels;
    int ret = avformat_alloc_output_context2(&w->oc, NULL, NULL, dest);
    if (ret < 0 || !w->oc) {
        writer_free(w);
        set_av_err(err, err_len, "cannot write this kind of file", ret < 0 ? ret : AVERROR(EINVAL));
        return VB_AV_ERR_UNSUPPORTED;
    }
    int global_header = (w->oc->oformat->flags & AVFMT_GLOBALHEADER) != 0;

    /* The picture. */
    w->pkt = av_packet_alloc();
    w->vframe = av_frame_alloc();
    w->aframe = av_frame_alloc();
    if (!w->pkt || !w->vframe || !w->aframe) {
        writer_free(w);
        set_err(err, err_len, "out of memory opening the encoder");
        return VB_AV_ERR_DECODE;
    }
    ret = open_video_encoder(w, vcodec, settings, global_header, 1);
    if (ret < 0 && ret != AVERROR(ENOMEM) && is_mf(vcodec)) {
        /* No GPU encoder Media Foundation will use -- a virtual machine,
         * a GPU without one: Microsoft's software encoder instead, which
         * writes H.264 only. */
        avcodec_free_context(&w->venc);
        ret = open_video_encoder(w, vcodec, settings, global_header, 0);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the video encoder", ret);
        writer_free(w);
        return ret == AVERROR(ENOMEM) ? VB_AV_ERR_DECODE : VB_AV_ERR_UNSUPPORTED;
    }
    w->vst = avformat_new_stream(w->oc, NULL);
    if (!w->vst) {
        writer_free(w);
        set_err(err, err_len, "out of memory adding the video stream");
        return VB_AV_ERR_DECODE;
    }
    w->vst->time_base = w->venc->time_base;
    w->vst->avg_frame_rate = w->venc->framerate;
    ret = avcodec_parameters_from_context(w->vst->codecpar, w->venc);
    if (ret < 0) {
        set_av_err(err, err_len, "could not describe the video stream", ret);
        writer_free(w);
        return VB_AV_ERR_DECODE;
    }
    if (settings->hevc) {
        /* QuickTime and the Photos app play HEVC in MP4 only as hvc1. */
        w->vst->codecpar->codec_tag = MKTAG('h', 'v', 'c', '1');
    }

    w->vframe->format = w->venc->pix_fmt;
    w->vframe->width = w->venc->width;
    w->vframe->height = w->venc->height;
    w->vframe->colorspace = AVCOL_SPC_BT709;
    w->vframe->color_primaries = AVCOL_PRI_BT709;
    w->vframe->color_trc = AVCOL_TRC_BT709;
    w->vframe->color_range = AVCOL_RANGE_MPEG;
    ret = av_frame_get_buffer(w->vframe, 0);
    if (ret < 0) {
        set_av_err(err, err_len, "could not allocate a frame", ret);
        writer_free(w);
        return VB_AV_ERR_DECODE;
    }
    /* RGB into BT.709 limited range, as HD video is. */
    w->sws = sws_alloc_context();
    if (!w->sws || av_opt_set_int(w->sws, "srcw", w->venc->width, 0) < 0 ||
        av_opt_set_int(w->sws, "srch", w->venc->height, 0) < 0 ||
        av_opt_set_int(w->sws, "src_format", AV_PIX_FMT_RGBA, 0) < 0 ||
        av_opt_set_int(w->sws, "dstw", w->venc->width, 0) < 0 ||
        av_opt_set_int(w->sws, "dsth", w->venc->height, 0) < 0 ||
        av_opt_set_int(w->sws, "dst_format", w->venc->pix_fmt, 0) < 0 ||
        av_opt_set_int(w->sws, "sws_flags", SWS_BICUBIC, 0) < 0 ||
        av_opt_set_int(w->sws, "src_range", 1, 0) < 0 ||
        av_opt_set_int(w->sws, "dst_range", 0, 0) < 0 || sws_init_context(w->sws, NULL, NULL) < 0) {
        writer_free(w);
        set_err(err, err_len, "cannot convert rgba frames to %s",
                av_get_pix_fmt_name(w->vframe->format));
        return VB_AV_ERR_DECODE;
    }
    sws_setColorspaceDetails(w->sws, sws_getCoefficients(SWS_CS_DEFAULT), 1,
                             sws_getCoefficients(SWS_CS_ITU709), 0, 0, 1 << 16, 1 << 16);

    /* The sound. */
    w->aenc = avcodec_alloc_context3(acodec);
    if (!w->aenc) {
        writer_free(w);
        set_err(err, err_len, "out of memory opening the audio encoder");
        return VB_AV_ERR_DECODE;
    }
    w->aenc->sample_fmt = AV_SAMPLE_FMT_FLTP;
    w->aenc->sample_rate = (int)settings->sample_rate;
    av_channel_layout_default(&w->aenc->ch_layout, (int)settings->channels);
    w->aenc->bit_rate = (int64_t)settings->audio_bitrate;
    w->aenc->time_base = (AVRational){1, (int)settings->sample_rate};
    if (global_header) {
        w->aenc->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    }
    ret = avcodec_open2(w->aenc, acodec, NULL);
    if (ret < 0) {
        set_av_err(err, err_len, "could not open the audio encoder", ret);
        writer_free(w);
        return VB_AV_ERR_UNSUPPORTED;
    }
    w->ast = avformat_new_stream(w->oc, NULL);
    if (!w->ast) {
        writer_free(w);
        set_err(err, err_len, "out of memory adding the audio stream");
        return VB_AV_ERR_DECODE;
    }
    w->ast->time_base = w->aenc->time_base;
    ret = avcodec_parameters_from_context(w->ast->codecpar, w->aenc);
    if (ret < 0) {
        set_av_err(err, err_len, "could not describe the audio stream", ret);
        writer_free(w);
        return VB_AV_ERR_DECODE;
    }
    w->frame_size = w->aenc->frame_size > 0 ? w->aenc->frame_size : 1024;
    w->fifo = av_audio_fifo_alloc(AV_SAMPLE_FMT_FLTP, (int)settings->channels, w->frame_size * 4);
    w->planes = av_calloc(settings->channels, sizeof *w->planes);
    if (!w->fifo || !w->planes) {
        writer_free(w);
        set_err(err, err_len, "out of memory opening the audio encoder");
        return VB_AV_ERR_DECODE;
    }

    if (!(w->oc->oformat->flags & AVFMT_NOFILE)) {
        ret = avio_open(&w->oc->pb, dest, AVIO_FLAG_WRITE);
        if (ret < 0) {
            set_av_err(err, err_len, "could not create the file", ret);
            writer_free(w);
            return VB_AV_ERR_IO;
        }
    }
    AVDictionary *mux = NULL;
    /* The index at the front, so the piece plays while it is still coming
     * down a share or a cable. */
    av_dict_set(&mux, "movflags", "+faststart", 0);
    ret = avformat_write_header(w->oc, &mux);
    av_dict_free(&mux);
    if (ret < 0) {
        set_av_err(err, err_len, "could not start the file", ret);
        writer_free(w);
        return VB_AV_ERR_IO;
    }
    w->header_written = 1;
    *out = w;
    return VB_AV_OK;
}

int vb_av_writer_video(vb_av_writer *w, const uint8_t *rgba, char *err, size_t err_len) {
    if (!w || !rgba) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    int ret = av_frame_make_writable(w->vframe);
    if (ret < 0) {
        set_av_err(err, err_len, "could not reuse the frame", ret);
        return VB_AV_ERR_DECODE;
    }
    const uint8_t *src[4] = {rgba, NULL, NULL, NULL};
    int stride[4] = {(int)w->width * 4, 0, 0, 0};
    if (sws_scale(w->sws, src, stride, 0, (int)w->height, w->vframe->data, w->vframe->linesize) <=
        0) {
        set_err(err, err_len, "converting the frame produced no output");
        return VB_AV_ERR_DECODE;
    }
    w->vframe->pts = w->vpts++;
    return writer_encode(w, w->venc, w->vst, w->vframe, err, err_len);
}

/* `count` samples from the queue into the encoder. */
static int writer_audio_frame(vb_av_writer *w, int count, char *err, size_t err_len) {
    av_frame_unref(w->aframe);
    w->aframe->format = AV_SAMPLE_FMT_FLTP;
    w->aframe->sample_rate = w->aenc->sample_rate;
    w->aframe->nb_samples = count;
    int ret = av_channel_layout_copy(&w->aframe->ch_layout, &w->aenc->ch_layout);
    if (ret >= 0) {
        ret = av_frame_get_buffer(w->aframe, 0);
    }
    if (ret >= 0 && av_audio_fifo_read(w->fifo, (void **)w->aframe->data, count) < count) {
        ret = AVERROR(EIO);
    }
    if (ret < 0) {
        set_av_err(err, err_len, "could not queue the sound", ret);
        return VB_AV_ERR_DECODE;
    }
    w->aframe->pts = w->apts;
    w->apts += count;
    return writer_encode(w, w->aenc, w->ast, w->aframe, err, err_len);
}

int vb_av_writer_audio(vb_av_writer *w, const float *interleaved, size_t frames, char *err,
                       size_t err_len) {
    if (!w || (!interleaved && frames > 0)) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    if (frames == 0) {
        return VB_AV_OK;
    }
    if (frames > w->planes_room) {
        for (uint32_t c = 0; c < w->channels; c++) {
            float *bigger = av_realloc(w->planes[c], frames * sizeof(float));
            if (!bigger) {
                set_err(err, err_len, "out of memory queueing the sound");
                return VB_AV_ERR_DECODE;
            }
            w->planes[c] = bigger;
        }
        w->planes_room = frames;
    }
    for (size_t i = 0; i < frames; i++) {
        for (uint32_t c = 0; c < w->channels; c++) {
            w->planes[c][i] = interleaved[i * w->channels + c];
        }
    }
    if (av_audio_fifo_write(w->fifo, (void **)w->planes, (int)frames) < (int)frames) {
        set_err(err, err_len, "out of memory queueing the sound");
        return VB_AV_ERR_DECODE;
    }
    while (av_audio_fifo_size(w->fifo) >= w->frame_size) {
        int rc = writer_audio_frame(w, w->frame_size, err, err_len);
        if (rc != VB_AV_OK) {
            return rc;
        }
    }
    return VB_AV_OK;
}

int vb_av_writer_finish(vb_av_writer *w, char *err, size_t err_len) {
    if (!w) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    int rc = VB_AV_OK;
    int rest = av_audio_fifo_size(w->fifo);
    if (rest > 0) {
        rc = writer_audio_frame(w, rest, err, err_len);
    }
    if (rc == VB_AV_OK) {
        rc = writer_encode(w, w->aenc, w->ast, NULL, err, err_len);
    }
    if (rc == VB_AV_OK) {
        rc = writer_encode(w, w->venc, w->vst, NULL, err, err_len);
    }
    if (rc == VB_AV_OK) {
        int ret = av_write_trailer(w->oc);
        if (ret < 0) {
            set_av_err(err, err_len, "could not finish the file", ret);
            rc = VB_AV_ERR_IO;
        }
    }
    writer_free(w);
    return rc;
}

/* ------------------------------------------------------------------------ */
/* Lossless export: keyframes, fingerprints, and stretches joined by copying  */
/* ------------------------------------------------------------------------ */

/*
 * A cut whose clips are untouched footage can leave without being decoded
 * or encoded again: each clip's packets are copied, from a keyframe, and
 * the clips joined end to end -- seconds for any length, and nothing lost.
 *
 * Not through the concat demuxer: it ends a stretch when any stream reaches
 * the out point, and a phone stores its sound ahead of its picture, so on
 * real footage every seam lost the last third of a second of picture. Each
 * stretch is read here twice: once for where its picture really ends, then
 * for its packets, the sound clipped to that end, the next stretch placed
 * where this one ended.
 */

static int times_push(int64_t **times, size_t *count, size_t *room, int64_t t) {
    if (*count == *room) {
        size_t grown = *room ? *room * 2 : 64;
        int64_t *bigger = av_realloc(*times, grown * sizeof **times);
        if (!bigger) {
            return 0;
        }
        *times = bigger;
        *room = grown;
    }
    (*times)[(*count)++] = t;
    return 1;
}

static int compare_times(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

int vb_av_keyframes(const char *path, int64_t from_us, int64_t to_us, int64_t **out,
                    size_t *count, char *err, size_t err_len) {
    if (!path || !out || !count || from_us < 0 || to_us < from_us) {
        set_err(err, err_len, "bad argument: keyframes from %lld to %lld us",
                (long long)from_us, (long long)to_us);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    *out = NULL;
    *count = 0;
    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    int index = find_stream(fmt, AVMEDIA_TYPE_VIDEO);
    if (index < 0) {
        avformat_close_input(&fmt);
        set_err(err, err_len, "the file has no video stream");
        return VB_AV_ERR_NO_VIDEO;
    }
    AVStream *st = fmt->streams[index];
    for (unsigned i = 0; i < fmt->nb_streams; i++) {
        if ((int)i != index) {
            fmt->streams[i]->discard = AVDISCARD_ALL;
        }
    }
    int64_t origin = fmt->start_time == AV_NOPTS_VALUE ? 0 : fmt->start_time;
    /* To the keyframe at or before `from`, as ffprobe's read intervals go. */
    int64_t target =
        av_rescale_q_rnd(from_us + origin, AV_TIME_BASE_Q, st->time_base, AV_ROUND_DOWN);
    (void)av_seek_frame(fmt, index, target, AVSEEK_FLAG_BACKWARD);

    AVPacket *pkt = av_packet_alloc();
    int64_t *times = NULL;
    size_t room = 0;
    rc = VB_AV_OK;
    if (!pkt) {
        rc = VB_AV_ERR_DECODE;
        set_err(err, err_len, "out of memory reading packets");
    }
    while (rc == VB_AV_OK && av_read_frame(fmt, pkt) >= 0) {
        if (pkt->stream_index != index) {
            av_packet_unref(pkt);
            continue;
        }
        int64_t dts = pkt->dts != AV_NOPTS_VALUE ? pkt->dts : pkt->pts;
        /* In decoding order every later packet is shown no earlier than its
         * own decode time, so once that passes `to` nothing more is due. */
        if (dts != AV_NOPTS_VALUE &&
            av_rescale_q(dts, st->time_base, AV_TIME_BASE_Q) - origin > to_us) {
            av_packet_unref(pkt);
            break;
        }
        if ((pkt->flags & AV_PKT_FLAG_KEY) && pkt->pts != AV_NOPTS_VALUE) {
            int64_t t = av_rescale_q(pkt->pts, st->time_base, AV_TIME_BASE_Q) - origin;
            if (t >= from_us && t <= to_us && !times_push(&times, count, &room, t)) {
                rc = VB_AV_ERR_DECODE;
                set_err(err, err_len, "out of memory listing keyframes");
            }
        }
        av_packet_unref(pkt);
    }
    av_packet_free(&pkt);
    avformat_close_input(&fmt);
    if (rc != VB_AV_OK) {
        av_free(times);
        *count = 0;
        return rc;
    }
    if (*count > 1) {
        qsort(times, *count, sizeof *times, compare_times);
    }
    *out = times;
    return VB_AV_OK;
}

void vb_av_free_times(int64_t *times) {
    av_free(times);
}

/* FNV-1a over a codec's own header: two streams that differ there cannot be
 * joined into one track. */
static uint64_t header_hash(const uint8_t *data, int size) {
    uint64_t h = 1469598103934665603ULL;
    for (int i = 0; data && i < size; i++) {
        h ^= data[i];
        h *= 1099511628211ULL;
    }
    return h;
}

int vb_av_copy_fingerprint(const char *path, char *out, size_t out_len, char *err,
                           size_t err_len) {
    if (!path || !out || out_len == 0) {
        set_err(err, err_len, "null argument");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    AVFormatContext *fmt = NULL;
    int rc = open_input(path, &fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    int vi = find_stream(fmt, AVMEDIA_TYPE_VIDEO);
    if (vi < 0) {
        avformat_close_input(&fmt);
        set_err(err, err_len, "the file has no video stream");
        return VB_AV_ERR_NO_VIDEO;
    }
    const AVCodecParameters *v = fmt->streams[vi]->codecpar;
    const char *pix = av_get_pix_fmt_name((enum AVPixelFormat)v->format);
    int n = snprintf(out, out_len,
                     "v:%s/%d/%d/%dx%d/%s/%d/%d/%d/%d/%d:%d/%d/rot%d/x%016llx",
                     avcodec_get_name(v->codec_id), v->profile, v->level, v->width, v->height,
                     pix ? pix : "-", (int)v->color_range, (int)v->color_space,
                     (int)v->color_trc, (int)v->color_primaries, v->sample_aspect_ratio.num,
                     v->sample_aspect_ratio.den, (int)v->field_order,
                     stream_rotation(fmt->streams[vi]),
                     (unsigned long long)header_hash(v->extradata, v->extradata_size));
    int ai = find_stream(fmt, AVMEDIA_TYPE_AUDIO);
    if (n >= 0 && (size_t)n < out_len) {
        if (ai >= 0) {
            const AVCodecParameters *a = fmt->streams[ai]->codecpar;
            snprintf(out + n, out_len - (size_t)n, "|a:%s/%d/%d/%d/x%016llx",
                     avcodec_get_name(a->codec_id), a->profile, a->sample_rate,
                     a->ch_layout.nb_channels,
                     (unsigned long long)header_hash(a->extradata, a->extradata_size));
        } else {
            snprintf(out + n, out_len - (size_t)n, "|a:none");
        }
    }
    avformat_close_input(&fmt);
    return VB_AV_OK;
}

int vb_av_can_copy(void) {
    return av_guess_format("mp4", NULL, NULL) != NULL;
}

/* One stretch open for copying: its file, its picture and sound, where it
 * starts and where the picture it keeps really ends. */
typedef struct {
    AVFormatContext *fmt;
    int vi;
    int ai;
    int64_t origin;
} JoinInput;

static void join_close(JoinInput *in) {
    avformat_close_input(&in->fmt);
}

static int join_open(const vb_av_part *part, int want_audio, JoinInput *in, char *err,
                     size_t err_len) {
    memset(in, 0, sizeof *in);
    in->vi = in->ai = -1;
    int rc = open_input(part->path, &in->fmt, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    in->vi = find_stream(in->fmt, AVMEDIA_TYPE_VIDEO);
    in->ai = want_audio ? find_stream(in->fmt, AVMEDIA_TYPE_AUDIO) : -1;
    if (in->vi < 0) {
        join_close(in);
        set_err(err, err_len, "%s has no video stream", part->path);
        return VB_AV_ERR_NO_VIDEO;
    }
    for (unsigned i = 0; i < in->fmt->nb_streams; i++) {
        if ((int)i != in->vi && (int)i != in->ai) {
            in->fmt->streams[i]->discard = AVDISCARD_ALL;
        }
    }
    in->origin = in->fmt->start_time == AV_NOPTS_VALUE ? 0 : in->fmt->start_time;
    return VB_AV_OK;
}

/* To the keyframe the stretch starts on. */
static void join_seek(JoinInput *in, int64_t from_us) {
    AVStream *st = in->fmt->streams[in->vi];
    int64_t target =
        av_rescale_q_rnd(from_us + in->origin, AV_TIME_BASE_Q, st->time_base, AV_ROUND_DOWN);
    (void)av_seek_frame(in->fmt, in->vi, target, AVSEEK_FLAG_BACKWARD);
}

/* A packet's time in microseconds of the file's own clock. */
static int64_t join_us(const JoinInput *in, int stream, int64_t ts) {
    return av_rescale_q(ts, in->fmt->streams[stream]->time_base, AV_TIME_BASE_Q) - in->origin;
}

/* A picture packet of a stretch, in microseconds of its file's clock. */
typedef struct {
    int64_t pts, dts;
} JoinFrame;

/* How much of the stretch's picture is kept, found by a first pass over the
 * picture alone: in decoding order from its keyframe, every frame shown
 * from the start on (a leading frame that needs the picture before the
 * keyframe is not), up to the last frame shown before the end -- and then
 * on until no frame left out would be shown before one kept. A stretch
 * that ends inside a group of B-frames waits for that group's reference
 * frame, shown after the end; stopping at the last frame before the end
 * would leave the frames between unshown, the picture frozen for as many
 * as the group holds (eight, a quarter of a second, on one phone's
 * footage), then the reference frame flashing. Carried on, the stretch
 * ends at the same moment with every frame in it shown. `keep` is how many
 * packets that is, and `end_us` where they end: when the next frame would
 * have been shown, or a frame after the last at the stretch's own rate --
 * never by the packets' durations, which some files spend on decoding
 * rather than showing (one phone's compressor gives each group's frames
 * 0.1 ms and its last a quarter of a second). */
static int join_measure(const vb_av_part *part, int64_t *end_us, int64_t *keep, char *err,
                        size_t err_len) {
    JoinInput in;
    int rc = join_open(part, 0, &in, err, err_len);
    if (rc != VB_AV_OK) {
        return rc;
    }
    join_seek(&in, part->from_us);
    AVPacket *pkt = av_packet_alloc();
    JoinFrame *frames = NULL;
    size_t count = 0, room = 0;
    if (!pkt) {
        join_close(&in);
        set_err(err, err_len, "out of memory reading packets");
        return VB_AV_ERR_DECODE;
    }
    AVStream *st = in.fmt->streams[in.vi];
    /* A frame's length with only one frame to go by: the stream's rate. */
    AVRational rate = st->avg_frame_rate.num > 0 ? st->avg_frame_rate : st->r_frame_rate;
    int64_t frame_us = rate.num > 0 ? av_rescale(1000000, rate.den, rate.num) : 33333;
    int64_t latest = INT64_MIN, beyond = INT64_MAX;
    while (rc == VB_AV_OK && av_read_frame(in.fmt, pkt) >= 0) {
        if (pkt->stream_index != in.vi || pkt->pts == AV_NOPTS_VALUE) {
            av_packet_unref(pkt);
            continue;
        }
        int64_t pts = join_us(&in, in.vi, pkt->pts);
        int64_t dts = pkt->dts != AV_NOPTS_VALUE ? join_us(&in, in.vi, pkt->dts) : pts;
        /* Decoded after the end and after every frame read is shown: from
         * here on nothing is shown before either. */
        if (dts >= part->to_us && dts > latest) {
            beyond = pts;
            av_packet_unref(pkt);
            break;
        }
        if (pts >= part->from_us) {
            if (count == room) {
                size_t grown = room ? room * 2 : 256;
                JoinFrame *bigger = av_realloc(frames, grown * sizeof *frames);
                if (!bigger) {
                    set_err(err, err_len, "out of memory reading packets");
                    rc = VB_AV_ERR_DECODE;
                    av_packet_unref(pkt);
                    break;
                }
                frames = bigger;
                room = grown;
            }
            frames[count++] = (JoinFrame){pts, dts};
            if (pts > latest) {
                latest = pts;
            }
        }
        av_packet_unref(pkt);
    }
    av_packet_free(&pkt);
    join_close(&in);
    if (rc != VB_AV_OK) {
        av_free(frames);
        return rc;
    }
    size_t n = 0;
    for (size_t i = 0; i < count; i++) {
        if (frames[i].pts < part->to_us) {
            n = i + 1;
        }
    }
    int64_t shown = INT64_MIN, first = INT64_MAX;
    for (size_t i = 0; i < n; i++) {
        shown = FFMAX(shown, frames[i].pts);
        first = FFMIN(first, frames[i].pts);
    }
    /* On while a frame not yet kept is shown before the latest kept. */
    int64_t later = beyond;
    while (n > 0 && n < count) {
        later = beyond;
        for (size_t i = n; i < count; i++) {
            later = FFMIN(later, frames[i].pts);
        }
        if (later > shown) {
            break;
        }
        shown = FFMAX(shown, frames[n].pts);
        n++;
        later = beyond;
    }
    int64_t end = part->from_us;
    if (n > 0) {
        /* The frames kept are shown one after another, none missing, so
         * their spacing is the stretch's rate. */
        int64_t step = n > 1 ? (shown - first) / (int64_t)(n - 1) : frame_us;
        end = shown + FFMAX(step, 1);
        if (later > shown && later < end) {
            end = later;
        }
    }
    av_free(frames);
    *keep = (int64_t)n;
    *end_us = end;
    return VB_AV_OK;
}

int vb_av_copy_end(const char *path, int64_t from_us, int64_t to_us, int64_t *end_us, char *err,
                   size_t err_len) {
    if (!path || !end_us || from_us < 0 || to_us < from_us) {
        set_err(err, err_len, "bad argument: a copy from %lld to %lld us", (long long)from_us,
                (long long)to_us);
        return VB_AV_ERR_ARG;
    }
    begin_call();
    vb_av_part part = {path, from_us, to_us};
    int64_t keep = 0;
    return join_measure(&part, end_us, &keep, err, err_len);
}

int vb_av_join(const vb_av_part *parts, size_t count, const char *dest, int32_t audio,
               vb_av_share_cb cb, void *ctx, char *err, size_t err_len) {
    if (!parts || count == 0 || !dest) {
        set_err(err, err_len, "nothing to join");
        return VB_AV_ERR_ARG;
    }
    begin_call();
    int rc = VB_AV_OK;
    int ret = 0;
    AVFormatContext *oc = NULL;
    AVPacket *pkt = av_packet_alloc();
    AVDictionary *mux = NULL;
    JoinInput in;
    memset(&in, 0, sizeof in);
    int out_v = -1, out_a = -1;
    int64_t last_dts[2] = {AV_NOPTS_VALUE, AV_NOPTS_VALUE};
    /* Where each stretch's picture ends, how many of its picture's packets
     * it keeps, where it starts in the finished piece, and how long all are. */
    int64_t *ends = av_calloc(count, sizeof *ends);
    int64_t *keeps = av_calloc(count, sizeof *keeps);
    int64_t total = 0, offset = 0;
    double reported = 0;
    if (!pkt || !ends || !keeps) {
        set_err(err, err_len, "out of memory joining");
        rc = VB_AV_ERR_DECODE;
        goto done;
    }
    for (size_t i = 0; i < count; i++) {
        rc = join_measure(&parts[i], &ends[i], &keeps[i], err, err_len);
        if (rc != VB_AV_OK) {
            goto done;
        }
        total += ends[i] - parts[i].from_us;
    }

    /* The piece's streams, from the first stretch's. */
    rc = join_open(&parts[0], audio, &in, err, err_len);
    if (rc != VB_AV_OK) {
        goto done;
    }
    ret = avformat_alloc_output_context2(&oc, NULL, NULL, dest);
    if (ret < 0 || !oc) {
        set_av_err(err, err_len, "cannot write this kind of file", ret < 0 ? ret : AVERROR(EINVAL));
        rc = VB_AV_ERR_UNSUPPORTED;
        goto done;
    }
    int sources[2] = {in.vi, in.ai};
    for (int k = 0; k < 2; k++) {
        if (sources[k] < 0) {
            continue;
        }
        AVStream *ist = in.fmt->streams[sources[k]];
        AVStream *ost = avformat_new_stream(oc, NULL);
        if (!ost || avcodec_parameters_copy(ost->codecpar, ist->codecpar) < 0) {
            set_err(err, err_len, "could not describe a stream of the piece");
            rc = VB_AV_ERR_DECODE;
            goto done;
        }
        /* QuickTime and Photos play HEVC in MP4 only as hvc1. */
        ost->codecpar->codec_tag =
            ost->codecpar->codec_id == AV_CODEC_ID_HEVC ? MKTAG('h', 'v', 'c', '1') : 0;
        ost->time_base = ist->time_base;
        if (k == 0) {
            out_v = ost->index;
        } else {
            out_a = ost->index;
        }
    }
    join_close(&in);
    if (!(oc->oformat->flags & AVFMT_NOFILE)) {
        ret = avio_open(&oc->pb, dest, AVIO_FLAG_WRITE);
        if (ret < 0) {
            set_av_err(err, err_len, "could not create the file", ret);
            rc = VB_AV_ERR_IO;
            goto done;
        }
    }
    av_dict_set(&mux, "movflags", "+faststart", 0);
    ret = avformat_write_header(oc, &mux);
    if (ret < 0) {
        set_av_err(err, err_len, "could not start the file", ret);
        rc = VB_AV_ERR_IO;
        goto done;
    }

    for (size_t i = 0; i < count && rc == VB_AV_OK; i++) {
        const vb_av_part *part = &parts[i];
        rc = join_open(part, out_a >= 0, &in, err, err_len);
        if (rc != VB_AV_OK) {
            break;
        }
        if (out_a >= 0 && in.ai < 0) {
            set_err(err, err_len, "%s has no sound, which the others have", part->path);
            rc = VB_AV_ERR_NO_AUDIO;
            break;
        }
        join_seek(&in, part->from_us);
        int video_past = keeps[i] == 0, audio_past = in.ai < 0;
        int64_t video_kept = 0, sound_end = part->from_us;
        while (!(video_past && audio_past) && (ret = av_read_frame(in.fmt, pkt)) >= 0) {
            int k = pkt->stream_index == in.vi ? 0 : pkt->stream_index == in.ai ? 1 : -1;
            int keep = 0;
            if (k == 0 && !video_past) {
                /* The packets the first pass counted, in the same order. */
                keep = pkt->pts != AV_NOPTS_VALUE &&
                       join_us(&in, in.vi, pkt->pts) >= part->from_us;
                if (keep && ++video_kept >= keeps[i]) {
                    video_past = 1;
                }
            } else if (k == 1 && pkt->pts != AV_NOPTS_VALUE) {
                /* The sound under the picture kept: from the stretch's start
                 * to where its picture ends, packet by whole packet. */
                int64_t at = join_us(&in, in.ai, pkt->pts);
                if (at >= ends[i]) {
                    audio_past = 1;
                } else if (at >= part->from_us) {
                    keep = 1;
                    int64_t length =
                        pkt->duration > 0
                            ? av_rescale_q(pkt->duration, in.fmt->streams[in.ai]->time_base,
                                           AV_TIME_BASE_Q)
                            : 0;
                    if (at + length > sound_end) {
                        sound_end = at + length;
                    }
                }
            }
            if (!keep) {
                av_packet_unref(pkt);
                continue;
            }
            AVStream *ist = in.fmt->streams[pkt->stream_index];
            AVStream *ost = oc->streams[k == 0 ? out_v : out_a];
            /* Onto the piece's clock: this stretch starts where the last
             * one's picture ended. */
            int64_t shift = offset - part->from_us - in.origin;
            int64_t shift_ts = av_rescale_q(shift, AV_TIME_BASE_Q, ist->time_base);
            if (pkt->pts != AV_NOPTS_VALUE) {
                pkt->pts += shift_ts;
            }
            if (pkt->dts != AV_NOPTS_VALUE) {
                pkt->dts += shift_ts;
            }
            av_packet_rescale_ts(pkt, ist->time_base, ost->time_base);
            /* Never decoded before the packet ahead of it, as the ffmpeg CLI
             * keeps it: a stretch's first frames are decoded a frame or two
             * before they are shown. */
            if (pkt->dts != AV_NOPTS_VALUE && last_dts[k] != AV_NOPTS_VALUE) {
                int64_t floor = last_dts[k] + !(oc->oformat->flags & AVFMT_TS_NONSTRICT);
                if (pkt->dts < floor) {
                    if (pkt->pts != AV_NOPTS_VALUE && pkt->pts >= pkt->dts) {
                        pkt->pts = FFMAX(pkt->pts, floor);
                    }
                    pkt->dts = floor;
                }
            }
            if (pkt->dts != AV_NOPTS_VALUE) {
                last_dts[k] = pkt->dts;
            }
            if (k == 0 && cb && total > 0 && pkt->pts != AV_NOPTS_VALUE) {
                double share = (double)av_rescale_q(pkt->pts, ost->time_base, AV_TIME_BASE_Q) /
                               (double)total;
                share = share < 0 ? 0 : share > 1 ? 1 : share;
                if (share - reported >= 0.01) {
                    reported = share;
                    if (!cb(ctx, share)) {
                        av_packet_unref(pkt);
                        set_err(err, err_len, "stopped");
                        rc = VB_AV_ERR_STOPPED;
                        break;
                    }
                }
            }
            pkt->stream_index = ost->index;
            pkt->pos = -1;
            ret = av_interleaved_write_frame(oc, pkt);
            if (ret < 0) {
                set_av_err(err, err_len, "could not write a packet", ret);
                rc = VB_AV_ERR_IO;
                break;
            }
        }
        av_packet_unref(pkt);
        join_close(&in);
        /* The next stretch starts after this one's picture and its sound, as
         * the concat demuxer places whole files: the last packet of sound
         * runs up to one past the picture, and an overlap would push each
         * later stretch's sound off its picture. */
        offset += FFMAX(ends[i], sound_end) - part->from_us;
    }
    if (rc == VB_AV_OK) {
        ret = av_write_trailer(oc);
        if (ret < 0) {
            set_av_err(err, err_len, "could not finish the file", ret);
            rc = VB_AV_ERR_IO;
        } else if (cb) {
            cb(ctx, 1.0);
        }
    }

done:
    join_close(&in);
    av_dict_free(&mux);
    av_packet_free(&pkt);
    av_free(ends);
    av_free(keeps);
    if (oc) {
        if (!(oc->oformat->flags & AVFMT_NOFILE)) {
            avio_closep(&oc->pb);
        }
        avformat_free_context(oc);
    }
    return rc;
}

/* ------------------------------------------------------------------------ */
/* Identity                                                                  */
/* ------------------------------------------------------------------------ */

const char *vb_av_version(void) {
    return av_version_info();
}

const char *vb_av_license(void) {
    return avcodec_license();
}

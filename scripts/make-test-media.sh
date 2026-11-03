#!/usr/bin/env bash
# Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as
# published by the Free Software Foundation, either version 3 of the
# License, or (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.

# Generate the fixtures the end-to-end tests run against.
#
# sample.mp4: four visually distinct scenes with hard cuts between them, so
# shot detection has unambiguous boundaries to find and the classical scorer
# has both a detail-free frame (navy) and a high-detail one (test pattern) to
# separate.
#
# beat-120.wav: a click every half second, which is 120 BPM exactly. Beat
# detection is only testable against a known tempo, and a click track is the
# one signal whose tempo is not a matter of opinion.
#
# sample-with-audio.mp4: video and that click track in one file, for the paths
# that decode a clip's own audio.
#
# photo-rotated.jpg: a photo stored on its side, 800x600, with EXIF saying to
# turn it a quarter clockwise (orientation 6) and when it was taken, with its
# zone -- what a phone held upright writes. A red block marks the stored
# top-left corner, so a test can see which way it was turned.
# photo.png: a PNG with no EXIF at all. photo-grid.heic (only where macOS's
# sips can write one): a HEIC big enough to be stored as a grid of tiles,
# which is how an iPhone stores every photo.
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p test-media

ffmpeg -y -v error \
  -f lavfi -i "testsrc2=size=1280x720:rate=30:duration=6" \
  -f lavfi -i "color=c=navy:size=1280x720:rate=30:duration=5" \
  -f lavfi -i "smptebars=size=1280x720:rate=30:duration=7" \
  -f lavfi -i "testsrc=size=1280x720:rate=30:duration=6" \
  -filter_complex "[0:v][1:v][2:v][3:v]concat=n=4:v=1:a=0[v]" \
  -map "[v]" -c:v libx264 -pix_fmt yuv420p -g 30 \
  test-media/sample.mp4

# 0.6 amplitude sine at 1 kHz, retriggered every 0.5 s and decaying fast: a
# metronome, whose onsets are unambiguous.
CLICK="aevalsrc='0.6*sin(2*PI*1000*t)*exp(-40*mod(t,0.5))':d=12:s=44100"

ffmpeg -y -v error -f lavfi -i "$CLICK" test-media/beat-120.wav

ffmpeg -y -v error \
  -f lavfi -i "testsrc2=size=640x360:rate=30:duration=12" \
  -f lavfi -i "$CLICK" \
  -map 0:v -map 1:a -c:v libx264 -pix_fmt yuv420p -g 30 -c:a aac -shortest \
  test-media/sample-with-audio.mp4

ffmpeg -y -v error \
  -f lavfi -i "color=c=gray:size=800x600:rate=1:duration=1" \
  -vf "drawbox=x=0:y=0:w=160:h=120:color=red:t=fill,drawgrid=w=40:h=40:color=white" \
  -frames:v 1 -q:v 2 test-media/photo-plain.jpg
# ffmpeg writes no EXIF, so the APP1 segment is spliced in after SOI.
python3 - test-media/photo-plain.jpg test-media/photo-rotated.jpg <<'EXIF'
import struct, sys
def ifd(entries, next_ifd=0):
    out = struct.pack(">H", len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack(">HHI", tag, kind, count) + value
    return out + struct.pack(">I", next_ifd)
taken, zone = b"2023:09:29 18:31:43\0", b"+08:00\0"
# TIFF header (8), IFD0 with two entries (2 + 24 + 4), then the Exif IFD.
exif_at = 8 + 2 + 2 * 12 + 4
data_at = exif_at + 2 + 2 * 12 + 4
ifd0 = ifd([
    (0x0112, 3, 1, struct.pack(">HH", 6, 0)),    # Orientation: 6
    (0x8769, 4, 1, struct.pack(">I", exif_at)),  # the Exif IFD
])
exif_ifd = ifd([
    (0x9003, 2, len(taken), struct.pack(">I", data_at)),              # DateTimeOriginal
    (0x9011, 2, len(zone), struct.pack(">I", data_at + len(taken))),  # OffsetTimeOriginal
])
tiff = b"MM\0*" + struct.pack(">I", 8) + ifd0 + exif_ifd + taken + zone
app1 = b"Exif\0\0" + tiff
jpeg = open(sys.argv[1], "rb").read()
assert jpeg[:2] == b"\xff\xd8"
open(sys.argv[2], "wb").write(
    jpeg[:2] + b"\xff\xe1" + struct.pack(">H", len(app1) + 2) + app1 + jpeg[2:]
)
EXIF
rm test-media/photo-plain.jpg

ffmpeg -y -v error -f lavfi -i "smptebars=size=640x480:rate=1:duration=1" \
  -frames:v 1 test-media/photo.png

if command -v sips >/dev/null; then
  ffmpeg -y -v error -f lavfi -i "testsrc=size=1400x1000:rate=1:duration=1" \
    -frames:v 1 test-media/photo-grid-src.png
  sips -s format heic test-media/photo-grid-src.png --out test-media/photo-grid.heic >/dev/null
  rm test-media/photo-grid-src.png
  echo "wrote test-media/photo-grid.heic (1400x1000, stored as tiles)"
fi

echo "wrote test-media/photo-rotated.jpg (800x600 stored, EXIF orientation 6) and photo.png"
echo "wrote test-media/beat-120.wav (12s, a click every 0.5s)"
echo "wrote test-media/sample-with-audio.mp4 (12s, video with that click track)"
echo "wrote test-media/sample.mp4 ($(ffprobe -v error -show_entries format=duration -of csv=p=0 test-media/sample.mp4)s, 4 scenes)"

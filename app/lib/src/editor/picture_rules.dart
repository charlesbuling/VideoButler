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

/// The renderer's rules for where things go in the frame, in Dart.
///
/// When the playhead stops, the editor shows a frame the engine drew --
/// exactly what the exported video will hold. While it plays, the picture
/// comes from the platform's player and is placed here, by the same rules
/// `vb-render` follows: the picture fitted, then enlarged, moved and turned;
/// brightness, contrast and saturation by the same matrix; text in points
/// of a frame whose shorter side is 540; transitions blended the same way.
/// Kept in one place so the two cannot drift apart unnoticed.
library;

import 'dart:math' as math;
import 'dart:ui';

import 'edit_state.dart';

/// The points a frame's shorter side holds (`vb_render::SHORT_SIDE_POINTS`).
const shortSidePoints = 540.0;

/// Pixels per point on a frame of [size].
double pixelsPerPoint(Size size) => math.min(size.width, size.height) / shortSidePoints;

/// `#RRGGBB` or `#RRGGBBAA` as a colour; null when it is neither.
Color? parseColor(String? text) {
  if (text == null || !text.startsWith('#')) return null;
  final hex = text.substring(1);
  if (hex.length != 6 && hex.length != 8) return null;
  final value = int.tryParse(hex, radix: 16);
  if (value == null) return null;
  return hex.length == 6
      ? Color(0xFF000000 | value)
      : Color(((value & 0xFF) << 24) | (value >> 8));
}

/// A colour as the contract writes it: `#RRGGBB`, or `#RRGGBBAA` when it
/// is not opaque.
String colorText(Color c) {
  String two(double v) => (v * 255).round().clamp(0, 255).toRadixString(16).padLeft(2, '0').toUpperCase();
  final rgb = '#${two(c.r)}${two(c.g)}${two(c.b)}';
  return c.a >= 0.999 ? rgb : '$rgb${two(c.a)}';
}

/// Where a picture of [picture] size lands in a frame of [frame] size:
/// its centre, its drawn size and its turn. Fitted whole, then the
/// placement's scale, offset (shares of the frame) and rotation.
({Offset centre, Size size, double radians}) placePicture(
  Size picture,
  Size frame,
  Placement placement,
) {
  if (picture.isEmpty || frame.isEmpty) {
    return (centre: frame.center(Offset.zero), size: Size.zero, radians: 0);
  }
  final fit = math.min(frame.width / picture.width, frame.height / picture.height);
  return (
    centre: Offset(
      frame.width / 2 + placement.x * frame.width,
      frame.height / 2 + placement.y * frame.height,
    ),
    size: Size(picture.width * fit * placement.scale, picture.height * fit * placement.scale),
    radians: placement.rotationDeg * math.pi / 180,
  );
}

/// Whether a placed picture covers the whole frame, so nothing behind it
/// shows.
bool covers(Size picture, Size frame, Placement placement) {
  if (!placement.isFitted) return false;
  final fit = math.min(frame.width / picture.width, frame.height / picture.height);
  return picture.width * fit >= frame.width - 0.5 && picture.height * fit >= frame.height - 0.5;
}

/// The colour matrix of an adjustment, as `vb_render`'s `adjust` applies
/// it: `(v - 0.5) * contrast + 0.5 + brightness` per channel, then each
/// pixel moved from its Rec. 709 grey by the saturation.
List<double> adjustMatrix(Adjustment a) {
  const lr = 0.2126, lg = 0.7152, lb = 0.0722;
  final s = a.saturation, c = a.contrast;
  final t = 255 * (0.5 - 0.5 * c + a.brightness);
  List<double> row(int channel) => [
        c * (lr * (1 - s) + (channel == 0 ? s : 0)),
        c * (lg * (1 - s) + (channel == 1 ? s : 0)),
        c * (lb * (1 - s) + (channel == 2 ? s : 0)),
        0,
        t,
      ];
  return [
    ...row(0),
    ...row(1),
    ...row(2),
    0, 0, 0, 1, 0,
  ];
}

/// How a text sits in a frame of [frame] size, given its laid-out [box]:
/// the top left, as `vb_render::caption::position` places it.
Offset placeText(TextLook look, Size box, Size frame) {
  if (look.x != null && look.y != null) {
    final cx = look.x!.clamp(0.0, 1.0) * frame.width;
    final cy = look.y!.clamp(0.0, 1.0) * frame.height;
    return Offset(cx - box.width / 2, cy - box.height / 2);
  }
  final marginV = (look.marginVPct ?? 0.08).clamp(0.0, 0.5) * frame.height;
  final marginH = 0.05 * frame.width;
  final a = look.alignment;
  final left = a.endsWith('Left')
      ? marginH
      : a.endsWith('Right')
          ? frame.width - marginH - box.width
          : (frame.width - box.width) / 2;
  final top = a.startsWith('top')
      ? marginV
      : a.startsWith('middle')
          ? (frame.height - box.height) / 2
          : frame.height - marginV - box.height;
  return Offset(left, top);
}

/// How far into a transition `atUs` is, 0..1, for the clip that comes in.
double transitionProgress(EditClip incoming, int atUs) {
  if (incoming.overlapUs <= 0) return 1;
  return ((atUs - incoming.startUs) / incoming.overlapUs).clamp(0.0, 1.0);
}

/// The outgoing and incoming pictures' opacities at progress [p] of a
/// transition of [kind]: a dissolve crosses them, a fade to black goes down
/// to black over the first half and up from it over the second. Wipes are
/// drawn with a clip instead and keep both whole.
(double, double) blendOpacities(TransitionKind kind, double p) => switch (kind) {
      TransitionKind.fadeToBlack => p < 0.5 ? (1 - 2 * p, 0) : (0, 2 * p - 1),
      TransitionKind.wipeLeft || TransitionKind.wipeRight => (1, 1),
      _ => (1, p),
    };

/// The linear gain of a level in decibels, at most 1: the platform's player
/// cannot amplify, so above 0 dB the preview plays at full volume.
double linearGain(double db, {bool muted = false}) =>
    muted ? 0 : math.pow(10, db / 20).toDouble().clamp(0.0, 1.0);

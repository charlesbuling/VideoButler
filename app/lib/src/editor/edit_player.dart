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

/// Playing a cut in the editor, live, through the platform's own player.
///
/// The exported video is drawn by the engine, frame by frame; that is too
/// slow to watch as it plays, so while the editor plays, each clip's file
/// is played by the platform (AVFoundation on Apple, ExoPlayer on Android),
/// started at the right moment of its footage and at its speed, prepared a
/// couple of seconds before it is due, and laid out by [picture_rules] --
/// the renderer's own rules. The music plays under it with its fades. When
/// the playhead stops, the editor goes back to the engine's exact frame.
///
/// A clock drives it, not the players: a player that is late to start is
/// caught up, not waited for, so the cut keeps its length.
library;

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:video_player/video_player.dart';

import 'edit_state.dart';
import 'picture_rules.dart';

/// How far ahead a clip's player is opened and set at its first frame.
const _prepareAheadUs = 2000000;

/// How far a player may wander from the clock before it is put back.
const _driftUs = 250000;

class _ClipPlayer {
  _ClipPlayer(this.clip, this.controller);

  EditClip clip;
  final VideoPlayerController? controller;
  bool ready = false;
  bool failed = false;
  bool running = false;
  Future<void>? opening;

  Future<void> dispose() async {
    final c = controller;
    if (c != null) await c.dispose();
  }
}

class _SoundPlayer {
  _SoundPlayer(this.sound, this.controller);

  EditSound sound;
  final VideoPlayerController controller;
  bool ready = false;
  bool failed = false;
  bool running = false;
}

/// The cut, playing.
class EditPlayer extends ChangeNotifier {
  EditPlayer(this._state);

  /// Whether this platform can play footage live. The web build cannot
  /// open a local file; it steps through the engine's frames instead.
  static bool get available => !kIsWeb;

  EditState _state;
  final Stopwatch _clock = Stopwatch();
  int _originUs = 0;
  Timer? _ticker;
  final Map<String, _ClipPlayer> _clips = {};
  final Map<String, _SoundPlayer> _sounds = {};
  bool _disposed = false;

  EditState get state => _state;

  bool get playing => _ticker != null;

  /// Where the playhead is.
  int get positionUs =>
      playing ? math.min(_state.totalUs, _originUs + _clock.elapsedMicroseconds) : _originUs;

  /// The cut changed: playing stops, since what was prepared was for the
  /// old one.
  set state(EditState next) {
    if (identical(next, _state)) return;
    final at = positionUs;
    _state = next;
    if (playing) pause();
    _originUs = at.clamp(0, next.totalUs);
    unawaited(_releaseAll());
  }

  /// Play from [fromUs].
  void play(int fromUs) {
    if (_disposed || _state.totalUs <= 0) return;
    _originUs = fromUs >= _state.totalUs ? 0 : fromUs.clamp(0, _state.totalUs);
    _clock
      ..reset()
      ..start();
    _ticker?.cancel();
    _ticker = Timer.periodic(const Duration(milliseconds: 16), (_) => _tick());
    _tick();
  }

  /// Stop where it is.
  void pause() {
    final at = positionUs;
    _ticker?.cancel();
    _ticker = null;
    _clock.stop();
    _originUs = at;
    for (final p in _clips.values) {
      if (p.running) {
        p.running = false;
        unawaited(p.controller?.pause());
      }
    }
    for (final s in _sounds.values) {
      if (s.running) {
        s.running = false;
        unawaited(s.controller.pause());
      }
    }
    if (!_disposed) notifyListeners();
  }

  void _tick() {
    if (_disposed) return;
    final now = positionUs;
    if (now >= _state.totalUs) {
      pause();
      _originUs = _state.totalUs;
      notifyListeners();
      return;
    }
    final active = _state.clipsAt(now);
    final activeIds = {for (final c in active) c.id};

    // Open what is about to come, and start what is due.
    for (final clip in _state.clips) {
      final due = clip.startUs <= now + _prepareAheadUs && clip.endUs > now;
      if (due) _open(clip);
    }
    for (final clip in active) {
      final p = _clips[clip.id];
      final c = p?.controller;
      if (p == null || c == null || !p.ready) continue;
      final want = Duration(microseconds: clip.sourceTimeAt(now));
      if (!p.running) {
        p.running = true;
        unawaited(_start(p, want));
      } else if ((c.value.position - want).inMicroseconds.abs() > _driftUs * clip.speed &&
          c.value.isPlaying) {
        unawaited(c.seekTo(want));
      }
      final volume = linearGain(clip.gainDb, muted: clip.muted);
      if ((c.value.volume - volume).abs() > 0.01) unawaited(c.setVolume(volume));
    }
    // Clips that have ended are stopped, and let go once well past.
    for (final entry in _clips.entries.toList()) {
      final p = entry.value;
      if (!activeIds.contains(entry.key) && p.running) {
        p.running = false;
        unawaited(p.controller?.pause());
      }
      final clip = _state.clip(entry.key);
      if (clip == null || clip.endUs < now - 1000000 || clip.startUs > now + 2 * _prepareAheadUs) {
        _clips.remove(entry.key);
        unawaited(p.dispose());
      }
    }
    _music(now);
    notifyListeners();
  }

  Future<void> _start(_ClipPlayer p, Duration at) async {
    final c = p.controller!;
    try {
      await c.setPlaybackSpeed(p.clip.speed.clamp(0.25, 4.0));
    } catch (_) {
      // Some platforms refuse some speeds; the clip plays at its own.
    }
    await c.seekTo(at);
    if (p.running && playing) await c.play();
  }

  void _open(EditClip clip) {
    final existing = _clips[clip.id];
    if (existing != null) {
      existing.clip = clip;
      return;
    }
    final source = clip.sourceId == null ? null : _state.sources[clip.sourceId];
    if (!clip.isClip || source == null || source.isImage || !source.exists) {
      _clips[clip.id] = _ClipPlayer(clip, null)..ready = true;
      return;
    }
    final controller = VideoPlayerController.file(
      File(source.path),
      videoPlayerOptions: VideoPlayerOptions(mixWithOthers: true),
    );
    final p = _ClipPlayer(clip, controller);
    _clips[clip.id] = p;
    p.opening = () async {
      try {
        await controller.initialize();
        await controller.setVolume(0);
        await controller.seekTo(Duration(microseconds: clip.inUs));
        p.ready = true;
      } catch (_) {
        p.failed = true;
      }
      if (!_disposed) notifyListeners();
    }();
  }

  void _music(int now) {
    for (final sound in _state.sounds) {
      final path = sound.path;
      final active = sound.startUs <= now && now < sound.endUs;
      var p = _sounds[sound.id];
      if (path == null || path.isEmpty) continue;
      if (p == null && sound.startUs <= now + _prepareAheadUs && sound.endUs > now) {
        final controller = VideoPlayerController.file(
          File(path),
          videoPlayerOptions: VideoPlayerOptions(mixWithOthers: true),
        );
        final created = _SoundPlayer(sound, controller);
        _sounds[sound.id] = created;
        () async {
          try {
            await controller.initialize();
            await controller.setVolume(0);
            created.ready = true;
          } catch (_) {
            created.failed = true;
          }
        }();
        p = created;
      }
      if (p == null || !p.ready) continue;
      p.sound = sound;
      final want = Duration(microseconds: sound.assetInUs + (now - sound.startUs));
      if (active && !p.running) {
        p.running = true;
        final starting = p;
        unawaited(() async {
          await starting.controller.seekTo(want);
          if (starting.running && playing) await starting.controller.play();
        }());
      } else if (!active && p.running) {
        p.running = false;
        unawaited(p.controller.pause());
      } else if (active &&
          p.controller.value.isPlaying &&
          (p.controller.value.position - want).inMicroseconds.abs() > _driftUs) {
        unawaited(p.controller.seekTo(want));
      }
      if (active) {
        // The fades, by the volume, on the piece's clock.
        var level = linearGain(sound.gainDb, muted: sound.muted);
        if (sound.fadeInUs > 0) {
          level *= ((now - sound.startUs) / sound.fadeInUs).clamp(0.0, 1.0);
        }
        if (sound.fadeOutUs > 0) {
          level *= ((sound.endUs - now) / sound.fadeOutUs).clamp(0.0, 1.0);
        }
        if ((p.controller.value.volume - level).abs() > 0.01) {
          unawaited(p.controller.setVolume(level));
        }
      }
    }
    for (final id in _sounds.keys.toList()) {
      final sound = _state.sound(id);
      if (sound == null || sound.endUs < now - 1000000) {
        final p = _sounds.remove(id)!;
        unawaited(p.controller.dispose());
      }
    }
  }

  /// The clip's player, once it can show a picture.
  VideoPlayerController? controllerFor(String slotId) {
    final p = _clips[slotId];
    return p != null && p.ready && !p.failed ? p.controller : null;
  }

  Future<void> _releaseAll() async {
    final clips = _clips.values.toList();
    final sounds = _sounds.values.toList();
    _clips.clear();
    _sounds.clear();
    for (final p in clips) {
      await p.dispose();
    }
    for (final s in sounds) {
      await s.controller.dispose();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _ticker?.cancel();
    _ticker = null;
    unawaited(_releaseAll());
    super.dispose();
  }
}

/// The picture while the cut plays: each clip on screen by the renderer's
/// rules, blended through its transition, the text over the top.
class LivePicture extends StatelessWidget {
  const LivePicture({super.key, required this.player, required this.atUs});

  final EditPlayer player;
  final int atUs;

  @override
  Widget build(BuildContext context) {
    final state = player.state;
    return LayoutBuilder(builder: (context, box) {
      final frame = Size(box.maxWidth, box.maxHeight);
      final active = state.clipsAt(atUs);
      final layers = <Widget>[const Positioned.fill(child: ColoredBox(color: Colors.black))];
      for (var i = 0; i < active.length; i++) {
        final clip = active[i];
        var opacity = 1.0;
        CustomClipper<Rect>? wipe;
        if (i == active.length - 1 && active.length > 1) {
          final join = state.joinAfter(active[i - 1].id);
          final kind = join?.kind ?? TransitionKind.cut;
          final p = transitionProgress(clip, atUs);
          opacity = blendOpacities(kind, p).$2;
          if (kind == TransitionKind.wipeLeft || kind == TransitionKind.wipeRight) {
            wipe = _Wipe(fromRight: kind == TransitionKind.wipeLeft, progress: p);
          }
        } else if (active.length > 1 && i == 0) {
          final join = state.joinAfter(clip.id);
          final p = transitionProgress(active.last, atUs);
          opacity = blendOpacities(join?.kind ?? TransitionKind.cut, p).$1;
        }
        Widget layer = _clipLayer(clip, state, frame);
        if (wipe != null) layer = ClipRect(clipper: wipe, child: layer);
        if (opacity < 0.999) layer = Opacity(opacity: opacity.clamp(0.0, 1.0), child: layer);
        layers.add(Positioned.fill(child: layer));
      }
      for (final text in state.texts) {
        if (text.startUs <= atUs && atUs < text.endUs) {
          layers.add(Positioned.fill(child: TextOverlay(text: text, frame: frame)));
        }
      }
      return ClipRect(child: Stack(children: layers));
    });
  }

  Widget _clipLayer(EditClip clip, EditState state, Size frame) {
    final source = clip.sourceId == null ? null : state.sources[clip.sourceId];
    if (!clip.isClip || source == null) return const ColoredBox(color: Colors.black);
    final pictureSize = Size(source.width.toDouble(), source.height.toDouble());
    final Widget picture;
    if (source.isImage) {
      picture = Image.file(File(source.path), fit: BoxFit.fill, gaplessPlayback: true);
    } else {
      final controller = player.controllerFor(clip.id);
      if (controller == null) return const ColoredBox(color: Colors.black);
      picture = VideoPlayer(controller);
    }
    final adjusted = clip.adjustment.isNeutral
        ? picture
        : ColorFiltered(colorFilter: ColorFilter.matrix(adjustMatrix(clip.adjustment)), child: picture);
    final placed = placePicture(pictureSize, frame, clip.placement);
    final children = <Widget>[];
    if (!covers(pictureSize, frame, clip.placement)) {
      final backdrop = clip.backdrop ?? Backdrop.black;
      if (backdrop.kind == 'blur') {
        children.add(Positioned.fill(
          child: ImageFiltered(
            imageFilter: ui.ImageFilter.blur(sigmaX: 24, sigmaY: 24, tileMode: TileMode.clamp),
            child: FittedBox(
              fit: BoxFit.cover,
              clipBehavior: Clip.hardEdge,
              child: SizedBox(width: pictureSize.width, height: pictureSize.height, child: adjusted),
            ),
          ),
        ));
      } else if (backdrop.kind == 'color') {
        children.add(Positioned.fill(
          child: ColoredBox(color: parseColor(backdrop.color)?.withValues(alpha: 1) ?? Colors.black),
        ));
      }
    }
    children.add(Positioned(
      left: placed.centre.dx - placed.size.width / 2,
      top: placed.centre.dy - placed.size.height / 2,
      width: placed.size.width,
      height: placed.size.height,
      child: Transform.rotate(angle: placed.radians, child: adjusted),
    ));
    return Stack(children: children);
  }
}

class _Wipe extends CustomClipper<Rect> {
  _Wipe({required this.fromRight, required this.progress});
  final bool fromRight;
  final double progress;

  @override
  Rect getClip(Size size) => fromRight
      ? Rect.fromLTRB(size.width * (1 - progress), 0, size.width, size.height)
      : Rect.fromLTRB(0, 0, size.width * progress, size.height);

  @override
  bool shouldReclip(_Wipe old) => old.progress != progress || old.fromRight != fromRight;
}

/// A text over the picture, sized and placed as the renderer does it.
class TextOverlay extends StatelessWidget {
  const TextOverlay({
    super.key,
    required this.text,
    required this.frame,
    this.selected = false,
    this.ghost = false,
    this.boxKey,
  });

  final EditText text;
  final Size frame;
  final bool selected;

  /// Only the text's outline, laid out exactly as the text is: drawn over
  /// the engine's frame, which already holds the text itself.
  final bool ghost;

  /// Lets the editor find where the text's box landed.
  final GlobalKey? boxKey;

  @override
  Widget build(BuildContext context) {
    final look = text.style;
    final ppp = pixelsPerPoint(frame);
    final size = look.fontSizePt * ppp;
    final align = look.alignment.endsWith('Left')
        ? TextAlign.left
        : look.alignment.endsWith('Right')
            ? TextAlign.right
            : TextAlign.center;
    final weight = look.bold ? FontWeight.w700 : FontWeight.w500;
    final fill = ghost ? Colors.transparent : parseColor(look.color) ?? Colors.white;
    final outline =
        !ghost && look.outlineWidth > 0 ? parseColor(look.outlineColor) ?? Colors.black : null;
    final base = TextStyle(fontSize: size, height: 1.25, fontWeight: weight, color: fill);
    Widget label = Text(text.content, textAlign: align, style: base);
    if (outline != null) {
      label = Stack(children: [
        Text(
          text.content,
          textAlign: align,
          style: base.copyWith(
            foreground: Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = look.outlineWidth * ppp * 2
              ..strokeJoin = StrokeJoin.round
              ..color = outline,
            color: null,
          ),
        ),
        label,
      ]);
    }
    final background = ghost ? null : parseColor(look.backgroundColor);
    if (background != null || (ghost && look.backgroundColor != null)) {
      label = DecoratedBox(
        decoration: BoxDecoration(
          color: background ?? Colors.transparent,
          borderRadius: BorderRadius.circular(size * 0.18),
        ),
        child: Padding(
          padding: EdgeInsets.symmetric(horizontal: size * 0.3, vertical: size * 0.12),
          child: label,
        ),
      );
    }
    if (selected || ghost) {
      label = DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(color: Colors.white, width: 1.5),
          boxShadow: const [BoxShadow(color: Colors.black54, blurRadius: 2)],
        ),
        child: label,
      );
    }
    return CustomSingleChildLayout(
      delegate: _TextPlace(look, frame),
      child: ConstrainedBox(
        key: boxKey,
        constraints: BoxConstraints(maxWidth: frame.width * 0.9),
        child: label,
      ),
    );
  }
}

class _TextPlace extends SingleChildLayoutDelegate {
  _TextPlace(this.look, this.frame);
  final TextLook look;
  final Size frame;

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) => constraints.loosen();

  @override
  Offset getPositionForChild(Size size, Size childSize) => placeText(look, childSize, size);

  @override
  bool shouldRelayout(_TextPlace old) => old.look != look || old.frame != frame;
}

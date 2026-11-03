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

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:video_player/video_player.dart';

import '../l10n/strings.dart';
import '../models.dart';

/// Whether this platform's player can play a local file: video_player's
/// own on Android, iOS and the Mac, video_player_win's Media Foundation
/// player on Windows. The web build cannot open a local file and Linux has
/// no player, so neither offers playback; the editor steps through the
/// engine's frames there instead.
bool get canPlayLocalVideo =>
    !kIsWeb &&
    (Platform.isAndroid || Platform.isIOS || Platform.isMacOS || Platform.isWindows);

/// A clip played in the footage picker, so it can be judged before it is
/// chosen, with the choice one tap away.
///
/// Plays the file where it is, through the platform's own player. Where
/// there is none ([canPlayLocalVideo]) it is never offered.
class VideoPreview extends StatefulWidget {
  const VideoPreview({
    super.key,
    required this.path,
    required this.chosen,
    required this.onToggle,
  });

  final String path;
  final bool chosen;
  final VoidCallback onToggle;

  static bool get available => canPlayLocalVideo;

  /// Show [path] playing; `onToggle` chooses or unchooses it.
  static Future<void> show(
    BuildContext context, {
    required String path,
    required bool chosen,
    required VoidCallback onToggle,
  }) {
    return showDialog<void>(
      context: context,
      builder: (_) => VideoPreview(path: path, chosen: chosen, onToggle: onToggle),
    );
  }

  @override
  State<VideoPreview> createState() => _VideoPreviewState();
}

/// "1:05", short enough to sit beside the buttons on a phone.
String _clock(Duration d) =>
    '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';

class _VideoPreviewState extends State<VideoPreview> {
  late final VideoPlayerController _controller =
      VideoPlayerController.file(File(widget.path));
  late bool _chosen = widget.chosen;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_changed);
    _controller.initialize().then((_) {
      if (!mounted) return;
      setState(() {});
      _controller.play();
    }).catchError((Object _) {
      if (mounted) setState(() => _failed = true);
    });
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    _controller.removeListener(_changed);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    final value = _controller.value;
    final ready = value.isInitialized;
    return Dialog(
      clipBehavior: Clip.antiAlias,
      insetPadding: const EdgeInsets.all(16),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ColoredBox(
              color: Colors.black,
              child: ConstrainedBox(
                constraints: BoxConstraints(
                  maxHeight: MediaQuery.of(context).size.height * 0.6,
                ),
                child: _failed
                    ? Padding(
                        padding: const EdgeInsets.all(32),
                        child: Text(
                          s.previewFailed,
                          textAlign: TextAlign.center,
                          style: const TextStyle(color: Colors.white70),
                        ),
                      )
                    : !ready
                        ? const SizedBox(
                            height: 200,
                            child: Center(child: CircularProgressIndicator()),
                          )
                        // As tall as the clip at this width, not as tall as
                        // the room: a landscape clip sat in a black box
                        // three times its height.
                        : Center(
                            heightFactor: 1,
                            child: AspectRatio(
                              aspectRatio: value.aspectRatio,
                              child: GestureDetector(
                                onTap: () => value.isPlaying
                                    ? _controller.pause()
                                    : _controller.play(),
                                child: VideoPlayer(_controller),
                              ),
                            ),
                          ),
              ),
            ),
            if (ready)
              VideoProgressIndicator(
                _controller,
                allowScrubbing: true,
                padding: const EdgeInsets.symmetric(vertical: 6),
              ),
            // Two rows: on a phone one row squeezed the clock to "0:03 / 0:…".
            Padding(
              padding: const EdgeInsets.fromLTRB(8, 0, 12, 0),
              child: Row(
                children: [
                  IconButton(
                    key: const Key('preview-play'),
                    onPressed: !ready
                        ? null
                        : () => value.isPlaying ? _controller.pause() : _controller.play(),
                    icon: Icon(
                      value.isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded,
                    ),
                  ),
                  Expanded(
                    child: Text(
                      ready
                          ? '${_clock(value.position)} / ${_clock(value.duration)}'
                          : fileNameOf(widget.path),
                      style: theme.textTheme.bodyMedium,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: Text(s.close),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
              child: FilledButton.icon(
                key: const Key('preview-choose'),
                onPressed: () {
                  widget.onToggle();
                  setState(() => _chosen = !_chosen);
                },
                icon: Icon(
                  _chosen ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
                  size: 18,
                ),
                label: Text(_chosen ? s.chosenClip : s.chooseClip),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

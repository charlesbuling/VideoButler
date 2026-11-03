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

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';

/// A photo shown large in the footage picker, with the choice one tap away.
///
/// The picture comes from the engine, which turns it upright by its EXIF and
/// reads every format the analysis does -- HEIC included -- on every
/// platform, the web build among them, where the UI cannot open a file.
class PhotoPreview extends StatefulWidget {
  const PhotoPreview({
    super.key,
    required this.engine,
    required this.path,
    required this.chosen,
    required this.onToggle,
  });

  final Engine engine;
  final String path;
  final bool chosen;
  final VoidCallback onToggle;

  /// The widest picture the engine makes.
  static const int width = 640;

  static Future<void> show(
    BuildContext context, {
    required Engine engine,
    required String path,
    required bool chosen,
    required VoidCallback onToggle,
  }) {
    return showDialog<void>(
      context: context,
      builder: (_) =>
          PhotoPreview(engine: engine, path: path, chosen: chosen, onToggle: onToggle),
    );
  }

  @override
  State<PhotoPreview> createState() => _PhotoPreviewState();
}

class _PhotoPreviewState extends State<PhotoPreview> {
  late bool _chosen = widget.chosen;
  Uint8List? _picture;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    widget.engine
        .thumbnails([widget.path], width: PhotoPreview.width)
        .then((made) {
      if (!mounted) return;
      setState(() {
        _picture = made[widget.path];
        _failed = _picture == null;
      });
    }).catchError((Object _) {
      if (mounted) setState(() => _failed = true);
    });
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    final picture = _picture;
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
                child: picture != null
                    ? Image.memory(picture, fit: BoxFit.contain, key: const Key('photo-preview'))
                    : SizedBox(
                        height: 200,
                        child: Center(
                          child: _failed
                              ? const Icon(Icons.broken_image_outlined, color: Colors.white54)
                              : const CircularProgressIndicator(),
                        ),
                      ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 12, 0),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      widget.path.split(RegExp(r'[/\\]')).last,
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
                label: Text(_chosen ? s.chosenClip : s.chooseThisPhoto),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

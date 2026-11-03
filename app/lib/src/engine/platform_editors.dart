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

/// Phone editors, which the engine cannot reach alone.
///
/// A phone editor takes no draft from another app: 剪映 keeps its drafts
/// where Android lets no one else write. It does open videos shared with it
/// as a new project, in the order given and whole, so the Android host cuts
/// each clip of a cut to its span and shares them (MainActivity's
/// `handOff`). This is the Dart end of that channel; elsewhere nothing
/// handles it.
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../models.dart';

class PlatformEditors {
  PlatformEditors._();

  static const _channel = MethodChannel('videobutler/editors');

  /// Can this platform hand a cut to a phone editor?
  static bool get available => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  /// Cut the clips of [handoff] to their spans into a folder named after
  /// [folder], and open them, in order, in the app with [package].
  /// `onProgress` hears clips done of the total. Returns the folder the
  /// clips were written to.
  static Future<String?> handOff({
    required String package,
    required String folder,
    required Handoff handoff,
    void Function(int done, int total)? onProgress,
  }) async {
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'handOffProgress' && onProgress != null) {
        final args = Map<String, dynamic>.from(call.arguments as Map);
        onProgress(
          (args['done'] as num?)?.toInt() ?? 0,
          (args['total'] as num?)?.toInt() ?? handoff.clips.length,
        );
      }
    });
    try {
      final result = await _channel.invokeMapMethod<String, Object?>('handOff', {
        'package': package,
        'folder': folder,
        'clips': [
          for (final clip in handoff.clips)
            {
              'path': clip.path,
              'inUs': clip.inUs,
              'outUs': clip.outUs,
              'width': clip.width,
              'height': clip.height,
              'still': clip.still,
            },
        ],
        if (handoff.frame case final frame?)
          'frame': {'width': frame.width, 'height': frame.height},
      });
      return result?['folder'] as String?;
    } finally {
      _channel.setMethodCallHandler(null);
    }
  }
}

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

/// Models on Android, where the engine cannot reach them alone.
///
/// The small models ship inside the APK and are copied into the library on
/// launch. A model pack -- the caption models, gigabytes -- is copied from a
/// folder the user picks with the system picker, because Android lets an app
/// read media by path but not other files. Both happen in the Android host
/// (MainActivity); this is the Dart end of that channel. On every other
/// platform the engine does the same work itself (`models.import`), and
/// these calls do nothing.
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

class PlatformModels {
  PlatformModels._();

  static const _channel = MethodChannel('videobutler/models');

  /// Does the host bring models in itself?
  static bool get handledByHost =>
      !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  /// Put the bundled models where the engine looks, before anything asks
  /// for them. Quick after the first launch.
  static Future<void> prepare() async {
    if (!handledByHost) return;
    try {
      await _channel.invokeMethod<int>('prepareBundled');
    } on PlatformException {
      // The self-test then reports the models as missing, which is true.
    } on MissingPluginException {
      // A test host with no Android side.
    }
  }

  /// Ask for a folder and copy the named files out of it into the library.
  /// `skip` names files never copied -- models this device cannot run --
  /// which come back under `tooBig` when the folder has them. `onProgress`
  /// hears bytes done of the total. Returns what the host did, or null when
  /// the user backed out.
  static Future<Map<String, dynamic>?> importFolder(
    List<({String name, int bytes})> files, {
    List<List<({String name, int bytes})>> prefer = const [],
    List<String> skip = const [],
    void Function(String name, int done, int total)? onProgress,
  }) async {
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'importProgress' && onProgress != null) {
        final args = Map<String, dynamic>.from(call.arguments as Map);
        onProgress(
          args['name'] as String? ?? '',
          (args['done'] as num?)?.toInt() ?? 0,
          (args['total'] as num?)?.toInt() ?? 1,
        );
      }
    });
    try {
      final result =
          await _channel.invokeMethod<Map<Object?, Object?>>('importFolder', {
        'files': [
          for (final f in files) {'name': f.name, 'bytes': f.bytes},
        ],
        'prefer': [
          for (final group in prefer)
            [
              for (final f in group) {'name': f.name, 'bytes': f.bytes},
            ],
        ],
        'skip': skip,
      });
      if (result == null || result['cancelled'] == true) return null;
      return result.map((k, v) => MapEntry(k.toString(), v));
    } finally {
      _channel.setMethodCallHandler(null);
    }
  }
}

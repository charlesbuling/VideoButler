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
import 'package:image_picker/image_picker.dart';

/// Videos and photos from the phone's photo library, on iOS.
///
/// Everywhere else the engine's own picker reads the footage where it is.
/// An iOS app sees only its own sandbox, so there the library's picker hands
/// over copies, which are kept in the app's Documents -- where the Files app
/// shows them too -- and the engine analyses those like any other file.
class PlatformPhotos {
  PlatformPhotos._();

  static bool get available => !kIsWeb && Platform.isIOS;

  /// Where imports are kept: the Documents folder of the app's container.
  static Directory get _imported => Directory(
      '${Platform.environment['HOME'] ?? Directory.systemTemp.path}/Documents/Imported');

  /// Ask for videos and photos; the paths of their copies, or none when
  /// the user chose nothing.
  static Future<List<String>> importFromLibrary() async {
    // Full metadata: the capture time and the orientation are in it, and a
    // cut is ordered and grouped by the one and shown by the other.
    final picked = await ImagePicker().pickMultipleMedia(requestFullMetadata: true);
    if (picked.isEmpty) return const [];
    final dir = _imported;
    await dir.create(recursive: true);
    final paths = <String>[];
    for (final file in picked) {
      final name = file.name.isEmpty ? file.path.split('/').last : file.name;
      var dest = File('${dir.path}/$name');
      var n = 2;
      while (await dest.exists()) {
        final dot = name.lastIndexOf('.');
        final stem = dot < 0 ? name : name.substring(0, dot);
        final ext = dot < 0 ? '' : name.substring(dot);
        dest = File('${dir.path}/$stem-$n$ext');
        n++;
      }
      // Moved when the picker's copy is on the same volume, which it is:
      // both live in the app's container.
      try {
        await File(file.path).rename(dest.path);
      } on FileSystemException {
        await File(file.path).copy(dest.path);
      }
      paths.add(dest.path);
    }
    return paths;
  }
}

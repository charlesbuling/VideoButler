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

/// The video butler on an Android device, with nothing but the APK.
///
/// Hands the butler a folder of videos and photos the way a file server
/// would -- the list of files itself, not a walk -- and checks that each
/// kind is filed and that the faces in them were grouped by AuraFace, which
/// the APK now carries, rather than left ungrouped.
///
///   adb -s <device> push <videos and photos> /sdcard/Movies/butler-test/
///   adb -s <device> shell pm grant dev.videobutler.video_butler android.permission.READ_MEDIA_VIDEO
///   adb -s <device> shell pm grant dev.videobutler.video_butler android.permission.READ_MEDIA_IMAGES
///   flutter test integration_test/android_butler_test.dart -d <device> --no-uninstall \
///     --dart-define=VB_IT_FOOTAGE=/sdcard/Movies/butler-test
///
/// It writes into the app's own storage and nowhere else.
library;

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:video_butler/src/engine/engine_factory.dart';
import 'package:video_butler/src/engine/platform_models.dart';

const footage = String.fromEnvironment('VB_IT_FOOTAGE');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('the butler files videos and photos and groups their faces', (tester) async {
    expect(footage, isNotEmpty, reason: 'pass --dart-define=VB_IT_FOOTAGE=<folder>');
    await PlatformModels.prepare();
    final engine = createEngine();

    final files = [
      for (final f in Directory(footage).listSync())
        if (f is File) f.path,
    ]..sort();
    // ignore: avoid_print
    print('handing over ${files.length} files from $footage');
    expect(files, isNotEmpty, reason: 'nothing readable in $footage: granted?');

    final started = await engine.call('hosting.start', {
      'generate': false,
      'roots': [footage],
      'files': files,
    }) as Map<String, dynamic>;
    final job = started['jobId'] as String;
    final clock = Stopwatch()..start();
    var last = '';
    while (true) {
      await Future<void>.delayed(const Duration(seconds: 2));
      final status =
          await engine.call('run.status', {'jobId': job}) as Map<String, dynamic>;
      final line = '${status['percent']}% ${status['step']} ${status['file'] ?? ''}';
      if (line != last) {
        // ignore: avoid_print
        print('[${clock.elapsed.inSeconds}s] $line');
        last = line;
      }
      if (status['finished'] == true) {
        expect(status['error'], isNull, reason: '${status['error']}');
        break;
      }
    }

    final hosting = await engine.call('hosting.get') as Map<String, dynamic>;
    // ignore: avoid_print
    print('labeller ${hosting['labeller']}, tracker ${hosting['tracker']}, '
        '${hosting['videos']} videos, ${hosting['photos']} photos, '
        '${hosting['unreadable']} unreadable, in ${clock.elapsed.inSeconds} s');
    for (final p in hosting['people'] as List<dynamic>) {
      // ignore: avoid_print
      print('person ${p['id']}: ${p['videos']} videos, ${p['photos']} photos');
    }
    for (final c in hosting['categories'] as List<dynamic>) {
      // ignore: avoid_print
      print('category ${c['id']}: ${c['videos']} videos, ${c['photos']} photos');
    }

    // Every face filed, listed or not: someone seen only in one day's
    // photos is one sighting and not listed among the people, but is in
    // the album all the same.
    final album = await engine.call('entities.list') as Map<String, dynamic>;
    final sources = <String, Set<String>>{};
    for (final subject in album['subjects'] as List<dynamic>) {
      for (final a in subject['appearances'] as List<dynamic>) {
        final path = a['sourcePath'] as String;
        (sources[path.toLowerCase().endsWith('.jpg') ? 'photo' : 'video'] ??= {})
            .add(path);
      }
    }
    // ignore: avoid_print
    print('album: ${(album['subjects'] as List).length} subjects, faces filed from '
        '${sources['video']?.length ?? 0} videos and ${sources['photo']?.length ?? 0} photos');

    expect(hosting['tracker'], 'AuraFace', reason: 'the APK carries the face model');
    expect(hosting['labeller'], isNotNull);
    expect(hosting['videos'], greaterThan(0));
    expect(hosting['photos'], greaterThan(0));
    final people = hosting['people'] as List<dynamic>;
    expect(people, isNotEmpty, reason: 'faces were grouped into people');
    expect(sources['photo'], isNotEmpty, reason: 'faces in photos are filed too');
  }, timeout: const Timeout(Duration(minutes: 30)));
}

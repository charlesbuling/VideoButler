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

/// The engine on an Android device, with the model host, end to end.
///
/// Drives the real engine through the same calls the app makes, without
/// tapping through the UI: the self-test, the model library, and a run over
/// footage already on the device, printing which models ran and how long
/// each stage took.
///
///   adb push clip.mp4 /sdcard/Movies/test/
///   flutter test integration_test/android_engine_test.dart -d <device> \
///     --dart-define=VB_IT_FOOTAGE=/sdcard/Movies/test \
///     --dart-define=VB_IT_TOPIC=dance \
///     [--dart-define=VB_IT_HANDOFF=com.lemon.lv]
///
/// It writes into the app's own storage and nowhere else -- unless asked to
/// hand the cut to a phone editor, which writes the cut clips to
/// Movies/VideoButler and opens the editor.
library;

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:video_butler/src/engine/engine_factory.dart';
import 'package:video_butler/src/engine/platform_editors.dart';
import 'package:video_butler/src/engine/platform_models.dart';
import 'package:video_butler/src/models.dart';

const footage = String.fromEnvironment('VB_IT_FOOTAGE');
const topic = String.fromEnvironment('VB_IT_TOPIC', defaultValue: 'dance');

/// A phone editor's package to hand the cut to afterwards, if any.
const handOffTo = String.fromEnvironment('VB_IT_HANDOFF');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('the engine and its models run on this device', (tester) async {
    expect(footage, isNotEmpty,
        reason: 'pass --dart-define=VB_IT_FOOTAGE=<folder>');
    await PlatformModels.prepare();
    final engine = createEngine();

    final selftest =
        await engine.call('selftest', {'lang': 'en'}) as Map<String, dynamic>;
    for (final check in selftest['checks'] as List<dynamic>) {
      // ignore: avoid_print
      print('check ${check['id']}: ${check['status']} -- ${check['detail']}');
    }
    for (final a in selftest['adapters'] as List<dynamic>) {
      // ignore: avoid_print
      print('adapter ${a['role']}: ${a['name']} mock=${a['mock']}');
    }
    final models = await engine.call('models.list') as Map<String, dynamic>;
    for (final m in models['models'] as List<dynamic>) {
      if (m['installed'] == true) {
        // ignore: avoid_print
        print('model ${m['id']} installed, fits=${m['fits']}, inUse=${m['inUse']}, '
            'bundled=${m['bundled']} at ${m['path']}');
      }
    }

    final started = await engine.call('run.start', {
      'inputs': [footage],
      'style': {'topic': topic, 'beatSync': true, 'targetDurationUs': 20000000},
      'title': 'Android test',
    }) as Map<String, dynamic>;
    final job = started['jobId'] as String;
    final clock = Stopwatch()..start();
    Map<String, dynamic> status;
    var last = '';
    while (true) {
      await Future<void>.delayed(const Duration(seconds: 2));
      status = await engine.call('run.status', {'jobId': job})
          as Map<String, dynamic>;
      final line =
          '${status['stage']} ${status['percent']}% ${status['step']} ${status['file'] ?? ''}';
      if (line != last) {
        // ignore: avoid_print
        print('[${clock.elapsed.inSeconds}s] $line');
        last = line;
      }
      if (status['finished'] == true) break;
    }
    expect(status['state'], 'done', reason: '${status['error']}');
    final result = status['result'] as Map<String, dynamic>;
    // ignore: avoid_print
    print('done in ${clock.elapsed.inSeconds}s: ${result['durationUs']} us, '
        '${(result['slots'] as List).length} clips');
    for (final note in result['adapterNotes'] as List<dynamic>) {
      // ignore: avoid_print
      print('note: $note');
    }
    for (final w in result['warnings'] as List<dynamic>) {
      // ignore: avoid_print
      print('warning: $w');
    }
    for (final slot in result['slots'] as List<dynamic>) {
      // ignore: avoid_print
      print('slot: ${slot['reason']}');
    }
    final graph = jsonDecode(
      await File(result['footageGraphPath'] as String).readAsString(),
    ) as Map<String, dynamic>;
    for (final p in graph['provenance'] as List<dynamic>) {
      // ignore: avoid_print
      print('stage ${p['stage']} by ${p['adapter']}: ${p['durationMs']} ms');
    }
    final dsl = jsonDecode(
      await File(result['editingDslPath'] as String).readAsString(),
    ) as Map<String, dynamic>;
    for (final t
        in (dsl['tracks'] as Map<String, dynamic>)['text'] as List<dynamic>? ??
            const []) {
      // ignore: avoid_print
      print('text ${t['role']}: ${t['content']}');
    }

    // Then, if asked, hand the cut to a phone editor as the app's button
    // does: each clip cut to its span, shared in order.
    if (handOffTo.isNotEmpty) {
      final handoff = RunResult.fromJson(result).handoff;
      for (final clip in handoff.clips) {
        // ignore: avoid_print
        print('hand off ${clip.path.split('/').last} '
            '${clip.inUs / 1e6}-${clip.outUs / 1e6}s');
      }
      final cutting = Stopwatch()..start();
      final folder = await PlatformEditors.handOff(
        package: handOffTo,
        folder: 'Android test',
        handoff: handoff,
        // ignore: avoid_print
        onProgress: (done, total) => print('[${cutting.elapsed.inSeconds}s] cut $done/$total'),
      );
      // ignore: avoid_print
      print('handed ${handoff.clips.length} clips to $handOffTo from $folder '
          'in ${cutting.elapsed.inSeconds}s; music ${handoff.music} from ${handoff.musicInUs}');
      expect(folder, isNotNull);
      expect(Directory(folder!).listSync().length, handoff.clips.length);
    }
  }, timeout: const Timeout(Duration(minutes: 60)));
}

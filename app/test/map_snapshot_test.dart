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

import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/src/l10n/strings.dart';
import 'package:video_butler/src/theme.dart';
import 'package:video_butler/src/ui/memory_map.dart';

import 'engine_contract_test.dart' show FakeEngine;

/// The engine's own answers for the world and the memories, read from files
/// the CLI wrote (`vb call geo.world`, `vb call memories.list`).
class _RecordedEngine extends FakeEngine {
  _RecordedEngine(this.world, this.list);

  final Map<String, dynamic> world;
  final Map<String, dynamic> list;

  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    switch (method) {
      case 'geo.world':
        return world;
      case 'memories.list':
        return list;
    }
    return super.call(method, args);
  }
}

Future<void> _font(String family, String file) async {
  final bytes = File(file).readAsBytesSync();
  await (FontLoader(family)..addFont(Future.value(ByteData.sublistView(bytes)))).load();
}

/// The memory map drawn from a real world and real memories into a picture,
/// for judging its design by eye: `VB_MAP_SNAPSHOT=<out.png>
/// VB_MAP_WORLD=<geo.world.json> VB_MAP_MEMORIES=<memories.list.json>
/// flutter test test/map_snapshot_test.dart`. `VB_MAP_FILTER=<kind>` shows
/// one kind, `VB_MAP_DARK=1` the dark theme, `VB_MAP_SIZE=390x844` a phone.
/// Skipped without them.
void main() {
  final env = Platform.environment;
  final out = env['VB_MAP_SNAPSHOT'];

  testWidgets('the memory map, drawn into a picture', (tester) async {
    await tester.runAsync(() async {
      await _font('Arial Unicode', '/System/Library/Fonts/Supplemental/Arial Unicode.ttf');
      // As the system would fall back for Chinese in a monospace readout.
      await _font('PingFang SC', '/System/Library/Fonts/Supplemental/Arial Unicode.ttf');
      await _font(monoFamily, '/System/Library/Fonts/SFNSMono.ttf');
      final flutter = File(Platform.resolvedExecutable).parent.parent.parent.parent.path;
      for (final candidate in [
        '$flutter/artifacts/material_fonts/MaterialIcons-Regular.otf',
        '/opt/homebrew/share/flutter/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf',
      ]) {
        if (File(candidate).existsSync()) {
          await _font('MaterialIcons', candidate);
          break;
        }
      }
    });
    final engine = _RecordedEngine(
      jsonDecode(File(env['VB_MAP_WORLD']!).readAsStringSync()) as Map<String, dynamic>,
      jsonDecode(File(env['VB_MAP_MEMORIES']!).readAsStringSync()) as Map<String, dynamic>,
    );
    final size = (env['VB_MAP_SIZE'] ?? '1280x800').split('x').map(double.parse).toList();
    await tester.binding.setSurfaceSize(Size(size[0], size[1]));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final boundary = GlobalKey();
    final theme = buildTheme(env['VB_MAP_DARK'] == '1' ? Brightness.dark : Brightness.light);
    await tester.pumpWidget(LanguageScope(
      language: AppLanguage.zh,
      onChanged: (_) {},
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: theme.copyWith(
          textTheme: theme.textTheme.apply(fontFamily: 'Arial Unicode'),
        ),
        home: Scaffold(
          body: RepaintBoundary(
            key: boundary,
            child: MemoryMapView(engine: engine),
          ),
        ),
      ),
    ));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 300)));
    await tester.pumpAndSettle();
    if (env['VB_MAP_FILTER'] case final kind?) {
      // The layers are open on a wide screen; on a phone, open them first.
      final row = find.byKey(Key('map-filter-$kind'));
      if (row.evaluate().isEmpty) {
        await tester.tap(find.byKey(const Key('map-filter')));
        await tester.pumpAndSettle();
      }
      await tester.tap(row.last);
      await tester.pumpAndSettle();
    }
    await tester.pump(const Duration(milliseconds: 700));
    await tester.runAsync(() async {
      final render = boundary.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      final image = await render.toImage(pixelRatio: 1.5);
      final png = await image.toByteData(format: ui.ImageByteFormat.png);
      File(out!).writeAsBytesSync(png!.buffer.asUint8List());
    });
  }, skip: out == null);
}

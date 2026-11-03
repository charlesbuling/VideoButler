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

/// The video butler and its memory map in the real macOS app, filled by the
/// real engine from whatever library it opens, photographed for a person to
/// look at.
///
/// It reads and changes nothing: no pass is started. Point the engine at a
/// library the butler has already filled -- the app reads
/// `VB_LIBRARY_DIR` -- and name where the pictures go:
///
///   VB_LIBRARY_DIR=/path/to/library flutter test \
///     integration_test/butler_screens_test.dart -d macos \
///     --dart-define=VB_IT_SHOTS=/path/to/a/folder
library;

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:video_butler/main.dart';
import 'package:video_butler/src/engine/engine_factory.dart';

const shots = String.fromEnvironment('VB_IT_SHOTS');

final _frame = GlobalKey();

Future<void> settle(WidgetTester tester, Duration time) async {
  final end = DateTime.now().add(time);
  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> photograph(WidgetTester tester, String name) async {
  if (shots.isEmpty) return;
  final boundary = _frame.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  final image = await tester.runAsync(() => boundary.toImage(pixelRatio: 2));
  final bytes = await tester.runAsync(() => image!.toByteData(format: ui.ImageByteFormat.png));
  await Directory(shots).create(recursive: true);
  await File('$shots/$name.png').writeAsBytes(bytes!.buffer.asUint8List());
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('the butler and its memory map, as the engine fills them', (tester) async {
    await tester.binding.setSurfaceSize(const Size(1440, 900));
    final engine = createEngine();
    await tester.pumpWidget(
      RepaintBoundary(key: _frame, child: VideoButlerApp(engine: engine, skipSelfTest: true)),
    );
    await settle(tester, const Duration(seconds: 3));

    final rail = find.byType(NavigationRail);
    await tester.tap(find.descendant(of: rail, matching: find.text('视频管家')), warnIfMissed: false);
    await settle(tester, const Duration(seconds: 6));
    expect(find.text('智能分类'), findsOneWidget);
    await photograph(tester, '1-categories');

    await tester.ensureVisible(find.byKey(const Key('category-scenery')));
    await settle(tester, const Duration(seconds: 1));
    await tester.tap(find.byKey(const Key('category-scenery')));
    await settle(tester, const Duration(seconds: 6));
    await photograph(tester, '2-scenery');
    await tester.tap(find.byType(BackButton));
    await settle(tester, const Duration(seconds: 2));

    await tester.tap(find.byKey(const Key('tab-memory-map')));
    await settle(tester, const Duration(seconds: 6));
    expect(find.byKey(const Key('map-filter')), findsOneWidget);
    await photograph(tester, '3-map');

    // One kind: the map flies to where its memories are.
    Future<void> only(String kind, String shot) async {
      await tester.tap(find.byKey(const Key('map-filter')));
      await settle(tester, const Duration(seconds: 1));
      await tester.tap(find.byKey(Key('map-filter-$kind')).last);
      await settle(tester, const Duration(seconds: 2));
      await photograph(tester, shot);
    }

    await only('annual', '4-map-years');
    await only('group', '5-map-groups');
    await only('travel', '5-map-trips');

    final card = find.byWidgetPredicate(
      (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('memory-card-travel'),
    );
    if (card.evaluate().isNotEmpty) {
      await tester.tap(card.first);
      await settle(tester, const Duration(seconds: 6));
      await photograph(tester, '6-trip');
    }
  });
}

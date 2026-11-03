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

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/main.dart';
import 'package:video_butler/src/api_models.dart';
import 'package:video_butler/src/l10n/strings.dart';
import 'package:video_butler/src/theme.dart';
import 'package:video_butler/src/ui/butler_page.dart';
import 'package:video_butler/src/ui/memory_page.dart';
import 'package:video_butler/src/ui/photo_preview.dart';
import 'package:video_butler/src/ui/project_page.dart';

import 'engine_contract_test.dart' show FakeEngine;

Future<void> pumpTall(WidgetTester tester, Widget child) async {
  await tester.binding.setSurfaceSize(const Size(1300, 1600));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(child);
  await tester.pumpAndSettle();
}

Widget app(Widget home) => LanguageScope(
      language: AppLanguage.zh,
      onChanged: (_) {},
      child: MaterialApp(theme: buildTheme(Brightness.light), home: home),
    );

void main() {
  testWidgets('the album is now the video butler, second in the navigation', (tester) async {
    await pumpTall(tester, VideoButlerApp(engine: FakeEngine(), skipSelfTest: true));
    final rail = find.byType(NavigationRail);
    expect(find.descendant(of: rail, matching: find.text('视频管家')), findsOneWidget);
    expect(find.descendant(of: rail, matching: find.text('人物相册')), findsNothing);

    await tester.tap(find.descendant(of: rail, matching: find.text('视频管家')), warnIfMissed: false);
    await tester.pumpAndSettle();
    expect(find.byType(ButlerPage), findsOneWidget);
    expect(find.text('分类'), findsOneWidget);
    expect(find.text('回忆地图'), findsOneWidget);
  });

  testWidgets('the categories show faces, the smart categories and say what they are',
      (tester) async {
    await pumpTall(tester, app(ButlerPage(engine: FakeEngine())));
    expect(find.text('人脸'), findsOneWidget);
    expect(find.byKey(const Key('butler-person-sub-0002')), findsOneWidget);
    expect(find.text('智能分类'), findsOneWidget);
    expect(find.byKey(const Key('category-scenery')), findsOneWidget);
    expect(find.text('136 个视频'), findsOneWidget);
    // Study's rules were set without real footage, and it says so.
    expect(find.text('1 个视频 · 规则未经实测'), findsOneWidget);
    expect(find.textContaining('Apple Vision 的场景标签按规则归类'), findsOneWidget);
    expect(find.textContaining('已整理 228 个视频'), findsOneWidget);
  });

  testWidgets('one tap hands the footage over and the pass shows where it is', (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    await tester.tap(find.byKey(const Key('hosting-start')));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(engine.calls.any((c) => c.$1 == 'hosting.start'), isTrue);
    expect(find.byKey(const Key('hosting-progress')), findsOneWidget);
    expect(find.textContaining('查看 42/230'), findsOneWidget);

    engine.hostingFinished = true;
    await tester.pump(const Duration(seconds: 1));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('hosting-progress')), findsNothing);
  });

  testWidgets('a disk plugged in is added to the folders in one tap', (tester) async {
    final engine = FakeEngine()
      ..volumes = const [
        {'name': 'Media', 'path': '/Volumes/Media'},
      ];
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    await tester.tap(find.byKey(const Key('hosting-folders')));
    await tester.pumpAndSettle();
    expect(find.text('接着的磁盘（点一下整盘加入）'), findsOneWidget);
    await tester.tap(find.byKey(const Key('hosting-add-disk-Media')));
    await tester.pumpAndSettle();
    // Added, so no longer offered.
    expect(find.text('/Volumes/Media'), findsOneWidget);
    expect(find.byKey(const Key('hosting-add-disk-Media')), findsNothing);
    await tester.tap(find.text('保存'));
    await tester.pumpAndSettle();
    final update = engine.calls.lastWhere((c) => c.$1 == 'hosting.update');
    expect(update.$2['roots'], ['/Users/me/Movies', '/Volumes/Media']);
  });

  testWidgets('a folder that cannot be read says why, before anything is filed',
      (tester) async {
    final engine = FakeEngine()
      ..hostingWarnings = const ['「Media」现在没有接上（/Volumes/Media）。之前从它整理出的视频先保留着，接上后会接着整理。'];
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    expect(find.byKey(const Key('hosting-warnings')), findsOneWidget);
    expect(find.text('有的文件夹现在读不到'), findsOneWidget);
    expect(find.textContaining('「Media」现在没有接上'), findsOneWidget);
  });

  testWidgets('a category opens its videos, and the chosen ones start a project',
      (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    await tester.tap(find.byKey(const Key('category-scenery')));
    await tester.pumpAndSettle();
    expect(find.text('风景'), findsWidgets);
    expect(find.byKey(const Key('video-/trip/V31001-181040.mp4')), findsOneWidget);
    expect(find.text('2023.10.01'), findsOneWidget);
    expect(find.text('新疆'), findsOneWidget);

    await tester.tap(find.byKey(const Key('select-all')));
    await tester.pumpAndSettle();
    expect(find.text('用这 2 个视频新建项目'), findsOneWidget);
    await tester.tap(find.byKey(const Key('grid-new-project')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('grid-project-create')));
    await tester.pumpAndSettle();

    expect(find.byType(ProjectPage), findsOneWidget);
    final update = engine.calls.lastWhere((c) => c.$1 == 'projects.update');
    expect(update.$2['inputs'], ['/trip/V31001-181040.mp4', '/trip/V31003-124026.mp4']);
  });

  testWidgets('photos are counted apart from videos, and shown large rather than played',
      (tester) async {
    final engine = FakeEngine()
      ..hostingPhotos = 395
      ..categoryPhotos = const {'scenery': 100};
    engine.catalogVideos = [
      ...engine.catalogVideos,
      {
        'path': '/trip/P31001-102838.jpg',
        'name': 'P31001-102838.jpg',
        'mediaKind': 'image',
        'capturedAt': '2023-10-01T10:28:38+08:00',
        'capturedFrom': 'camera',
        'durationUs': 0,
        'width': 4080,
        'height': 3060,
        'categories': ['scenery'],
        'subjects': <String>[],
      },
    ];
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    expect(find.textContaining('已整理 228 个视频 · 395 张照片'), findsOneWidget);
    expect(find.text('36 个视频 · 100 张照片'), findsOneWidget);

    await tester.tap(find.byKey(const Key('category-scenery')));
    await tester.pumpAndSettle();
    final photo = find.byKey(const Key('video-/trip/P31001-102838.jpg'));
    expect(photo, findsOneWidget);
    // A photo has no running time on its tile, as each video has.
    expect(find.descendant(of: photo, matching: find.text('0:00')), findsNothing);
    expect(find.text('0:20'), findsOneWidget);

    await tester.tap(photo);
    await tester.pumpAndSettle();
    expect(find.byType(PhotoPreview), findsOneWidget);
    await tester.tap(find.byKey(const Key('preview-choose')));
    await tester.pumpAndSettle();
    expect(find.text('用这 1 张照片新建项目'), findsOneWidget);
    await tester.tap(find.text('关闭'));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('select-all')));
    await tester.pumpAndSettle();
    expect(find.text('用这 2 个视频和 1 张照片新建项目'), findsOneWidget);
  });

  testWidgets('removing a category asks first, then it waits at the bottom to come back',
      (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    await tester.tap(find.byKey(const Key('category-scenery')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('remove-category')));
    await tester.pumpAndSettle();
    expect(find.text('删除「风景」分类？'), findsOneWidget);
    await tester.tap(find.byKey(const Key('remove-category-confirm')));
    await tester.pumpAndSettle();

    expect(engine.removedCategories, {'scenery'});
    expect(find.byKey(const Key('category-scenery')), findsNothing);
    expect(find.byKey(const Key('removed-scenery')), findsOneWidget);
    await tester.tap(find.byKey(const Key('removed-scenery')));
    await tester.pumpAndSettle();
    expect(engine.removedCategories, isEmpty);
  });

  testWidgets('the memory map pins each memory by its year and month, and filters by kind',
      (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(ButlerPage(engine: engine)));
    await tester.tap(find.byKey(const Key('tab-memory-map')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('pin-travel-20230929-CN-XJ')), findsOneWidget);
    expect(find.text('2023.09'), findsOneWidget);
    expect(find.byKey(const Key('pin-annual-2026')), findsOneWidget);
    // A memory with no location is listed, not pinned.
    expect(find.byKey(const Key('pin-growth-sub-0063')), findsNothing);
    expect(find.text('1 段回忆没有定位，不在地图上'), findsOneWidget);

    // The layers are open on a wide screen: one tap shows one kind.
    expect(find.byKey(const Key('map-filter-all')), findsOneWidget);
    await tester.tap(find.byKey(const Key('map-filter-annual')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('pin-annual-2026')), findsOneWidget);
    expect(find.byKey(const Key('pin-travel-20230929-CN-XJ')), findsNothing);
    expect(find.byKey(const Key('memory-card-annual-2026')), findsOneWidget);
    expect(find.byKey(const Key('memory-card-travel-20230929-CN-XJ')), findsNothing);
  });

  testWidgets('a year can be cut by time or by kind', (tester) async {
    final engine = FakeEngine();
    await pumpTall(
      tester,
      app(MemoryPage(engine: engine, memoryId: 'annual-2026', title: '2026年度回忆')),
    );
    expect(find.text('等待生成视频'), findsOneWidget);
    expect(find.text('按类别：人物→风景→食物→其他'), findsOneWidget);
    await tester.tap(find.text('按时间'));
    await tester.pumpAndSettle();
    final update = engine.calls.lastWhere((c) => c.$1 == 'memories.update');
    expect(update.$2, {'id': 'annual-2026', 'order': 'time'});
  });

  test('a memory and the map parse as the engine sends them', () async {
    final engine = FakeEngine();
    final list = MemoryList.fromJson(await engine.call('memories.list') as Map<String, dynamic>);
    expect(list.memories.first.kind, MemoryKind.travel);
    expect(list.memories.first.pin!.lon, 84.0);
    final world = WorldMap.fromJson(await engine.call('geo.world') as Map<String, dynamic>);
    expect(world.regions.last.province, isTrue);
    expect(world.regions.last.rings.first.first, 74.0);
    expect(world.regions.last.label.lat, 42.0);
  });
}

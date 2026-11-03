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
import 'package:video_butler/src/api_models.dart';
import 'package:video_butler/src/engine/engine.dart';
import 'package:video_butler/src/l10n/strings.dart';
import 'package:video_butler/src/models.dart';
import 'package:video_butler/src/theme.dart';
import 'package:video_butler/src/ui/file_browser.dart';
import 'package:video_butler/src/ui/home_page.dart';
import 'package:video_butler/src/ui/result_view.dart';

import 'engine_contract_test.dart' show FakeEngine;

/// These tests pin the English wording; flow_test.dart covers Chinese, the
/// default. Both languages render every screen, so both are held to it.
Widget english(Widget app) =>
    LanguageScope(language: AppLanguage.en, onChanged: (_) {}, child: app);

Widget wrap(Widget child) => english(MaterialApp(
      theme: buildTheme(Brightness.light),
      home: Scaffold(body: SingleChildScrollView(child: child)),
    ));

RunResult sampleResult({
  int rejected = 0,
  List<String> warnings = const [],
  Handoff handoff = const Handoff(),
}) {
  return RunResult(
    handoff: handoff,
    draftPath: '/tmp/p/sample.otio',
    footageGraphPath: '/tmp/p/sample.footage-graph.json',
    editingDslPath: '/tmp/p/sample.editing-dsl.json',
    projectDir: '/tmp/p',
    templateId: 'rhythm.calm-observational',
    durationUs: 24700000,
    shotsDetected: 4,
    clipsUsed: 3,
    verified: 4,
    rejected: rejected,
    rejections: rejected == 0
        ? const []
        : const [
            Rejection(
              slotId: 'slot-shot-0003',
              reason: 'shotId "shot-0099" does not exist in the footage graph',
            ),
          ],
    reusedSources: 0,
    totalSources: 1,
    warnings: warnings,
    shots: const [
      DetectedShot(
        id: 'shot-0000',
        startUs: 0,
        endUs: 6000000,
        durationUs: 6000000,
        score: 0.901,
        keyframes: 2,
        used: true,
        description: null,
      ),
      DetectedShot(
        id: 'shot-0001',
        startUs: 6000000,
        endUs: 11000000,
        durationUs: 5000000,
        score: 0.349,
        keyframes: 2,
        used: false,
        description: null,
      ),
    ],
    slots: const [
      CutSlot(
        id: 'slot-title',
        kind: 'titlecard',
        durationUs: 2500000,
        timelineStartUs: 0,
        shotId: null,
        reason: 'Template opens on a title card.',
        score: null,
      ),
      CutSlot(
        id: 'slot-shot-0000',
        kind: 'clip',
        durationUs: 6000000,
        timelineStartUs: 2500000,
        shotId: 'shot-0000',
        reason: 'Selected on classical score 0.90.',
        score: 0.901,
      ),
    ],
  );
}

void main() {
  group('ResultView', () {
    testWidgets('shows the headline numbers', (tester) async {
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.text('00:24.700'), findsOneWidget);
      expect(find.text('rhythm.calm-observational'), findsOneWidget);
      expect(find.text('Every shot detected'), findsOneWidget);
    });

    testWidgets('shows why each clip was chosen', (tester) async {
      // A cut the user cannot interrogate is a black box, so the reason has
      // to reach the screen.
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.text('Selected on classical score 0.90.'), findsOneWidget);
      expect(find.text('Title card'), findsOneWidget);
    });

    testWidgets('lists shots that were left out alongside the ones used',
        (tester) async {
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.text('shot-0000'), findsWidgets);
      expect(find.text('shot-0001'), findsOneWidget);
    });

    testWidgets('says plainly when mock adapters produced the result',
        (tester) async {
      // This warning is the difference between a draft and a claim, and it
      // must not be possible to lose it in a refactor.
      await tester.pumpWidget(wrap(ResultView(
        result: sampleResult(
          warnings: const ['MockAsr produced no real output'],
        ),
      )));
      expect(
        find.text('This is a rough structure, not a finished cut'),
        findsOneWidget,
      );
      expect(find.text('MockAsr produced no real output'), findsOneWidget);
    });

    testWidgets(
        'on a phone the cut goes to the editor as clips, and what stays behind is said first',
        (tester) async {
      EditorInfo? handed;
      const jianying = EditorInfo(
        id: 'jianying',
        name: '剪映',
        installed: true,
        version: '21.6.1',
        draftRoot: null,
        canExport: false,
        running: false,
        phone: true,
        appPath: 'com.lemon.lv',
      );
      await tester.pumpWidget(wrap(ResultView(
        result: sampleResult(
          handoff: const Handoff(
            clips: [
              (path: '/sdcard/DCIM/a.mp4', inUs: 1000000, outUs: 4000000, width: 1920, height: 1080, still: false),
              (path: '/sdcard/DCIM/b.mp4', inUs: 0, outUs: 2000000, width: 1080, height: 1920, still: false),
            ],
            titleCards: 1,
            texts: 2,
            music: '/sdcard/Music/song.mp3',
            musicInUs: 2500000,
          ),
        ),
        phoneEditors: const [jianying],
        onHandOff: (editor) => handed = editor,
      )));
      expect(find.text('Continue in 剪映'), findsOneWidget);
      expect(find.textContaining('these 2 clips'), findsOneWidget);
      expect(find.textContaining("home screen first"), findsOneWidget);
      expect(find.text('Left behind: 1 title card, 2 texts.'), findsOneWidget);
      expect(find.textContaining('song.mp3, from 00:02.500'), findsOneWidget);
      await tester.tap(find.byKey(const Key('hand-off-jianying')));
      expect(handed?.appPath, 'com.lemon.lv');
    });

    testWidgets('offers no phone editor where none can take the cut', (tester) async {
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.textContaining('Continue in'), findsNothing);
    });

    testWidgets('shows nothing about grounding when nothing was rejected',
        (tester) async {
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.text('Rejected by grounding'), findsNothing);
    });

    testWidgets('surfaces rejections with their reasons', (tester) async {
      await tester.pumpWidget(
        wrap(ResultView(result: sampleResult(rejected: 1))),
      );
      expect(find.text('Rejected by grounding'), findsOneWidget);
      expect(
        find.textContaining('does not exist in the footage graph'),
        findsOneWidget,
      );
    });

    testWidgets('shows the files it wrote', (tester) async {
      await tester.pumpWidget(wrap(ResultView(result: sampleResult())));
      expect(find.text('/tmp/p/sample.otio'), findsOneWidget);
      expect(find.text('/tmp/p/sample.editing-dsl.json'), findsOneWidget);
    });
  });

  group('HomePage', () {
    // The page is a ListView, which builds lazily, so the default 800x600
    // test surface leaves the lower cards unbuilt and unfindable.
    setUp(() => TestWidgetsFlutterBinding.ensureInitialized());

    Future<void> pumpTall(WidgetTester tester, Widget child) async {
      await tester.binding.setSurfaceSize(const Size(1000, 1600));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await tester.pumpWidget(child);
      await tester.pumpAndSettle();
    }

    testWidgets('loads engine state and shows it', (tester) async {
      await pumpTall(tester, english(MaterialApp(home: HomePage(engine: FakeEngine()))));

      expect(find.text('Video Butler'), findsOneWidget);
      expect(find.text('Engine'), findsOneWidget);
      expect(find.text('Source video'), findsOneWidget);
      // The empty state has to say that a project can be more than one file,
      // because nothing else on the screen does.
      expect(
        find.text(
            'Nothing chosen yet. Videos and photos both work: pick several, or a whole folder.'),
        findsOneWidget,
      );
    });

    testWidgets('marks mock adapters rather than presenting them as real',
        (tester) async {
      await pumpTall(tester, english(MaterialApp(home: HomePage(engine: FakeEngine()))));
      expect(find.text('Speech: MockAsr'), findsOneWidget);
      expect(
        find.textContaining('produce no real output yet'),
        findsOneWidget,
      );
    });

    testWidgets('will not run until a video has been chosen', (tester) async {
      final engine = FakeEngine();
      await pumpTall(tester, english(MaterialApp(home: HomePage(engine: engine))));

      final button = tester.widget<FilledButton>(find.byType(FilledButton));
      expect(
        button.onPressed,
        isNull,
        reason: 'the run button must be disabled with no source video',
      );
      expect(engine.runCalls, 0);
    });

    testWidgets(
        'the picker filters by day and by frame, and selects what shows',
        (tester) async {
      final engine = tripEngine();
      List<String>? returned;
      // A tall window, set on the view itself: the sheet takes its height
      // from the MediaQuery the view reports.
      tester.view.physicalSize = const Size(1000, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(english(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async => returned =
                  await FileBrowserSheet.show(context, engine: engine),
              child: const Text('open'),
            ),
          ),
        ),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();

      Finder clip(String name) => find.byKey(Key('video-$name'));
      expect(find.text('4 items'), findsOneWidget);
      expect(find.text('Oct 1 · 2'), findsOneWidget);
      expect(find.text('Oct 3 · 1'), findsOneWidget);
      expect(find.text('Date unknown · 1'), findsOneWidget);
      expect(find.text('16:9 landscape · 3'), findsOneWidget);
      expect(find.text('9:16 portrait · 1'), findsOneWidget);
      // Each clip says when and how it was filmed.
      expect(find.textContaining('2023-10-01 18:30 · 9:16 portrait · 0:23'),
          findsOneWidget);

      // By frame: only the portrait clip, and select-all takes only it.
      await tester.tap(find.byKey(const Key('ratio-9:16')));
      await tester.pumpAndSettle();
      expect(clip('b.mp4'), findsOneWidget);
      expect(clip('a.mp4'), findsNothing);
      expect(find.text('1 of 4 items'), findsOneWidget);
      expect(find.text('more'), findsOneWidget, reason: 'folders always show');
      await tester.tap(find.byKey(const Key('select-all')));
      await tester.pumpAndSettle();
      expect(find.text('1 video chosen'), findsOneWidget);

      // By day, the other filter cleared: both clips of the first day.
      await tester.tap(find.byKey(const Key('ratio-all')));
      await tester.tap(find.byKey(const Key('date-2023-10-01')));
      await tester.pumpAndSettle();
      expect(find.text('2 of 4 items'), findsOneWidget);
      expect(clip('c.mp4'), findsNothing);
      await tester.tap(find.byKey(const Key('select-all')));
      await tester.pumpAndSettle();
      expect(find.text('2 videos chosen'), findsOneWidget);
      expect(find.text('Deselect all'), findsOneWidget);

      // Both filters together: the first day's landscape clip only.
      await tester.tap(find.byKey(const Key('ratio-16:9')));
      await tester.pumpAndSettle();
      expect(find.text('1 of 4 items'), findsOneWidget);
      expect(clip('a.mp4'), findsOneWidget);

      await tester.tap(find.text('Use these'));
      await tester.pumpAndSettle();
      expect(returned, unorderedEquals(['/trip/a.mp4', '/trip/b.mp4']));
    });

    testWidgets(
        'a folder of videos and photos can be narrowed to either, and counts both',
        (tester) async {
      final engine = mixedEngine();
      List<String>? returned;
      tester.view.physicalSize = const Size(1000, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(english(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async => returned =
                  await FileBrowserSheet.show(context, engine: engine),
              child: const Text('open'),
            ),
          ),
        ),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();

      expect(find.text('Choose videos and photos'), findsOneWidget);
      expect(find.text('Videos · 2'), findsOneWidget);
      expect(find.text('Photos · 2'), findsOneWidget);
      // A photo says when it was taken and its frame, and has no running time.
      expect(find.textContaining('2023-10-01 18:31 · 3:4 portrait · 4.0 MB'), findsOneWidget);

      await tester.tap(find.byKey(const Key('kind-image')));
      await tester.pumpAndSettle();
      expect(find.text('2 of 4 items'), findsOneWidget);
      expect(find.byKey(const Key('video-a.mp4')), findsNothing);
      await tester.tap(find.byKey(const Key('select-all')));
      await tester.pumpAndSettle();
      expect(find.text('2 photos chosen'), findsOneWidget);

      await tester.tap(find.byKey(const Key('kind-all')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('video-a.mp4')));
      await tester.pumpAndSettle();
      expect(find.text('1 video and 2 photos chosen'), findsOneWidget);

      // A photo's picture opens the photo large, from the engine.
      await tester.tap(find.byKey(const Key('preview-p1.jpg')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('photo-preview')), findsOneWidget);
      expect(find.text('Chosen'), findsOneWidget);
      await tester.tap(find.text('Close'));
      await tester.pumpAndSettle();

      await tester.tap(find.text('Use these'));
      await tester.pumpAndSettle();
      expect(returned,
          unorderedEquals(['/trip/a.mp4', '/trip/p1.jpg', '/trip/p2.heic']));
    });

    testWidgets('each clip in the picker shows its picture, to be played before choosing',
        (tester) async {
      final engine = tripEngine();
      tester.view.physicalSize = const Size(1000, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(english(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => FileBrowserSheet.show(context, engine: engine),
              child: const Text('open'),
            ),
          ),
        ),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();
      final preview = find.byKey(const Key('preview-a.mp4'));
      expect(preview, findsOneWidget);
      expect(find.descendant(of: preview, matching: find.byType(Image)), findsOneWidget);
      expect(
        engine.calls.where((c) => c.$1 == 'media.thumbnails'),
        isNotEmpty,
        reason: 'the pictures come from the engine',
      );
    });

    testWidgets(
        'the music picker jumps to where music is, and says why a phone shows none',
        (tester) async {
      final engine = MusicEngine();
      tester.view.physicalSize = const Size(1000, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      List<String>? returned;
      await tester.pumpWidget(english(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async => returned = await FileBrowserSheet.show(
                  context,
                  engine: engine,
                  kind: 'audio'),
              child: const Text('open'),
            ),
          ),
        ),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();

      // The Music folder holds no song the app can see: on a phone, why.
      expect(find.byKey(const Key('place-music')), findsOneWidget);
      expect(find.text('NetEase Cloud Music'), findsOneWidget);
      expect(find.byKey(const Key('phone-music-note')), findsOneWidget);
      expect(find.textContaining('Music and audio'), findsOneWidget);

      // One tap to the music app's downloads, where the songs are.
      await tester.tap(find.byKey(const Key('place-netease')));
      await tester.pumpAndSettle();
      expect(engine.browsed.last, '/sdcard/netease/cloudmusic/Music');
      expect(find.byKey(const Key('phone-music-note')), findsNothing);
      await tester.tap(find.text('song.mp3'));
      await tester.pumpAndSettle();
      expect(returned, ['/sdcard/netease/cloudmusic/Music/song.mp3']);
    }, variant: TargetPlatformVariant.only(TargetPlatform.android));

    testWidgets('a disk plugged in is one tap away in the footage picker, by its name',
        (tester) async {
      final engine = DiskEngine();
      tester.view.physicalSize = const Size(1000, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      List<String>? returned;
      await tester.pumpWidget(english(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async =>
                  returned = await FileBrowserSheet.show(context, engine: engine),
              child: const Text('open'),
            ),
          ),
        ),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();

      // It opens at home; the usual folders and the disk are shortcuts.
      expect(find.byKey(const Key('place-home')), findsOneWidget);
      expect(find.text('Movies'), findsOneWidget);
      expect(find.byKey(const Key('place-volume-Media')), findsOneWidget);

      await tester.tap(find.byKey(const Key('place-volume-Media')));
      await tester.pumpAndSettle();
      expect(engine.browsed.last, '/Volumes/Media');
      await tester.tap(find.text('Use this folder'));
      await tester.pumpAndSettle();
      expect(returned, ['/Volumes/Media']);
    });

    testWidgets('reports a startup failure instead of showing an empty page',
        (tester) async {
      await pumpTall(
          tester, english(MaterialApp(home: HomePage(engine: FailingEngine()))));
      expect(find.text('The engine could not be reached'), findsOneWidget);
      expect(find.textContaining('vb serve'), findsOneWidget);
    });
  });
}

/// A phone's music: an empty Music folder, and a music app's downloads.
class MusicEngine extends FakeEngine {
  final List<String?> browsed = [];

  @override
  Future<DirectoryListing> browse(String? directory, {String kind = 'video'}) async {
    browsed.add(directory);
    const places = [
      (id: 'music', path: '/sdcard/Music', name: null),
      (id: 'netease', path: '/sdcard/netease/cloudmusic/Music', name: null),
    ];
    if (directory == '/sdcard/netease/cloudmusic/Music') {
      return DirectoryListing(
        path: directory!,
        parent: '/sdcard/netease/cloudmusic',
        entries: const [
          DirectoryEntry(
              name: 'song.mp3',
              path: '/sdcard/netease/cloudmusic/Music/song.mp3',
              kind: 'audio',
              sizeBytes: 1 << 22),
        ],
        error: null,
        places: places,
      );
    }
    return const DirectoryListing(
      path: '/sdcard/Music',
      parent: '/sdcard',
      entries: [
        DirectoryEntry(
            name: 'Albums', path: '/sdcard/Music/Albums', kind: 'dir', sizeBytes: 0),
      ],
      error: null,
      places: places,
    );
  }
}

/// A Mac with an external disk plugged in.
class DiskEngine extends FakeEngine {
  final List<String?> browsed = [];

  @override
  Future<DirectoryListing> browse(String? directory, {String kind = 'video'}) async {
    browsed.add(directory);
    const places = [
      (id: 'home', path: '/Users/me', name: null),
      (id: 'movies', path: '/Users/me/Movies', name: null),
      (id: 'volume', path: '/Volumes/Media', name: 'Media'),
    ];
    final here = directory ?? '/Users/me';
    return DirectoryListing(
      path: here,
      parent: here == '/' ? null : '/',
      entries: [
        DirectoryEntry(name: 'DJ', path: '$here/DJ', kind: 'dir', sizeBytes: 0),
      ],
      error: null,
      places: places,
    );
  }
}

/// A folder of a trip: two days, filmed both ways up, and one clip whose
/// date nothing records.
FakeEngine tripEngine() {
  final engine = FakeEngine();
  DirectoryEntry video(String name) => DirectoryEntry(
      name: name, path: '/trip/$name', kind: 'video', sizeBytes: 1 << 20);
  engine.browseEntries = [
    const DirectoryEntry(
        name: 'more', path: '/trip/more', kind: 'dir', sizeBytes: 0),
    video('a.mp4'),
    video('b.mp4'),
    video('c.mp4'),
    video('d.mp4'),
  ];
  Map<String, dynamic> clip(
          String name, String? at, String ratio, String shape) =>
      {
        'path': '/trip/$name',
        'capturedAt': at,
        'capturedFrom': at == null ? 'file' : 'camera',
        'durationUs': 23000000,
        'width': shape == 'portrait' ? 1080 : 1920,
        'height': shape == 'portrait' ? 1920 : 1080,
        'orientation': shape,
        'ratio': ratio,
      };
  engine.media = {
    '/trip/a.mp4': clip('a.mp4', '2023-10-01T09:00:00', '16:9', 'landscape'),
    '/trip/b.mp4': clip('b.mp4', '2023-10-01T18:30:00', '9:16', 'portrait'),
    '/trip/c.mp4': clip('c.mp4', '2023-10-03T11:15:00', '16:9', 'landscape'),
    '/trip/d.mp4': clip('d.mp4', null, '16:9', 'landscape'),
  };
  return engine;
}

/// A folder with both kinds of footage: two videos and two photos.
FakeEngine mixedEngine() {
  final engine = FakeEngine();
  DirectoryEntry file(String name, String kind) => DirectoryEntry(
      name: name, path: '/trip/$name', kind: kind, sizeBytes: 4 << 20);
  engine.browseEntries = [
    file('a.mp4', 'video'),
    file('b.mp4', 'video'),
    file('p1.jpg', 'image'),
    file('p2.heic', 'image'),
  ];
  Map<String, dynamic> item(String name, String kind, String at) => {
        'path': '/trip/$name',
        'kind': kind,
        'capturedAt': at,
        'capturedFrom': 'camera',
        'durationUs': kind == 'image' ? 0 : 23000000,
        'width': kind == 'image' ? 3060 : 1920,
        'height': kind == 'image' ? 4080 : 1080,
        'orientation': kind == 'image' ? 'portrait' : 'landscape',
        'ratio': kind == 'image' ? '3:4' : '16:9',
      };
  engine.media = {
    '/trip/a.mp4': item('a.mp4', 'video', '2023-10-01T19:12:00'),
    '/trip/b.mp4': item('b.mp4', 'video', '2023-10-01T19:18:00'),
    '/trip/p1.jpg': item('p1.jpg', 'image', '2023-10-01T18:31:00'),
    '/trip/p2.heic': item('p2.heic', 'image', '2023-10-01T18:32:00'),
  };
  return engine;
}

/// An engine that cannot be reached, which is what the web build sees when
/// `vb serve` has stopped.
class FailingEngine extends FakeEngine {
  @override
  String get transportLabel => 'fake';

  @override
  Future<EngineVersion> version() async =>
      throw const EngineException('connection refused');

  @override
  Future<DoctorReport> doctor() async =>
      throw const EngineException('connection refused');

  @override
  Future<List<EditTemplate>> templates() async =>
      throw const EngineException('connection refused');
}

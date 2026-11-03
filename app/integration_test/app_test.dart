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

/// The real app, with the real engine, driven from code.
///
/// Everything else in `test/` runs the UI against a fake engine. This runs
/// the macOS build with the Rust engine linked in, through the same FFI calls
/// a user's clicks make: the self-test, a project, a style, a run with
/// progress, the model library and the video butler.
///
/// It needs footage, named at build time:
///
///   flutter test integration_test -d macos \
///     --dart-define=VB_IT_FOOTAGE=/path/to/a/folder \
///     --dart-define=VB_IT_MUSIC=/path/to/music.wav \
///     --dart-define=VB_IT_PHOTOS=true   # when the folder holds photos too
///
/// It writes one project into the real library and deletes it at the end. It
/// does not export to an editor, so it leaves no draft in 剪映's list.
library;

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:video_butler/main.dart';
import 'package:video_butler/src/api_models.dart';
import 'package:video_butler/src/editor/edit_state.dart';
import 'package:video_butler/src/engine/engine_api.dart';
import 'package:video_butler/src/engine/engine_factory.dart';

const footage = String.fromEnvironment('VB_IT_FOOTAGE');
const music = String.fromEnvironment('VB_IT_MUSIC');

/// The footage folder holds photos as well as videos, so the cut should use
/// some: `--dart-define=VB_IT_PHOTOS=true`.
const photos = bool.fromEnvironment('VB_IT_PHOTOS');

/// Pump until [finder] finds something, or fail after [timeout]. The real
/// engine answers on other isolates, so a fixed number of frames is not
/// enough and `pumpAndSettle` gives up while a spinner turns.
Future<void> pumpUntil(
  WidgetTester tester,
  Finder finder, {
  Duration timeout = const Duration(seconds: 60),
}) async {
  final end = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 100));
    if (finder.evaluate().isNotEmpty) return;
  }
  fail('timed out waiting for $finder');
}

/// Pump until [finder] finds nothing.
Future<void> pumpUntilGone(
  WidgetTester tester,
  Finder finder, {
  Duration timeout = const Duration(minutes: 5),
}) async {
  final end = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 200));
    if (finder.evaluate().isEmpty) return;
  }
  fail('timed out waiting for $finder to go away');
}

/// Where the editor's transport says the playhead is, in microseconds.
int playheadShown(WidgetTester tester) {
  final label = tester
      .widgetList<Text>(find.byType(Text))
      .map((t) => t.data ?? '')
      .firstWhere((t) => RegExp(r'^\d\d:\d\d\.\d{3} / ').hasMatch(t));
  final m = RegExp(r'^(\d\d):(\d\d)\.(\d{3})').firstMatch(label)!;
  return ((int.parse(m[1]!) * 60 + int.parse(m[2]!)) * 1000 + int.parse(m[3]!)) * 1000;
}

/// Scroll the page's list until [finder] is built and on screen, as a user
/// would. Lists build lazily, so something far down does not exist yet.
Future<void> scrollTo(WidgetTester tester, Finder finder, {bool up = false}) async {
  await tester.scrollUntilVisible(
    finder,
    up ? -250 : 250,
    scrollable: find.byType(Scrollable).first,
    maxScrolls: 80,
  );
  await tester.pump(const Duration(milliseconds: 100));
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('from the self-test to a finished cut, on the real engine',
      (tester) async {
    expect(footage, isNotEmpty, reason: 'pass --dart-define=VB_IT_FOOTAGE=<folder>');
    final engine = createEngine();

    // A project set up through the engine rather than the file picker: the
    // picker is covered by widget tests, and walking it to a temp folder here
    // would test nothing but patience.
    final project = await engine.createProject('IT 验证');
    addTearDown(() => engine.deleteProject(project.id));
    await engine.updateProject(
      project.id,
      inputs: [footage],
      style: StyleChoice(
        mood: Mood.happy,
        topic: Topic.travel,
        beatSync: true,
        music: music.isEmpty ? null : music,
        // Shorter than the style's own length, so the choice is what decides.
        targetDurationUs: 15000000,
        // The fixture's music is 12 s, shorter than the cut: answered here,
        // as a person's saved answer would be, so the run is not held up by
        // the question (the question itself is a widget test).
        musicLoop: true,
      ),
    );

    await tester.pumpWidget(VideoButlerApp(engine: engine));

    // --- the self-test, in Chinese: the UI's and the engine's own words ----
    await pumpUntil(tester, find.text('运行环境'));
    expect(find.text('视频引擎'), findsOneWidget);
    expect(find.text('导出到剪映专业版'), findsOneWidget);
    expect(find.text('就绪'), findsOneWidget, reason: 'the linked engine can analyse');
    await tester.tap(find.byKey(const Key('selftest-continue')));

    // --- projects -----------------------------------------------------------
    await pumpUntil(tester, find.text('IT 验证'));
    await tester.tap(find.text('IT 验证'));
    await pumpUntil(tester, find.text('素材'));

    // The choices made above are what the page shows.
    await pumpUntil(tester, find.byKey(const Key('mood-happy')));
    final happy = tester.widget<ChoiceChip>(find.byKey(const Key('mood-happy')));
    expect(happy.selected, isTrue);
    // The editor list arrives from the engine a moment after the page.
    await tester.pump(const Duration(seconds: 2));
    await scrollTo(tester, find.byKey(const Key('editor-jianying')));
    final jianying =
        tester.widget<CheckboxListTile>(find.byKey(const Key('editor-jianying')));
    expect(jianying.onChanged, isNotNull, reason: '剪映 is installed and has a draft folder');

    // --- the run --------------------------------------------------------------
    await scrollTo(tester, find.byKey(const Key('run')));
    await tester.tap(find.byKey(const Key('run')));

    // Progress is shown while it runs, in place of the button: a percentage
    // and a way to stop.
    await pumpUntil(tester, find.byKey(const Key('run-percent')),
        timeout: const Duration(seconds: 20));
    expect(find.byKey(const Key('run-cancel')), findsOneWidget);
    final seen = <String>{};
    final end = DateTime.now().add(const Duration(minutes: 5));
    while (find.byKey(const Key('run-percent')).evaluate().isNotEmpty &&
        DateTime.now().isBefore(end)) {
      final percent = tester.widget<Text>(find.byKey(const Key('run-percent'))).data;
      if (percent != null) seen.add(percent);
      await tester.pump(const Duration(milliseconds: 150));
    }
    await pumpUntilGone(tester, find.byKey(const Key('run-percent')));
    // A fresh analysis shows the bar moving through several readings. The
    // same footage a second time is served from the library's cache and can
    // finish between two polls -- which is the cache doing its job, and is
    // checked below instead.
    expect(seen, isNotEmpty);

    // And then the result.
    await scrollTo(tester, find.text('结果'));
    if (seen.length <= 1) {
      expect(find.text('复用了分析结果'), findsOneWidget,
          reason: 'a run too quick to watch must have come from the cache');
    }
    expect(find.textContaining('欢快 · 旅行 · 卡点'), findsWidgets);
    expect(find.textContaining('配乐的 120 BPM 节拍'), findsWidgets);
    if (photos) {
      // A photo in the cut, chosen and held as a photo, in the engine's words.
      await scrollTo(tester, find.textContaining('张照片').first);
      expect(find.textContaining('照片踩着'), findsWidgets,
          reason: 'on the beat a photo is held whole beats');
    }
    await scrollTo(tester, find.text('这一版是怎么做的'));
    expect(find.textContaining('Apple Vision'), findsWidgets);
    expect(find.textContaining('Whisper'), findsWidgets,
        reason: 'the speech model downloaded to the library is used');
    expect(find.textContaining('景深、人脸、人像三个模型在本机运行'), findsWidgets,
        reason: 'the framing models in the library run on the ONNX Runtime the app carries');

    // The engine remembered the run against the project.
    final saved = (await engine.projects()).firstWhere((p) => p.id == project.id);
    expect(saved.lastRun, isNotNull);
    expect(saved.lastRun!.durationUs, greaterThan(0));
    expect(saved.lastRun!.durationUs, lessThanOrEqualTo(15000000),
        reason: 'the length chosen is the length delivered');

    // --- the built-in editor (0.2.0) ---------------------------------------------
    // The cut opens in the app's own editor, is changed there, plays, and
    // leaves as a video the engine renders.
    // The 成片 card sits above the result just read.
    await scrollTo(tester, find.byKey(const Key('open-editor')), up: true);
    await tester.tap(find.byKey(const Key('open-editor')));
    await pumpUntil(tester, find.byKey(const Key('editor-play')),
        timeout: const Duration(seconds: 30));
    // What the editor shows is the engine's session; ask it directly.
    Future<EditState> current() async => EditState.fromJson(
        await engine.call('editor.state', {'projectId': project.id}) as Map<String, dynamic>);
    await pumpUntil(tester, find.byKey(const Key('tool-split')));
    final opened = await current();
    expect(opened.clips, isNotEmpty);
    expect(opened.totalUs, saved.lastRun!.durationUs,
        reason: 'the editor opens the cut the run made, at its length');

    // Into the middle of the first clip of footage -- a cut may open on a
    // title card, which has nothing to split -- and split there; the engine
    // answers with one clip more. The timeline shows 60 pixels a second,
    // and a drag spends its first pixels getting past the touch slop.
    final first = opened.clips.firstWhere((c) => c.isClip && c.endUs - c.startUs >= 600000);
    final middle = (first.startUs + first.endUs) ~/ 2;
    await tester.drag(
      find.byKey(const Key('timeline-scroll')),
      Offset(-(middle / 1e6 * 60 + kDragSlopDefault), 0),
    );
    await tester.pump(const Duration(milliseconds: 500));
    expect(playheadShown(tester), inInclusiveRange(first.startUs + 100000, first.endUs - 100000));
    await tester.tap(find.byKey(const Key('tool-split')));
    final splitEnd = DateTime.now().add(const Duration(seconds: 20));
    var afterSplit = await current();
    while (afterSplit.clips.length == opened.clips.length && DateTime.now().isBefore(splitEnd)) {
      await tester.pump(const Duration(milliseconds: 100));
      afterSplit = await current();
    }
    expect(afterSplit.clips.length, opened.clips.length + 1);
    expect(afterSplit.totalUs, opened.totalUs, reason: 'a split changes no length');

    // The engine has split it; the page shows the new half selected once
    // its answer arrives.
    await pumpUntil(tester, find.byKey(const Key('tool-back')),
        timeout: const Duration(seconds: 20));

    // Text at the playhead, written in its sheet.
    await tester.tap(find.byKey(const Key('tool-back')));
    await tester.pump(const Duration(milliseconds: 300));
    await tester.tap(find.byKey(const Key('tool-text')));
    await pumpUntil(tester, find.byKey(const Key('text-words')));
    await tester.enterText(find.byKey(const Key('text-words')), '集成测试');
    await tester.tap(find.byKey(const Key('text-save')));
    await pumpUntilGone(tester, find.byKey(const Key('text-words')),
        timeout: const Duration(seconds: 20));
    final texts = (await current()).texts.where((t) => t.content == '集成测试');
    expect(texts, hasLength(1));

    // It plays through the platform's player, and stops.
    await tester.tap(find.byKey(const Key('editor-play')));
    await tester.pump(const Duration(seconds: 2));
    await tester.tap(find.byKey(const Key('editor-play')));
    await tester.pump(const Duration(milliseconds: 500));

    // A 720p video, rendered as a job, with its progress.
    await tester.tap(find.byKey(const Key('editor-export')));
    await pumpUntil(tester, find.byKey(const Key('export-res-720')));
    await tester.tap(find.byKey(const Key('export-res-720')));
    await tester.pump(const Duration(milliseconds: 200));
    await tester.tap(find.byKey(const Key('export-video')));
    await pumpUntil(tester, find.text('导出完成'), timeout: const Duration(minutes: 5));
    final exported =
        (await engine.projects()).firstWhere((p) => p.id == project.id).exports.last;
    addTearDown(() async {
      final file = File(exported.path);
      if (file.existsSync()) await file.delete();
    });
    expect(exported.kind, 'video');
    final read = (await engine.inspectMedia([exported.path]))[exported.path]!;
    expect(read.error, isNull);
    expect((read.durationUs - (await current()).totalUs).abs(), lessThan(100000),
        reason: 'the video is as long as the cut');
    expect(read.width < read.height ? read.width : read.height, 720);
    await tester.tap(find.text('完成'));
    await tester.pump(const Duration(milliseconds: 300));
    // The project page's back button is under the editor's: the last is
    // the editor's own.
    await tester.tap(find.byType(BackButton).last);
    await pumpUntilGone(tester, find.byKey(const Key('editor-play')),
        timeout: const Duration(seconds: 10));
    await pumpUntil(tester, find.byKey(const Key('cut-card')));
    expect(find.textContaining('上次编辑'), findsOneWidget,
        reason: 'the project says it was edited');

    // --- the model library and the video butler ----------------------------------------
    // By type, not with pageBack(): that finds the button by its English
    // tooltip, and in Chinese the tooltip is 返回.
    await tester.tap(find.byType(BackButton));
    // The project page also has a "Models" card, so wait for it to be gone
    // and go by the navigation rail.
    await pumpUntilGone(tester, find.text('素材'), timeout: const Duration(seconds: 10));
    Finder rail(String label) => find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text(label),
        );
    // The rail's hit target is the destination around the label, not the
    // label itself; the page that appears next is the real check.
    await tester.tap(rail('模型'), warnIfMissed: false);
    await pumpUntil(tester, find.textContaining('下载一次，所有项目通用'));
    expect(find.text('已安装'), findsWidgets,
        reason: 'the model downloaded earlier is in the shared library');
    await tester.tap(rail('视频管家'), warnIfMissed: false);
    // The faces come first; the smart categories are below them, out of
    // sight on a library with notes to show, and a list builds lazily.
    await pumpUntil(tester, find.byKey(const Key('butler-faces')));
    await tester.dragUntilVisible(
      find.byKey(const Key('butler-smart')),
      find.byKey(const Key('butler-faces')),
      const Offset(0, -200),
    );
    expect(find.text('智能分类'), findsOneWidget);
  });
}

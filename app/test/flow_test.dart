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
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/main.dart';
import 'package:video_butler/src/api_models.dart';
import 'package:video_butler/src/l10n/strings.dart';
import 'package:video_butler/src/theme.dart';
import 'package:video_butler/src/ui/album_page.dart';
import 'package:video_butler/src/ui/models_page.dart';
import 'package:video_butler/src/ui/project_page.dart';
import 'package:video_butler/src/ui/run_progress.dart';
import 'package:video_butler/src/ui/settings_page.dart';

import 'engine_contract_test.dart' show FakeEngine;

/// Pages here are ListViews, which build lazily; a tall surface keeps the
/// lower cards built and findable.
Future<void> pumpTall(WidgetTester tester, Widget child) async {
  await tester.binding.setSurfaceSize(const Size(1200, 3000));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(child);
  await tester.pumpAndSettle();
}

/// A page on its own, in Chinese -- the language the app starts in.
Widget app(Widget home) => LanguageScope(
      language: AppLanguage.zh,
      onChanged: (_) {},
      child: MaterialApp(theme: buildTheme(Brightness.light), home: home),
    );

Project projectFrom(FakeEngine engine, String id) =>
    Project.fromJson(engine.projects[id]!);

void main() {
  group('Opening the app', () {
    testWidgets('checks the machine first, in Chinese, including the editor',
        (tester) async {
      final engine = FakeEngine();
      await pumpTall(tester, VideoButlerApp(engine: engine));

      expect(find.text('运行环境'), findsOneWidget);
      expect(find.text('继续'), findsOneWidget);
      // Exporting to the editor is part of the self-test, because a draft
      // folder that is not there is found out otherwise only after a run.
      expect(find.text('剪映专业版 draft export'), findsOneWidget);
      // And the engine is asked to word the checks in the UI's language.
      final selftest = engine.calls.firstWhere((c) => c.$1 == 'selftest');
      expect(selftest.$2['lang'], 'zh');
    });

    testWidgets('continues to projects and the album', (tester) async {
      await pumpTall(tester, VideoButlerApp(engine: FakeEngine()));
      await tester.tap(find.byKey(const Key('selftest-continue')));
      await tester.pumpAndSettle();

      expect(find.text('项目'), findsWidgets);
      expect(find.byKey(const Key('new-project')), findsOneWidget);
      // People from earlier projects are on the first screen.
      expect(find.text('Mum'), findsOneWidget);
    });

    testWidgets('on a phone the pages are along the bottom and a project card fits',
        (tester) async {
      // 412 wide, as a Galaxy S21: a rail took half of it, and a project's
      // name ran down its card a letter or two at a time.
      tester.view.physicalSize = const Size(412, 915);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      final engine = FakeEngine();
      final project = await engine.call('projects.create', {'name': 'wohoo'}) as Map<String, dynamic>;
      project['inputs'] = List.generate(35, (i) => '/sdcard/DCIM/$i.mp4');
      project['style'] = {'topic': 'travel', 'targetDurationUs': 15000000, 'beatSync': true, 'music': '/m.mp3'};
      project['lastRun'] = {'at': '2026-09-27T10:00:00Z', 'durationUs': 14558000, 'clipsUsed': 4};
      await tester.pumpWidget(VideoButlerApp(engine: engine, skipSelfTest: true));
      await tester.pumpAndSettle();

      expect(tester.takeException(), isNull);
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(find.byType(NavigationRail), findsNothing);
      // The name on one line, the last run under the chips.
      expect(tester.getSize(find.text('wohoo')).height, lessThan(40));
      expect(find.textContaining('00:14.558 · '), findsOneWidget);
    });

    testWidgets('a new project opens straight into its page', (tester) async {
      final engine = FakeEngine();
      await pumpTall(tester, VideoButlerApp(engine: engine, skipSelfTest: true));

      await tester.tap(find.byKey(const Key('new-project')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('new-project-name')), '京都');
      await tester.tap(find.byKey(const Key('new-project-create')));
      await tester.pumpAndSettle();

      expect(engine.projects.values.single['name'], '京都');
      for (final title in ['素材', '风格', '时长', '配乐', '模型', '草稿送到哪里']) {
        expect(find.text(title), findsWidgets, reason: title);
      }
    });

    testWidgets('the language can be switched to English and is remembered',
        (tester) async {
      final engine = FakeEngine();
      await pumpTall(tester, VideoButlerApp(engine: engine, skipSelfTest: true));
      await tester.tap(find.text('设置'));
      await tester.pumpAndSettle();

      await tester.tap(find.text('English'));
      await tester.pumpAndSettle();

      expect(find.text('Settings'), findsWidgets);
      expect(find.text('Language'), findsOneWidget);
      expect(engine.language, 'en', reason: 'the engine words its own sentences in it');
    });
  });

  group('A project', () {
    Future<FakeEngine> open(
      WidgetTester tester, {
      List<String> inputs = const [],
      Map<String, dynamic>? style,
    }) async {
      final engine = FakeEngine();
      await engine.call('projects.create', {'name': '京都'});
      engine.projects['p1']!['inputs'] = inputs;
      if (style != null) engine.projects['p1']!['style'] = style;
      await pumpTall(
        tester,
        app(ProjectPage(engine: engine, project: projectFrom(engine, 'p1'))),
      );
      return engine;
    }

    testWidgets('saves the mood, the topic and the beat as they are chosen',
        (tester) async {
      final engine = await open(tester);

      await tester.tap(find.byKey(const Key('mood-happy')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('topic-travel')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('beat-sync')));
      await tester.pumpAndSettle();

      final style = engine.projects['p1']!['style'] as Map<String, dynamic>;
      expect(style['mood'], 'happy');
      expect(style['topic'], 'travel');
      expect(style['beatSync'], true);
      expect(find.text('欢快'), findsOneWidget);
      expect(find.text('旅行'), findsOneWidget);
    });

    testWidgets('主体衔接 says what it does, and what the loop switch does for it',
        (tester) async {
      final engine = await open(tester, style: {'music': '/music/song.mp3'});
      expect(find.byKey(const Key('music-loop-switch')), findsOneWidget);
      expect(find.byKey(const Key('continuity-hint')), findsNothing);

      await tester.ensureVisible(find.byKey(const Key('topic-continuity')));
      await tester.tap(find.byKey(const Key('topic-continuity')));
      await tester.pumpAndSettle();

      final style = engine.projects['p1']!['style'] as Map<String, dynamic>;
      expect(style['topic'], 'continuity');
      expect(find.text('主体衔接'), findsOneWidget);
      expect(find.byKey(const Key('continuity-hint')), findsOneWidget);
      // Its automatic length is the action's own, unknown until analysis,
      // so the run cannot ask about short music: the switch stays, saying
      // that off means silence, never a faster action.
      expect(find.byKey(const Key('music-loop-switch')), findsOneWidget);
      expect(find.byKey(const Key('music-loop-continuity')), findsOneWidget);
      expect(find.textContaining('静音'), findsOneWidget);
      expect(find.textContaining('完整动作的长度'), findsOneWidget);
      expect(find.textContaining('一拍半'), findsOneWidget);

      await tester.ensureVisible(find.byKey(const Key('music-loop-switch')));
      await tester.tap(find.byKey(const Key('music-loop-switch')));
      await tester.pumpAndSettle();
      expect(
        (engine.projects['p1']!['style'] as Map<String, dynamic>)['musicLoop'],
        true,
      );
    });

    testWidgets('a length can be chosen, and automatic clears it', (tester) async {
      final engine = await open(tester);
      await tester.tap(find.byKey(const Key('duration-30')));
      await tester.pumpAndSettle();
      var style = engine.projects['p1']!['style'] as Map<String, dynamic>;
      expect(style['targetDurationUs'], 30000000);
      expect(find.text('30 秒'), findsOneWidget);
      expect(find.text('1 分钟'), findsOneWidget);

      await tester.tap(find.byKey(const Key('duration-auto')));
      await tester.pumpAndSettle();
      style = engine.projects['p1']!['style'] as Map<String, dynamic>;
      expect(style.containsKey('targetDurationUs'), isFalse);
    });

    testWidgets('importing music asks for audio, then shows its length and tempo',
        (tester) async {
      final engine = await open(tester);
      expect(find.text('没有配乐，草稿保留素材原声。'), findsOneWidget);

      await tester.tap(find.byKey(const Key('import-music')));
      await tester.pumpAndSettle();
      expect(engine.lastBrowseKind, 'audio');
      expect(find.text('选择音乐'), findsOneWidget);
    });

    testWidgets('chosen music is heard: its length and beat are shown',
        (tester) async {
      final engine = await open(tester, style: {'music': '/music/song.mp3'});
      expect(find.text('song.mp3'), findsOneWidget);
      expect(find.text('03:04 · 118 BPM'), findsOneWidget);
      expect(engine.calls.any((c) => c.$1 == 'music.inspect'), isTrue);
    });

    testWidgets('offers the editors that can take a draft and not the others',
        (tester) async {
      final engine = await open(tester);
      final jianying =
          tester.widget<CheckboxListTile>(find.byKey(const Key('editor-jianying')));
      final capcut =
          tester.widget<CheckboxListTile>(find.byKey(const Key('editor-capcut')));
      expect(jianying.onChanged, isNotNull);
      expect(capcut.onChanged, isNull, reason: 'CapCut is not installed');

      await tester.tap(find.byKey(const Key('editor-jianying')));
      await tester.pumpAndSettle();
      expect(engine.projects['p1']!['editors'], ['jianying']);
      expect(find.text('生成草稿并送到剪映专业版'), findsOneWidget);
    });

    testWidgets('will not run with no footage', (tester) async {
      await open(tester);
      final run = tester.widget<FilledButton>(find.byKey(const Key('run')));
      expect(run.onPressed, isNull);
    });

    testWidgets('shows the stage and percentage while running, then the draft',
        (tester) async {
      final engine = await open(tester, inputs: ['/movies/kyoto']);

      await tester.tap(find.byKey(const Key('run')));
      // One poll: the run is part-way through the analysis.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 10));

      expect(find.text('42%'), findsOneWidget);
      expect(find.text('分析画面'), findsWidgets);
      // Clips are analysed several at a time; the count is of those done.
      expect(find.text('已完成 1 / 3 个素材 · 2 个同时分析'), findsOneWidget);
      expect(find.text('clip-a.mp4 · 从头到尾看画面并分段'), findsOneWidget);
      expect(find.byKey(const Key('run-cancel')), findsOneWidget);

      // The next poll finds it finished.
      await tester.pump(const Duration(milliseconds: 450));
      await tester.pumpAndSettle();

      expect(find.text('已送到剪辑软件'), findsOneWidget);
      expect(find.text('打开剪映专业版'), findsOneWidget);
      expect(find.textContaining('踩着配乐的 120 BPM 节拍'), findsOneWidget);
      expect(find.textContaining('欢快 · 旅行 · 卡点'), findsOneWidget);
      expect(engine.calls.map((c) => c.$1), contains('run.start'));
    });
  });

  group('Before a run', () {
    Future<FakeEngine> open(
      WidgetTester tester, {
      Map<String, dynamic>? style,
      bool musicShort = false,
    }) async {
      final engine = FakeEngine()..musicShort = musicShort;
      await engine.call('projects.create', {'name': '新疆'});
      engine.projects['p1']!['inputs'] = ['/movies/xinjiang'];
      if (style != null) engine.projects['p1']!['style'] = style;
      await pumpTall(
        tester,
        app(ProjectPage(engine: engine, project: projectFrom(engine, 'p1'))),
      );
      return engine;
    }

    testWidgets('music shorter than the cut is raised, and the answer kept',
        (tester) async {
      final engine = await open(
        tester,
        style: {'music': '/music/song.mp3', 'targetDurationUs': 300000000},
        musicShort: true,
      );
      await tester.tap(find.byKey(const Key('run')));
      await tester.pumpAndSettle();

      expect(find.text('配乐不够长'), findsOneWidget);
      expect(find.textContaining('配乐只有 02:35，成片最长 05:00'), findsOneWidget);
      expect(engine.calls.map((c) => c.$1), isNot(contains('run.start')),
          reason: 'nothing runs until the user has answered');

      await tester.tap(find.byKey(const Key('music-loop')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 10));
      expect(engine.projects['p1']!['style']['musicLoop'], isTrue);
      expect(engine.calls.map((c) => c.$1), contains('run.start'));
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();

      // The choice shows, and can be changed, under the music.
      final loop =
          tester.widget<SwitchListTile>(find.byKey(const Key('music-loop-switch')));
      expect(loop.value, isTrue);
    });

    testWidgets('a cancelled question starts nothing', (tester) async {
      final engine = await open(
        tester,
        style: {'music': '/music/song.mp3'},
        musicShort: true,
      );
      await tester.tap(find.byKey(const Key('run')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('取消'));
      await tester.pumpAndSettle();
      expect(engine.calls.map((c) => c.$1), isNot(contains('run.start')));
      expect(engine.projects['p1']!['style']['musicLoop'], isNull);
    });

    testWidgets('music long enough is not asked about', (tester) async {
      final engine = await open(tester, style: {'music': '/music/song.mp3'});
      await tester.tap(find.byKey(const Key('run')));
      await tester.pump();
      expect(find.text('配乐不够长'), findsNothing);
      expect(engine.calls.map((c) => c.$1), contains('run.start'));
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
    });

    testWidgets('earlier analysis is reused unless asked to look again',
        (tester) async {
      final engine = await open(tester);
      expect(find.text('沿用已有分析'), findsOneWidget);
      await tester.tap(find.text('重新分析'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('run')));
      await tester.pump();
      final start = engine.calls.lastWhere((c) => c.$1 == 'run.start');
      expect(start.$2['fresh'], isTrue);
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();

      // A one-off: the next run reuses what this one found.
      await tester.tap(find.byKey(const Key('run')));
      await tester.pump();
      final again = engine.calls.lastWhere((c) => c.$1 == 'run.start');
      expect(again.$2.containsKey('fresh'), isFalse);
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
    });
  });

  testWidgets('settings clear earlier analysis only after asking', (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(SettingsPage(engine: engine)));
    expect(find.textContaining('12 条'), findsOneWidget);

    await tester.tap(find.byKey(const Key('cache-clear')));
    await tester.pumpAndSettle();
    expect(find.text('清除全部分析？'), findsOneWidget);
    expect(engine.calls.map((c) => c.$1), isNot(contains('cache.clear')));

    await tester.tap(find.byKey(const Key('cache-clear-confirm')));
    await tester.pumpAndSettle();
    expect(engine.calls.map((c) => c.$1), contains('cache.clear'));
    expect(find.text('还没有保存任何分析结果。'), findsOneWidget);
  });

  testWidgets('settings say hard links take no space, and keep the choice',
      (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(SettingsPage(engine: engine)));
    expect(find.text('硬链接（推荐）'), findsOneWidget);
    expect(find.textContaining('不占额外空间'), findsOneWidget);

    await tester.tap(find.byKey(const Key('draft-media-reference')));
    await tester.pumpAndSettle();
    expect(engine.draftMedia, 'reference');
    final update = engine.calls.lastWhere((c) => c.$1 == 'settings.update');
    expect(update.$2['draftMedia'], 'reference');
  });

  testWidgets('the heat guard is on until the user turns it off', (tester) async {
    final engine = FakeEngine();
    await pumpTall(tester, app(SettingsPage(engine: engine)));
    expect(find.text('发热时自动放慢'), findsOneWidget);
    final guard = find.byKey(const Key('thermal-guard'));
    expect(tester.widget<SwitchListTile>(guard).value, isTrue);

    await tester.tap(guard);
    await tester.pumpAndSettle();
    expect(engine.thermalGuard, isFalse);
    expect(tester.widget<SwitchListTile>(guard).value, isFalse);
  });

  testWidgets('a run slowed by heat says so, rather than looking hung', (tester) async {
    JobStatus running(String? thermal) => JobStatus(
          jobId: 'j',
          state: 'running',
          stage: 'analyse',
          stageLabel: 'Watching the footage',
          percent: 40,
          detail: '',
          sourceIndex: 2,
          sourceCount: 9,
          stages: const [],
          thermal: thermal,
        );
    Widget card(String? thermal) => app(Scaffold(
          body: RunProgressCard(status: running(thermal), onCancel: () {}),
        ));
    await tester.pumpWidget(card(null));
    expect(find.byKey(const Key('run-thermal')), findsNothing);
    await tester.pumpWidget(card('slowing'));
    expect(find.text('设备有点热，已放慢速度，避免过热'), findsOneWidget);
    await tester.pumpWidget(card('cooling'));
    expect(find.text('设备太热，暂停一会儿等它降温，之后自动继续'), findsOneWidget);
  });

  testWidgets('the model library offers downloads with their licence',
      (tester) async {
    await pumpTall(tester, app(ModelsPage(engine: FakeEngine())));
    expect(find.text('Whisper base (quantised)'), findsOneWidget);
    expect(find.text('MIT'), findsOneWidget);
    expect(find.byKey(const Key('download-whisper-base-q5')), findsOneWidget);
    expect(find.text('下载一次，所有项目通用'), findsOneWidget);
  });

  testWidgets('the model library fits a phone', (tester) async {
    // 360 wide: a name and its chips ran under the download button on a
    // phone before they wrapped.
    tester.view.physicalSize = const Size(360, 3000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(app(ModelsPage(engine: _CrowdedModels())));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    expect(find.text('Whisper 基础版（量化）加长名字'), findsOneWidget);
  });

  testWidgets('every size came with the app, and the one in use can be changed',
      (tester) async {
    final engine = _ChoosableModels();
    await pumpTall(tester, app(ModelsPage(engine: engine)));
    // The most capable runs; the others can be chosen, except one this
    // device cannot hold. Nothing that came with the app can be deleted.
    expect(find.byKey(const Key('in-use-whisper-small')), findsOneWidget);
    expect(find.byKey(const Key('use-whisper-small')), findsNothing);
    expect(find.byKey(const Key('use-whisper-tiny')), findsNothing);
    expect(find.text('随安装包提供'), findsNWidgets(3));
    expect(find.byTooltip('删除'), findsNothing);

    await tester.tap(find.byKey(const Key('use-whisper-small-q5')));
    await tester.pumpAndSettle();
    expect(engine.used, 'whisper-small-q5');
    expect(find.byKey(const Key('in-use-whisper-small-q5')), findsOneWidget);
    expect(find.byKey(const Key('use-whisper-small')), findsOneWidget);
  });

  testWidgets('a folder of models this phone cannot run says so, not that it is empty',
      (tester) async {
    const channel = MethodChannel('videobutler/models');
    Map<Object?, Object?>? sent;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      if (call.method != 'importFolder') return null;
      sent = call.arguments as Map<Object?, Object?>;
      // The folder had the 8B and nothing else.
      return {
        'imported': <String>[],
        'alreadyInstalled': <String>[],
        'failed': <String>[],
        'missing': <String>[],
        'tooBig': ['qwen3-vl-8b-instruct-q4_k_m.gguf', 'qwen3-vl-8b-instruct-mmproj-f16.gguf'],
      };
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, null));
    await pumpTall(tester, app(ModelsPage(engine: _PhoneModels())));

    await tester.tap(find.byKey(const Key('import-models')));
    await tester.pumpAndSettle();
    // The 8B is only looked for, never copied; the 4B is the one to take.
    expect(sent!['skip'], contains('qwen3-vl-8b-instruct-q4_k_m.gguf'));
    final preferred = [
      for (final group in sent!['prefer']! as List)
        for (final file in group as List) (file as Map)['name'],
    ];
    expect(preferred.first, 'qwen3-vl-4b-instruct-q4_k_m.gguf');
    expect(preferred.where((name) => '$name'.contains('8b')), isEmpty);
    expect(find.text('Qwen3-VL 8B 需要的内存比这台设备多，没有导入'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('a person opens onto every project and clip they are in',
      (tester) async {
    final engine = FakeEngine();
    await engine.call('projects.create', {'name': '京都'});
    await pumpTall(tester, app(AlbumPage(engine: engine)));

    await tester.tap(find.text('Mum'));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('subject-summary')), findsOneWidget);
    expect(find.text('出现在 2 个项目、3 段视频里，共 7 个镜头'), findsOneWidget);
    expect(find.text('2 段视频 · 6 个镜头'), findsOneWidget);
    expect(find.text('a.mp4'), findsOneWidget);
    expect(find.text('b.mp4'), findsOneWidget);
    // A project that is no longer there shows, and cannot be opened.
    expect(find.byKey(const Key('subject-open-p1')), findsOneWidget);
    expect(find.byKey(const Key('subject-open-p9')), findsNothing);

    await tester.tap(find.byKey(const Key('subject-open-p1')));
    await tester.pumpAndSettle();
    expect(find.byType(ProjectPage), findsOneWidget);
    expect(find.text('京都'), findsWidgets);
  });

  testWidgets('the album says faces are grouped, not recognised', (tester) async {
    await pumpTall(tester, app(AlbumPage(engine: FakeEngine())));
    expect(find.text('Mum'), findsOneWidget);
    expect(find.textContaining('not recognised'), findsOneWidget);
  });

  testWidgets('settings never show the stored key', (tester) async {
    await pumpTall(tester, app(SettingsPage(engine: FakeEngine())));
    final url = tester.widget<TextField>(find.byKey(const Key('cloud-url')));
    expect(url.controller!.text, 'https://api.example.com/v1');
    // Only that one is stored; the engine never sends the key itself back.
    expect(find.textContaining('已保存密钥'), findsWidgets);
    expect(find.text('删除已保存的密钥'), findsOneWidget);
  });
}

/// Models whose names and chips are as wide as they get: installed, too big
/// for the device, and one not installed with its download button.
class _CrowdedModels extends FakeEngine {
  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    if (method != 'models.list') return super.call(method, args);
    Map<String, dynamic> model(String id, String name, bool installed) => {
          'id': id,
          'name': name,
          'role': 'speech',
          'roleLabel': 'Speech',
          'bytes': 59707625,
          'license': 'Apache-2.0',
          'note': 'A note long enough to take two lines on a phone screen.',
          'memoryMb': 500,
          'installed': installed,
          'fits': false,
          'path': '/lib/models/$id.bin',
        };
    return {
      'directory': '/lib/models',
      'models': [
        model('a', 'Whisper 基础版（量化）加长名字', true),
        model('b', 'Depth Anything V2 小型', false),
      ],
    };
  }
}

/// Three speech models that came with the app: the largest in use by
/// default, one too big for this device.
class _ChoosableModels extends FakeEngine {
  String used = 'whisper-small';

  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    if (method == 'models.use') {
      used = args['id'] as String;
    } else if (method != 'models.list') {
      return super.call(method, args);
    }
    Map<String, dynamic> model(String id, String name, {bool fits = true}) => {
          'id': id,
          'name': name,
          'role': 'speech',
          'roleLabel': 'Speech',
          'bytes': 190085487,
          'license': 'MIT',
          'note': 'Speech.',
          'memoryMb': 1000,
          'installed': true,
          'bundled': true,
          'fits': fits,
          'inUse': id == used,
          'path': '/app/models/$id.bin',
        };
    return {
      'directory': '/lib/models',
      'models': [
        model('whisper-tiny', 'Whisper 微型', fits: false),
        model('whisper-small-q5', 'Whisper 小型（量化）'),
        model('whisper-small', 'Whisper 小型'),
      ],
    };
  }
}

/// Caption models on a phone that can run the 4B and not the 8B.
class _PhoneModels extends FakeEngine {
  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    if (method != 'models.list') return super.call(method, args);
    Map<String, dynamic> model(String size, int memoryMb, {required bool fits}) => {
          'id': 'qwen3-vl-$size-q4',
          'name': 'Qwen3-VL ${size.toUpperCase()}',
          'role': 'describe',
          'roleLabel': 'Captions',
          'bytes': 3333461920,
          'license': 'Apache-2.0',
          'note': 'Captions.',
          'memoryMb': memoryMb,
          'installed': false,
          'fits': fits,
          'files': [
            {'name': 'qwen3-vl-$size-instruct-q4_k_m.gguf', 'bytes': 1},
            {'name': 'qwen3-vl-$size-instruct-mmproj-f16.gguf', 'bytes': 1},
          ],
          'path': '/lib/models/qwen3-vl-$size-instruct-q4_k_m.gguf',
        };
    return {
      'directory': '/lib/models',
      'models': [
        model('8b', 7000, fits: false),
        model('4b', 4500, fits: true),
        model('2b', 2800, fits: true),
      ],
    };
  }
}

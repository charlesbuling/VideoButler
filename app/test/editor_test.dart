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

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/src/api_models.dart';
import 'package:video_butler/src/editor/edit_state.dart';
import 'package:video_butler/src/editor/editor_page.dart';
import 'package:video_butler/src/editor/picture_rules.dart';
import 'package:video_butler/src/engine/engine.dart';
import 'package:video_butler/src/l10n/strings.dart';
import 'package:video_butler/src/theme.dart';
import 'package:video_butler/src/ui/project_page.dart';

import 'engine_contract_test.dart' show FakeEngine;

const _png =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

/// The engine's editor, in memory: a cut of two clips the fake changes the
/// way `vb-edit` would for the operations these tests send, with the
/// engine's own timeline arithmetic (a transition overlaps its neighbours).
class EditorEngine extends FakeEngine {
  EditorEngine() {
    projects['p1'] = {
      'id': 'p1',
      'name': '京都',
      'inputs': ['/movies/kyoto'],
      'style': <String, dynamic>{},
      'model': 'local',
      'editors': <String>[],
      'createdAt': '2026-10-09T00:00:00Z',
      'updatedAt': '2026-10-09T00:00:00Z',
      'dir': '/lib/projects/p1',
      'lastRun': {
        'at': '2026-10-09T10:00:00Z',
        'draftPath': '/lib/projects/p1/project.otio',
        'durationUs': 7000000,
        'clipsUsed': 2,
        'editorDrafts': <String>[],
      },
    };
  }

  List<Map<String, dynamic>> slots = [
    {
      'id': 's1',
      'order': 0,
      'kind': 'clip',
      'durationUs': 4000000,
      'sourceRef': {'sourceId': 'src', 'inUs': 2000000, 'outUs': 6000000},
    },
    {
      'id': 's2',
      'order': 1,
      'kind': 'clip',
      'durationUs': 3000000,
      'sourceRef': {'sourceId': 'src', 'inUs': 10000000, 'outUs': 13000000},
    },
  ];
  List<Map<String, dynamic>> texts = [];
  List<Map<String, dynamic>> audio = [];
  List<Map<String, dynamic>> transitions = [];
  String title = '京都';
  int cutVersion = 1;

  /// Whether this device has a video encoder, as the engine reports it.
  bool canRenderVideo = true;

  /// What the engine says about exporting without re-encoding.
  Map<String, dynamic> lossless = {
    'possible': true,
    'extraUs': 300000,
    'totalUs': 7300000,
    'reasons': <String>[],
  };

  /// What the last export asked for.
  String lastTarget = '';
  final List<String> undo = [];
  final List<String> redo = [];
  int _ids = 3;
  int renderPolls = 0;

  /// Every operation the editor sent, in order.
  List<Map<String, dynamic>> get ops => [
        for (final (method, args) in calls)
          if (method == 'editor.apply') args['op'] as Map<String, dynamic>,
      ];

  String _snapshot() => jsonEncode({'slots': slots, 'texts': texts, 'audio': audio, 'transitions': transitions, 'title': title});

  void _restore(String snapshot) {
    final m = jsonDecode(snapshot) as Map<String, dynamic>;
    List<Map<String, dynamic>> list(String k) =>
        [for (final e in m[k] as List<dynamic>) Map<String, dynamic>.from(e as Map)];
    slots = list('slots');
    texts = list('texts');
    audio = list('audio');
    transitions = list('transitions');
    title = m['title'] as String;
  }

  /// Where each slot sits, as `resolve_timeline` would put it.
  List<Map<String, dynamic>> _timeline() {
    final out = <Map<String, dynamic>>[];
    var cursor = 0;
    for (var i = 0; i < slots.length; i++) {
      final d = slots[i]['durationUs'] as int;
      var overlap = 0;
      if (i > 0) {
        final t = transitions.where((t) => t['fromSlotId'] == slots[i - 1]['id']).firstOrNull;
        if (t != null && t['type'] != 'cut') {
          final cap = [(slots[i - 1]['durationUs'] as int) ~/ 2, d ~/ 2].reduce((a, b) => a < b ? a : b);
          overlap = [(t['durationUs'] as int), cap].reduce((a, b) => a < b ? a : b);
        }
      }
      final start = cursor - overlap;
      out.add({'slotId': slots[i]['id'], 'startUs': start, 'endUs': start + d, 'overlapUs': overlap});
      cursor = start + d;
    }
    return out;
  }

  Map<String, dynamic> _state({Map<String, dynamic>? selection}) {
    final timeline = _timeline();
    int startOf(String slotId) => timeline.firstWhere((r) => r['slotId'] == slotId)['startUs'] as int;
    final total = timeline.isEmpty ? 0 : timeline.last['endUs'] as int;
    return {
      'projectId': 'p1',
      'title': title,
      'version': cutVersion,
      'dsl': {
        'meta': {'title': title},
        'slots': slots,
        'tracks': {'video': <dynamic>[], 'text': texts, 'audio': audio},
        'transitions': transitions,
      },
      'timeline': {
        'totalUs': total,
        'slots': timeline,
        'texts': [
          for (final t in texts)
            {
              'id': t['id'],
              'startUs': startOf((t['anchor'] as Map)['slotId'] as String) + ((t['anchor'] as Map)['offsetUs'] as int? ?? 0),
              'endUs': startOf((t['anchor'] as Map)['slotId'] as String) +
                  ((t['anchor'] as Map)['offsetUs'] as int? ?? 0) +
                  (t['durationUs'] as int),
            },
        ],
        'audio': [
          for (final a in audio) {'id': a['id'], 'kind': a['kind'], 'startUs': 0, 'endUs': total},
        ],
      },
      'sources': [
        {
          'id': 'src',
          'path': '/movies/kyoto/a.mp4',
          'kind': 'video',
          'durationUs': 20000000,
          'width': 1920,
          'height': 1080,
          'exists': true,
        },
      ],
      'frame': {'width': 1920, 'height': 1080},
      'fps': 30.0,
      'canUndo': undo.isNotEmpty,
      'canRedo': redo.isNotEmpty,
      'undoLabel': undo.isEmpty ? null : '上一步',
      'redoLabel': redo.isEmpty ? null : '下一步',
      'edited': cutVersion > 1,
      'changeRatio': cutVersion > 1 ? 0.25 : 0.0,
      'newerRun': false,
      'canRenderVideo': canRenderVideo,
      'selection': selection,
      'limits': {'minSlotUs': 100000, 'minTextUs': 200000, 'minSpeed': 0.25, 'maxSpeed': 4.0},
    };
  }

  Map<String, dynamic> _slotAt(int at) {
    final timeline = _timeline();
    final r = timeline.lastWhere((r) => (r['startUs'] as int) <= at, orElse: () => timeline.first);
    return slots.firstWhere((s) => s['id'] == r['slotId']);
  }

  Map<String, dynamic>? _apply(Map<String, dynamic> op) {
    switch (op['op']) {
      case 'split':
        final at = op['atUs'] as int;
        final slot = op['slotId'] == null ? _slotAt(at) : slots.firstWhere((s) => s['id'] == op['slotId']);
        final start = _timeline().firstWhere((r) => r['slotId'] == slot['id'])['startUs'] as int;
        final into = at - start;
        if (into < 100000 || (slot['durationUs'] as int) - into < 100000) {
          throw const EngineException('离片段两端太近，无法分割。');
        }
        final ref = Map<String, dynamic>.from(slot['sourceRef'] as Map);
        final cut = (ref['inUs'] as int) + into;
        final second = {
          'id': 's${_ids++}',
          'order': 0,
          'kind': 'clip',
          'durationUs': (slot['durationUs'] as int) - into,
          'sourceRef': {...ref, 'inUs': cut},
        };
        slot['durationUs'] = into;
        slot['sourceRef'] = {...ref, 'outUs': cut};
        slots.insert(slots.indexOf(slot) + 1, second);
        return {'kind': 'slot', 'id': second['id']};
      case 'delete':
        if (slots.length == 1) throw const EngineException('只剩一个片段，不能再删了。');
        slots.removeWhere((s) => s['id'] == op['slotId']);
        texts.removeWhere((t) => (t['anchor'] as Map)['slotId'] == op['slotId']);
        return null;
      case 'speed':
        final slot = slots.firstWhere((s) => s['id'] == op['slotId']);
        final ref = slot['sourceRef'] as Map;
        final speed = (op['speed'] as num).toDouble();
        slot['speed'] = speed;
        slot['durationUs'] = (((ref['outUs'] as int) - (ref['inUs'] as int)) / speed).round();
        return {'kind': 'slot', 'id': slot['id']};
      case 'addText':
        final slot = _slotAt(op['atUs'] as int);
        final id = 'txt-${_ids++}';
        texts.add({
          'id': id,
          'anchor': {'slotId': slot['id'], 'offsetUs': 0},
          'durationUs': 3000000,
          'content': op['content'],
          'role': 'caption',
          'style': {'fontSizePt': 36.0, 'primaryColor': '#FFFFFF', 'alignment': 'middleCenter'},
        });
        return {'kind': 'text', 'id': id};
      case 'updateText':
        final text = texts.firstWhere((t) => t['id'] == op['id']);
        if (op['content'] != null) text['content'] = op['content'];
        if (op['style'] != null) text['style'] = op['style'];
        return {'kind': 'text', 'id': text['id']};
      case 'transition':
        transitions.removeWhere((t) => t['fromSlotId'] == op['fromSlotId']);
        final from = slots.indexWhere((s) => s['id'] == op['fromSlotId']);
        transitions.add({
          'fromSlotId': op['fromSlotId'],
          'toSlotId': slots[from + 1]['id'],
          'type': op['kind'],
          'durationUs': op['durationUs'],
        });
        return null;
      case 'setMusic':
        audio
          ..removeWhere((a) => a['kind'] == 'bgm')
          ..add({'id': 'aud-music-1', 'kind': 'bgm', 'assetPath': op['path'], 'anchor': {'slotId': 's1'}});
        return {'kind': 'music', 'id': 'aud-music-1'};
      case 'rename':
        title = op['title'] as String;
        return null;
      default:
        return null;
    }
  }

  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    switch (method) {
      case 'editor.open':
      case 'editor.state':
        calls.add((method, args));
        return _state();
      case 'editor.apply':
        calls.add((method, args));
        final before = _snapshot();
        final selection = _apply(Map<String, dynamic>.from(args['op'] as Map));
        if (_snapshot() != before) {
          undo.add(before);
          redo.clear();
          cutVersion++;
        }
        return _state(selection: selection);
      case 'editor.undo':
      case 'editor.redo':
        calls.add((method, args));
        final from = method == 'editor.undo' ? undo : redo;
        final to = method == 'editor.undo' ? redo : undo;
        if (from.isNotEmpty) {
          to.add(_snapshot());
          _restore(from.removeLast());
          cutVersion++;
        }
        return _state();
      case 'editor.frame':
        calls.add((method, args));
        return {'png': _png, 'version': cutVersion, 'width': 1, 'height': 1, 'atUs': args['atUs']};
      case 'editor.strip':
        return {
          'items': [
            for (final item in args['items'] as List<dynamic>)
              {...(item as Map<String, dynamic>), 'png': _png},
          ],
        };
      case 'editor.lossless':
        calls.add((method, args));
        return lossless;
      case 'editor.export':
        calls.add((method, args));
        lastTarget = args['target'] as String;
        if (const ['video', 'lossless', 'clips'].contains(lastTarget)) {
          renderPolls = 0;
          return {'jobId': 'job-render'};
        }
        return {
          'path': '/Users/me/Movies/JianyingPro/京都',
          'editor': args['target'],
          'warnings': <String>[],
        };
      case 'editor.close':
        calls.add((method, args));
        return {'closed': true};
      case 'run.status' when args['jobId'] == 'job-render':
        renderPolls++;
        final done = renderPolls > 1;
        return {
          'jobId': 'job-render',
          'kind': 'render',
          'state': done ? 'done' : 'running',
          'finished': done,
          'stage': done ? 'done' : 'render',
          'stageLabel': 'Exporting the video',
          'percent': done ? 100.0 : 40.0,
          'detail': '',
          'step': 'render.frames',
          'sourceIndex': done ? 210 : 84,
          'sourceCount': 210,
          'stages': const <dynamic>[],
          if (done && lastTarget == 'clips')
            'result': {
              'path': '/Users/me/Movies/Video Butler/京都',
              'files': ['/Users/me/Movies/Video Butler/京都/01 a.mp4'],
              'copied': 2,
              'rendered': 0,
              'warnings': <String>[],
            }
          else if (done)
            'result': {
              'path': '/Users/me/Movies/Video Butler/京都.mp4',
              'durationUs': 7000000,
              'bytes': 5300000,
              'warnings': <String>[],
            },
        };
      default:
        return super.call(method, args);
    }
  }
}

Widget app(Widget home) => LanguageScope(
      language: AppLanguage.zh,
      onChanged: (_) {},
      child: MaterialApp(theme: buildTheme(Brightness.light), home: home),
    );

Future<EditorEngine> openEditor(WidgetTester tester) async {
  await tester.binding.setSurfaceSize(const Size(1200, 900));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final engine = EditorEngine();
  await tester.pumpWidget(app(EditorPage(
    engine: engine,
    projectId: 'p1',
    projectName: '京都',
    livePlayback: false,
  )));
  await tester.pumpAndSettle();
  return engine;
}

/// Move the playhead by dragging the timeline, as a person scrubs.
Future<void> scrub(WidgetTester tester, double dx) async {
  await tester.drag(find.byKey(const Key('timeline-scroll')), Offset(dx, 0));
  await tester.pumpAndSettle();
}

/// Where the transport says the playhead is, in microseconds.
int playheadShown(WidgetTester tester) {
  final label = tester
      .widgetList<Text>(find.byType(Text))
      .map((t) => t.data ?? '')
      .firstWhere((t) => RegExp(r'^\d\d:\d\d\.\d{3} / ').hasMatch(t));
  final m = RegExp(r'^(\d\d):(\d\d)\.(\d{3})').firstMatch(label)!;
  return ((int.parse(m[1]!) * 60 + int.parse(m[2]!)) * 1000 + int.parse(m[3]!)) * 1000;
}

void main() {
  group('The editor', () {
    testWidgets('opens the cut with its clips, the frame and the tools', (tester) async {
      final engine = await openEditor(tester);
      expect(find.byKey(const Key('clip-s1')), findsOneWidget);
      expect(find.byKey(const Key('clip-s2')), findsOneWidget);
      expect(find.text('AI 成片，尚未修改'), findsOneWidget);
      expect(find.text('00:00.000 / 00:07.000'), findsOneWidget);
      // The still preview is the engine's own frame.
      expect(engine.calls.any((c) => c.$1 == 'editor.frame'), isTrue);
      for (final tool in ['add', 'split', 'text', 'music', 'canvas', 'transitions', 'sound']) {
        expect(find.byKey(Key('tool-$tool')), findsOneWidget, reason: tool);
      }
    });

    testWidgets('splits at the playhead, and undoes and redoes it', (tester) async {
      final engine = await openEditor(tester);
      // Some way into the first clip, as far as the drag took it.
      await scrub(tester, -140);
      final at = playheadShown(tester);
      expect(at, inInclusiveRange(1000000, 3000000));
      await tester.tap(find.byKey(const Key('tool-split')));
      await tester.pumpAndSettle();
      final split = engine.ops.single;
      expect(split['op'], 'split');
      // Exactly where the playhead was shown, to the millisecond shown.
      expect((split['atUs'] as int) ~/ 1000 * 1000, at);
      expect(engine.slots, hasLength(3));
      expect(find.byKey(const Key('clip-s3')), findsOneWidget);
      // The new clip is selected, with the clip's tools.
      expect(find.byKey(const Key('tool-speed')), findsOneWidget);

      await tester.tap(find.byKey(const Key('editor-undo')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('clip-s3')), findsNothing);
      await tester.tap(find.byKey(const Key('editor-redo')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('clip-s3')), findsOneWidget);
      expect(find.textContaining('已修改'), findsOneWidget);
    });

    testWidgets('a refused edit says why and changes nothing', (tester) async {
      final engine = await openEditor(tester);
      // At the very start there is nothing to split.
      await tester.tap(find.byKey(const Key('tool-split')));
      await tester.pumpAndSettle();
      expect(find.text('离片段两端太近，无法分割。'), findsOneWidget);
      expect(engine.slots, hasLength(2));
    });

    testWidgets('a selected clip can be deleted and sped up', (tester) async {
      final engine = await openEditor(tester);
      await tester.tap(find.byKey(const Key('clip-s2')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('tool-speed')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('2.0x'));
      await tester.pumpAndSettle();
      expect(engine.ops.last, {'op': 'speed', 'slotId': 's2', 'speed': 2.0});
      expect(engine.slots[1]['durationUs'], 1500000);
      Navigator.of(tester.element(find.byKey(const Key('speed-dial')))).pop();
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('tool-delete')));
      await tester.pumpAndSettle();
      expect(engine.ops.last, {'op': 'delete', 'slotId': 's2'});
      expect(find.byKey(const Key('clip-s2')), findsNothing);
    });

    testWidgets('text goes in at the playhead and is written in its sheet', (tester) async {
      final engine = await openEditor(tester);
      await scrub(tester, -60);
      await tester.tap(find.byKey(const Key('tool-text')));
      await tester.pumpAndSettle();
      expect(engine.ops.last['op'], 'addText');
      expect(find.byKey(const Key('text-words')), findsOneWidget);
      await tester.enterText(find.byKey(const Key('text-words')), '山顶到了');
      await tester.tap(find.byKey(const Key('text-save')));
      await tester.pumpAndSettle();
      expect(engine.ops.last, {'op': 'updateText', 'id': 'txt-3', 'content': '山顶到了'});
      expect(find.byKey(const Key('text-txt-3')), findsOneWidget);
    });

    testWidgets('a join opens its transition', (tester) async {
      final engine = await openEditor(tester);
      await tester.tap(find.byKey(const Key('join-s1')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('transition-xfade')));
      await tester.pumpAndSettle();
      expect(engine.ops.last, {'op': 'transition', 'fromSlotId': 's1', 'kind': 'xfade', 'durationUs': 500000});
      // The dissolve overlaps the clips, so the piece is shorter.
      expect(find.textContaining('/ 00:06.500'), findsOneWidget);
    });

    testWidgets('exports a 剪映 draft and a video, the video as a job', (tester) async {
      final engine = await openEditor(tester);
      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('export-jianying')));
      await tester.pumpAndSettle();
      expect(engine.calls.last.$2['target'], 'jianying');
      expect(find.text('导出完成'), findsOneWidget);
      await tester.tap(find.text('完成'));
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('export-res-720')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('export-video')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pumpAndSettle();
      final export = engine.calls.lastWhere((c) => c.$1 == 'editor.export').$2;
      expect(export['target'], 'video');
      expect(export['shortSide'], 720);
      expect(find.text('导出完成'), findsOneWidget);
      expect(find.textContaining('京都.mp4'), findsWidgets);
    });

    testWidgets('where no video can be written, offers project files and says why',
        (tester) async {
      await tester.binding.setSurfaceSize(const Size(1200, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final engine = EditorEngine()..canRenderVideo = false;
      await tester.pumpWidget(app(EditorPage(
        engine: engine,
        projectId: 'p1',
        projectName: '京都',
        livePlayback: false,
      )));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('export-no-video')), findsOneWidget);
      expect(find.byKey(const Key('export-video')), findsNothing);
      expect(find.byKey(const Key('export-otio')), findsOneWidget);
    });

    testWidgets('a cut of only cuts is offered a lossless copy, saying what it adds',
        (tester) async {
      final engine = await openEditor(tester);
      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      expect(find.text('无损快速导出'), findsOneWidget);
      expect(find.textContaining('一共多出 0.3 秒'), findsOneWidget);
      await tester.tap(find.byKey(const Key('export-lossless')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pumpAndSettle();
      expect(engine.lastTarget, 'lossless');
      expect(find.text('导出完成'), findsOneWidget);
    });

    testWidgets('a cut that must be re-encoded says why, and offers no copy',
        (tester) async {
      await tester.binding.setSurfaceSize(const Size(1200, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final engine = EditorEngine()
        ..lossless = {
          'possible': false,
          'reasons': ['有转场：无损导出只能硬切', '有文字（标题、字幕）'],
        };
      await tester.pumpWidget(app(EditorPage(
        engine: engine,
        projectId: 'p1',
        projectName: '京都',
        livePlayback: false,
      )));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('export-lossless-blocked')), findsOneWidget);
      expect(find.text('· 有转场：无损导出只能硬切'), findsOneWidget);
      expect(find.text('· 有文字（标题、字幕）'), findsOneWidget);
      expect(find.byKey(const Key('export-lossless')), findsNothing);
    });

    testWidgets('each clip can leave as a file of its own', (tester) async {
      final engine = await openEditor(tester);
      await tester.tap(find.byKey(const Key('editor-export')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('export-clips')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pumpAndSettle();
      expect(engine.lastTarget, 'clips');
      expect(find.text('直接拷贝 2 段，重新渲染 0 段'), findsOneWidget);
    });

    testWidgets('lets the session go when it is left', (tester) async {
      final engine = await openEditor(tester);
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      expect(engine.calls.any((c) => c.$1 == 'editor.close'), isTrue);
    });
  });

  group('A project with a cut', () {
    testWidgets('offers to edit it here or export it', (tester) async {
      await tester.binding.setSurfaceSize(const Size(1200, 3000));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final engine = EditorEngine();
      await tester.pumpWidget(app(ProjectPage(
        engine: engine,
        project: Project.fromJson(engine.projects['p1']!),
      )));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('cut-card')), findsOneWidget);
      expect(find.text('在软件内剪辑'), findsOneWidget);
      await tester.tap(find.byKey(const Key('open-editor')));
      await tester.pumpAndSettle();
      expect(find.byType(EditorPage), findsOneWidget);
      expect(engine.calls.any((c) => c.$1 == 'editor.open'), isTrue);
    });
  });

  group('The preview follows the renderer', () {
    test('colours read as the contract writes them', () {
      expect(parseColor('#FF8000'), const Color(0xFFFF8000));
      expect(parseColor('#00000080'), const Color(0x80000000));
      expect(parseColor('red'), isNull);
      expect(colorText(const Color(0xFFFFE066)), '#FFE066');
      expect(colorText(const Color(0x99000000)), '#00000099');
    });

    test('a neutral adjustment changes nothing, and saturation keeps grey', () {
      expect(adjustMatrix(const Adjustment()), [
        1, 0, 0, 0, 0, //
        0, 1, 0, 0, 0, //
        0, 0, 1, 0, 0, //
        0, 0, 0, 1, 0,
      ]);
      final grey = adjustMatrix(const Adjustment(saturation: 0));
      // Every channel of a desaturated pixel is its Rec. 709 luma.
      for (var row = 0; row < 3; row++) {
        expect(grey[row * 5], closeTo(0.2126, 1e-9));
        expect(grey[row * 5 + 1], closeTo(0.7152, 1e-9));
        expect(grey[row * 5 + 2], closeTo(0.0722, 1e-9));
      }
    });

    test('a subtitle sits above the bottom margin, as the renderer puts it', () {
      const look = TextLook(alignment: 'bottomCenter');
      final at = placeText(look, const Size(400, 60), const Size(1920, 1080));
      expect(at.dx, 760);
      expect(at.dy, closeTo(1080 - 0.08 * 1080 - 60, 1e-9));
      const placed = TextLook(x: 0.25, y: 0.2);
      final there = placeText(placed, const Size(100, 40), const Size(1000, 1000));
      expect(there, const Offset(200, 180));
    });

    test('a portrait picture is fitted into a landscape frame, then placed', () {
      final fitted = placePicture(const Size(1080, 1920), const Size(1920, 1080), Placement.fitted);
      expect(fitted.size.height, 1080);
      expect(fitted.size.width, closeTo(607.5, 1e-9));
      final moved = placePicture(
        const Size(1920, 1080),
        const Size(1920, 1080),
        const Placement(scale: 1.5, x: 0.1, y: -0.2, rotationDeg: 90),
      );
      expect(moved.size, const Size(2880, 1620));
      expect(moved.centre, const Offset(960 + 192, 540 - 216));
      expect(moved.radians, closeTo(1.5707963, 1e-6));
      expect(covers(const Size(1080, 1920), const Size(1920, 1080), Placement.fitted), isFalse);
      expect(covers(const Size(3840, 2160), const Size(1920, 1080), Placement.fitted), isTrue);
    });

    test('transitions blend as the renderer blends them', () {
      expect(blendOpacities(TransitionKind.xfade, 0.25), (1.0, 0.25));
      expect(blendOpacities(TransitionKind.fadeToBlack, 0.25), (0.5, 0.0));
      expect(blendOpacities(TransitionKind.fadeToBlack, 0.75), (0.0, 0.5));
      expect(linearGain(-6), closeTo(0.501, 1e-3));
      expect(linearGain(6), 1.0);
      expect(linearGain(0, muted: true), 0);
    });
  });

  test('the engine\'s answer reads into the editor\'s state', () {
    final engine = EditorEngine();
    engine.transitions.add({'fromSlotId': 's1', 'toSlotId': 's2', 'type': 'fadeToBlack', 'durationUs': 1000000});
    final state = EditState.fromJson(engine._stateForTest());
    expect(state.clips.map((c) => c.id), ['s1', 's2']);
    expect(state.clips[1].startUs, 3000000);
    expect(state.clips[1].overlapUs, 1000000);
    expect(state.totalUs, 6000000);
    expect(state.joinAfter('s1')!.kind, TransitionKind.fadeToBlack);
    expect(state.clipAt(3500000)!.id, 's2');
    expect(state.clipsAt(3500000).map((c) => c.id), ['s1', 's2']);
    expect(state.clips[0].sourceTimeAt(1000000), 3000000);
    expect(state.frameAspect, closeTo(16 / 9, 1e-9));
  });
}

extension on EditorEngine {
  Map<String, dynamic> _stateForTest() => _state();
}

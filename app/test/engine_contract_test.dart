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

import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/src/engine/engine.dart';
import 'package:video_butler/src/models.dart';

/// A stand-in engine, so UI behaviour can be tested without a Rust build.
class FakeEngine implements Engine {
  FakeEngine({this.failWith});

  final String? failWith;
  int runCalls = 0;
  List<String>? lastInputs;
  String? lastTemplate;

  @override
  String get transportLabel => 'fake';

  @override
  Future<EngineVersion> version() async => const EngineVersion(
        version: '0.1.0',
        footageGraph: 'footage-graph/0.1',
        editingDsl: 'editing-dsl/0.1',
      );

  @override
  Future<DoctorReport> doctor() async => const DoctorReport(
        mediaBackend: 'FakeMedia',
        mediaAvailable: true,
        mediaVersion: '1.0',
        mediaNote: '',
        adapters: [
          AdapterInfo(role: 'Speech', name: 'MockAsr', mock: true),
        ],
        canAnalyse: true,
      );

  @override
  Future<List<EditTemplate>> templates() async => const [];

  @override
  Future<DirectoryListing> browse(String? directory, {String kind = 'video'}) async {
    lastBrowseKind = kind;
    return DirectoryListing(
      path: browseEntries.isEmpty ? '/' : '/trip',
      parent: null,
      entries: browseEntries,
      error: null,
    );
  }

  String? lastBrowseKind;

  /// What the picker finds in the folder it opens, and what the engine reads
  /// of each clip there, by path.
  List<DirectoryEntry> browseEntries = const [];
  Map<String, Map<String, dynamic>> media = {};

  /// The language the UI last saved, as the engine would keep it.
  String language = 'zh';

  /// How editor drafts reach the footage, as the engine would keep it.
  String draftMedia = 'link';

  /// Whether runs slow down on a hot device, as the engine would keep it.
  bool thermalGuard = true;

  /// Whether the project's music ends before its cut does.
  bool musicShort = false;

  /// Earlier analysis the library holds.
  int cachedEntries = 12;

  // --- the video butler ------------------------------------------------------

  bool hostingEnabled = false;
  bool hostingFinished = false;

  /// The butler's folders, the disks plugged in, and what stands in the way.
  List<String> hostingRoots = const ['/Users/me/Movies'];
  List<Map<String, String>> volumes = const [];
  List<String> hostingWarnings = const [];
  final Set<String> removedCategories = {};

  /// Photos the butler filed, overall and per category.
  int hostingPhotos = 0;
  Map<String, int> categoryPhotos = const {};

  List<Map<String, dynamic>> catalogVideos = [
    {
      'path': '/trip/V31001-181040.mp4',
      'name': 'V31001-181040.mp4',
      'capturedAt': '2023-10-01T10:10:40Z',
      'capturedFrom': 'camera',
      'durationUs': 20000000,
      'width': 1920,
      'height': 1080,
      'place': {'name': '新疆', 'lat': 43.3, 'lon': 87.4},
      'categories': ['scenery'],
      'subjects': <String>[],
    },
    {
      'path': '/trip/V31003-124026.mp4',
      'name': 'V31003-124026.mp4',
      'capturedAt': '2023-10-03T04:40:26Z',
      'capturedFrom': 'camera',
      'durationUs': 31000000,
      'width': 1920,
      'height': 1080,
      'categories': ['scenery', 'animals'],
      'subjects': <String>[],
    },
  ];

  Map<String, dynamic> hosting() => {
        'enabled': hostingEnabled,
        'roots': hostingRoots,
        'volumes': volumes,
        'warnings': hostingWarnings,
        'customRoots': false,
        'videos': 228,
        'photos': hostingPhotos,
        'scannedAt': '2026-10-07T01:00:00Z',
        'memories': memories.length,
        'categories': [
          for (final (id, name, count) in [
            ('study', '学习', 1),
            ('livestream', '直播切片', 0),
            ('scenery', '风景', 136),
            ('animals', '动物', 6),
          ])
            if (!removedCategories.contains(id))
              {
                'id': id,
                'name': name,
                'count': count,
                'photos': categoryPhotos[id] ?? 0,
                'samples': count == 0 ? <String>[] : ['/trip/V31001-181040.mp4'],
                'unverified': id == 'study' || id == 'livestream',
              },
        ],
        'removed': [
          for (final id in removedCategories) {'id': id, 'name': id == 'scenery' ? '风景' : id},
        ],
        'people': [
          {'id': 'sub-0002', 'label': null, 'thumbnail': null, 'videos': 24},
        ],
        'notes': ['分类依据：每个视频取三帧，用 Apple Vision 的场景标签按规则归类。'],
      };

  List<Map<String, dynamic>> memories = [
    {
      'id': 'travel-20230929-CN-XJ',
      'kind': 'travel',
      'title': '2023年9月到新疆的旅行回忆',
      'start': '2023-09-29',
      'end': '2023-10-08',
      'year': 2023,
      'month': 9,
      'label': '2023.09',
      'place': '新疆',
      'pin': {'lat': 43.2, 'lon': 84.0},
      'regions': ['CN-HB', 'CN-XJ'],
      'points': [
        {'lat': 43.3, 'lon': 87.4},
      ],
      'videoCount': 183,
      'cover': '/trip/V31001-181040.mp4',
      'people': <Map<String, dynamic>>[],
      'projectId': 'p1',
      'projectRun': true,
      'status': 'done',
    },
    {
      'id': 'annual-2026',
      'kind': 'annual',
      'title': '2026年度回忆',
      'start': '2026-02-14',
      'end': '2026-09-24',
      'year': 2026,
      'month': 2,
      'label': '2026.02',
      'pin': {'lat': 30.5, 'lon': 114.3},
      'regions': ['CN-HB'],
      'points': <Map<String, dynamic>>[],
      'videoCount': 35,
      'people': <Map<String, dynamic>>[],
      'order': 'category',
      'projectRun': false,
      'status': 'pending',
    },
    {
      'id': 'growth-sub-0063',
      'kind': 'growth',
      'title': 'TA的成长记忆',
      'start': '2024-11-02',
      'end': '2026-09-24',
      'year': 2024,
      'month': 11,
      'label': '2024.11',
      'regions': <String>[],
      'points': <Map<String, dynamic>>[],
      'videoCount': 23,
      'people': [
        {'id': 'sub-0063', 'label': null, 'thumbnail': null, 'videos': 7},
      ],
      'projectRun': false,
    },
  ];

  // --- an in-memory engine behind `call` ------------------------------------

  /// Every call made, in order, for tests that check what was sent.
  final List<(String, Map<String, dynamic>)> calls = [];
  final Map<String, Map<String, dynamic>> projects = {};
  int _nextProject = 1;

  /// Statuses a run reports, one per poll, the last repeated.
  List<Map<String, dynamic>> runScript = [
    {
      'jobId': 'job-1',
      'state': 'running',
      'stage': 'analyse',
      'stageLabel': 'Watching the footage',
      'percent': 42.0,
      'detail': 'clip-a.mp4 · keyframes',
      'step': 'analyse.keyframes',
      'file': 'clip-a.mp4',
      'sourceIndex': 1,
      'sourceCount': 3,
      'activeSources': 2,
      'stages': _stages,
    },
    {
      'jobId': 'job-1',
      'state': 'done',
      'stage': 'done',
      'stageLabel': 'Done',
      'percent': 100.0,
      'detail': 'done',
      'sourceIndex': 0,
      'sourceCount': 3,
      'stages': _stages,
      'result': {
        'draftPath': '/lib/projects/p1/project.otio',
        'templateId': 'style.happy.travel.beat',
        'templateName': 'Happy and bright, Travel diary, on the beat',
        'durationUs': 30000000,
        'clipsUsed': 8,
        'totalSources': 3,
        'editorDrafts': [
          {
            'editor': 'jianying',
            'editorName': '剪映专业版',
            'name': 'Kyoto',
            'path': '/drafts/Kyoto',
            'clips': 8,
            'hasMusic': true,
            'transitionsDropped': 0,
          },
        ],
        'beat': {'bpm': 120.0, 'origin': 'music', 'confidence': 0.8},
        'adapterNotes': ['Vision: Apple Vision on this device, as scene labels.'],
        'warnings': [],
        'shots': [],
        'slots': [],
      },
    },
  ];
  int _polls = 0;

  static const _stages = [
    {'id': 'prepare', 'label': 'Gathering the footage'},
    {'id': 'analyse', 'label': 'Watching the footage'},
    {'id': 'music', 'label': 'Listening for the beat'},
    {'id': 'compose', 'label': 'Choosing the cut'},
    {'id': 'export', 'label': 'Writing the draft'},
  ];

  Map<String, dynamic> _project(String name) {
    final id = 'p${_nextProject++}';
    return {
      'id': id,
      'name': name,
      'inputs': <String>[],
      'style': <String, dynamic>{},
      'model': 'local',
      'editors': <String>[],
      'createdAt': '2026-09-21T00:00:00Z',
      'updatedAt': '2026-09-21T00:00:00Z',
      'dir': '/lib/projects/$id',
    };
  }

  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    calls.add((method, args));
    switch (method) {
      case 'selftest':
        return {
          'status': 'warn',
          'canAnalyse': true,
          'checks': [
            {'id': 'media', 'label': 'Video engine', 'status': 'pass', 'detail': 'LibAv · ffmpeg 9.0.1'},
            {'id': 'editor.jianying', 'label': '剪映专业版 draft export', 'status': 'pass', 'detail': 'drafts go to /drafts'},
            {'id': 'speech', 'label': 'Speech model', 'status': 'warn', 'detail': 'None downloaded'},
          ],
        };
      case 'projects.list':
        return {'projects': projects.values.toList()};
      case 'projects.create':
        final project = _project(args['name'] as String);
        projects[project['id'] as String] = project;
        return project;
      case 'projects.update':
        final project = projects[args['id']]!;
        for (final key in ['name', 'inputs', 'style', 'model', 'editors']) {
          if (args.containsKey(key)) project[key] = args[key];
        }
        return project;
      case 'projects.delete':
        projects.remove(args['id']);
        return {'deleted': args['id']};
      case 'run.start':
        _polls = 0;
        return {'jobId': 'job-1'};
      case 'run.preflight':
        final style = projects[args['projectId']]?['style'] as Map<String, dynamic>? ?? {};
        return {
          'cutDurationUs': 300000000,
          'musicDurationUs': 155000000,
          'musicShort': musicShort,
          'musicLoop': style['musicLoop'],
          'askMusicLoop': musicShort && style['musicLoop'] == null,
        };
      case 'cache.info':
        return {'entries': cachedEntries, 'bytes': cachedEntries * 780000};
      case 'cache.clear':
        final cleared = {'cleared': cachedEntries, 'bytes': cachedEntries * 780000};
        cachedEntries = 0;
        return cleared;
      case 'run.status' when args['jobId'] == 'job-hosting':
        return {
          'jobId': 'job-hosting',
          'state': hostingFinished ? 'done' : 'running',
          'stage': 'analyse',
          'stageLabel': 'Watching the footage',
          'percent': 37.0,
          'detail': '',
          'step': 'hosting.look',
          'file': 'V31001-181040.mp4',
          'sourceIndex': 41,
          'sourceCount': 230,
          'stages': _stages,
        };
      case 'run.status':
        final status = runScript[_polls.clamp(0, runScript.length - 1)];
        _polls++;
        return status;
      case 'media.inspect':
        return {
          'items': [
            for (final path in (args['paths'] as List<dynamic>).cast<String>())
              media[path] ?? {'path': path, 'error': 'unreadable'},
          ],
        };
      case 'media.thumbnails':
        // A one-pixel PNG for each clip the fake knows.
        return {
          'items': [
            for (final path in (args['paths'] as List<dynamic>).cast<String>())
              if (media.containsKey(path))
                {
                  'path': path,
                  'png': 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
                }
              else
                {'path': path, 'error': 'unreadable'},
          ],
        };
      case 'music.inspect':
        return {
          'path': args['path'],
          'durationUs': 184000000,
          'bpm': 118.0,
          'confidence': 0.8,
          'steady': true,
        };
      case 'run.cancel':
        return {'cancelling': args['jobId']};
      case 'models.list':
        return {
          'directory': '/lib/models',
          'models': [
            {
              'id': 'whisper-base-q5',
              'name': 'Whisper base (quantised)',
              'role': 'speech',
              'roleLabel': 'Speech',
              'bytes': 59707625,
              'license': 'MIT',
              'note': 'Small and quick.',
              'memoryMb': 500,
              'installed': false,
              'path': '/lib/models/ggml-base-q5_1.bin',
            },
          ],
        };
      case 'settings.get':
      case 'settings.update':
        if (args['language'] != null) language = args['language'] as String;
        if (args['draftMedia'] != null) draftMedia = args['draftMedia'] as String;
        if (args['thermalGuard'] != null) thermalGuard = args['thermalGuard'] as bool;
        return {
          'language': language,
          'draftMedia': draftMedia,
          'thermalGuard': thermalGuard,
          'cloud': {
            'baseUrl': 'https://api.example.com/v1',
            'model': 'some-model',
            'visionModel': '',
            'apiKeySet': true,
            'configured': true,
          },
          'defaultEditors': <String>[],
          'libraryPath': '/lib',
        };
      case 'entities.list':
        return {
          'subjects': [
            {
              'id': 'sub-0001',
              'label': 'Mum',
              'kind': 'person',
              'thumbnail': null,
              'clips': 3,
              'shots': 7,
              'appearances': [
                {'projectId': 'p1', 'projectName': '京都', 'sourcePath': '/movies/kyoto/a.mp4', 'shots': 4},
                {'projectId': 'p1', 'projectName': '京都', 'sourcePath': '/movies/kyoto/b.mp4', 'shots': 2},
                {'projectId': 'p9', 'projectName': 'Gone', 'sourcePath': '/movies/c.mp4', 'shots': 1},
              ],
            },
          ],
          'grouping': 'Subjects are grouped by how alike they look, not recognised.',
        };
      case 'hosting.get':
        return hosting();
      case 'hosting.start':
        hostingEnabled = true;
        return {'jobId': 'job-hosting'};
      case 'hosting.update':
        if (args['enabled'] != null) hostingEnabled = args['enabled'] as bool;
        if (args['roots'] != null) {
          hostingRoots = (args['roots'] as List<dynamic>).cast<String>();
        }
        return hosting();
      case 'hosting.removeCategory':
        removedCategories.add(args['id'] as String);
        return hosting();
      case 'hosting.restoreCategory':
        removedCategories.remove(args['id'] as String);
        return hosting();
      case 'hosting.category':
      case 'hosting.person':
        return {'videos': catalogVideos};
      case 'memories.list':
        return {
          'memories': memories,
          'regions': ['CN-XJ', 'CN-HB'],
          'notes': ['Places come from the GPS fix.'],
        };
      case 'memories.get':
        final m = memories.firstWhere((m) => m['id'] == args['id']);
        return {...m, 'videos': catalogVideos};
      case 'memories.update':
      case 'memories.generate':
        return {'jobId': 'job-memory'};
      case 'geo.world':
        return {
          'scale': 100,
          'regions': [
            {
              'id': 'CN',
              'kind': 'country',
              'zh': '中国',
              'en': 'China',
              'label': [10500, 3500],
              'rings': [
                [7400, 1800, 13500, 1800, 13500, 5300, 7400, 5300],
              ],
            },
            {
              'id': 'CN-XJ',
              'kind': 'province',
              'parent': 'CN',
              'zh': '新疆',
              'en': 'Xinjiang',
              'label': [8500, 4200],
              'rings': [
                [7400, 3500, 9600, 3500, 9600, 4900, 7400, 4900],
              ],
            },
          ],
        };
      case 'entities.merge':
        return <String, dynamic>{};
      case 'editors.list':
        return [
          {
            'id': 'jianying',
            'name': '剪映专业版',
            'installed': true,
            'version': '11.4.0',
            'draftRoot': '/drafts',
            'canExport': true,
            'running': false,
          },
          {
            'id': 'capcut',
            'name': 'CapCut',
            'installed': false,
            'canExport': false,
            'running': false,
          },
        ];
      default:
        return <String, dynamic>{};
    }
  }

  @override
  Future<RunResult> run({
    required List<String> inputs,
    String? projectDir,
    String? templateId,
  }) async {
    runCalls++;
    lastInputs = inputs;
    lastTemplate = templateId;
    if (failWith != null) throw EngineException(failWith!);
    return RunResult.fromJson(const {'templateId': 'x'});
  }

  @override
  void dispose() {}
}

void main() {
  test('an engine failure surfaces as a readable message', () async {
    final engine = FakeEngine(failWith: 'ffmpeg was not found');
    expect(
      () => engine.run(inputs: const ['/a.mp4']),
      throwsA(isA<EngineException>().having(
        (e) => e.message,
        'message',
        contains('ffmpeg'),
      )),
    );
  });

  test('a null template means the engine chooses one', () async {
    // "Automatic" is the default in the UI, and it has to reach the engine as
    // an absent template rather than as a literal string.
    final engine = FakeEngine();
    await engine.run(inputs: const ['/a.mp4']);
    expect(engine.lastTemplate, isNull);
  });

  test('a chosen template is passed through unchanged', () async {
    final engine = FakeEngine();
    await engine.run(inputs: const ['/a.mp4'], templateId: 'rhythm.dialogue-led');
    expect(engine.lastTemplate, 'rhythm.dialogue-led');
  });

  test('a project of several videos reaches the engine as a list', () async {
    // The engine expands folders, so what the user picked is what it is told:
    // two clips and a folder stay two clips and a folder.
    final engine = FakeEngine();
    await engine.run(inputs: const ['/a.mp4', '/b.mp4', '/holiday']);
    expect(engine.lastInputs, ['/a.mp4', '/b.mp4', '/holiday']);
  });
}

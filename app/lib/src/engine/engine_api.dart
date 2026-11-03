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

/// Typed methods over [Engine.call].
///
/// The engine exposes one dispatcher; this is where its method names and
/// argument shapes live on the Dart side, so a page never builds a method
/// name by hand and a typo is a compile error rather than an "unknown
/// method" at run time.
library;

import 'dart:convert';
import 'dart:typed_data';

import '../api_models.dart';
import '../editor/edit_state.dart';
import '../models.dart';
import 'engine.dart';

extension EngineApi on Engine {
  /// The self-test, worded in [language] when given, so it matches the UI
  /// even before the choice has been saved.
  Future<SelfTest> selfTest({String? language}) async => SelfTest.fromJson(
        await call('selftest', {if (language != null) 'lang': language})
            as Map<String, dynamic>,
      );

  /// Save the UI language. The engine writes its own sentences in it.
  Future<void> saveLanguage(String language) =>
      call('settings.update', {'language': language});

  /// How long a piece of music is and how fast, before a run uses it.
  Future<MusicInfo> inspectMusic(String path) async => MusicInfo.fromJson(
        await call('music.inspect', {'path': path}) as Map<String, dynamic>,
      );

  Future<List<EditorInfo>> editors() async =>
      (await call('editors.list') as List<dynamic>)
          .map((e) => EditorInfo.fromJson(e as Map<String, dynamic>))
          .toList(growable: false);

  Future<void> openEditor(String id) => call('editors.open', {'id': id});

  /// When and how each video was filmed, by path, for choosing among them.
  Future<Map<String, MediaDetails>> inspectMedia(List<String> paths) async {
    if (paths.isEmpty) return const {};
    final data =
        await call('media.inspect', {'paths': paths}) as Map<String, dynamic>;
    return {
      for (final item in data['items'] as List<dynamic>? ?? const [])
        (item as Map<String, dynamic>)['path'] as String:
            MediaDetails.fromJson(item),
    };
  }

  /// A small upright picture of each video, by path, for the picker to
  /// show before anything is chosen. Videos that cannot be read are left
  /// out.
  Future<Map<String, Uint8List>> thumbnails(List<String> paths, {int width = 200}) async {
    if (paths.isEmpty) return const {};
    final data = await call('media.thumbnails', {'paths': paths, 'width': width})
        as Map<String, dynamic>;
    return {
      for (final item in data['items'] as List<dynamic>? ?? const [])
        if ((item as Map<String, dynamic>)['png'] is String)
          item['path'] as String: base64Decode(item['png'] as String),
    };
  }

  /// Open a file or folder in the desktop's file manager.
  Future<void> reveal(String path) => call('reveal', {'path': path});

  // --- projects ------------------------------------------------------------

  Future<List<Project>> projects() async {
    final data = await call('projects.list') as Map<String, dynamic>;
    return (data['projects'] as List<dynamic>? ?? [])
        .map((p) => Project.fromJson(p as Map<String, dynamic>))
        .toList(growable: false);
  }

  Future<Project> createProject(String name) async => Project.fromJson(
        await call('projects.create', {'name': name}) as Map<String, dynamic>,
      );

  Future<Project> updateProject(
    String id, {
    String? name,
    List<String>? inputs,
    StyleChoice? style,
    ModelChoice? model,
    List<String>? editors,
  }) async =>
      Project.fromJson(
        await call('projects.update', {
          'id': id,
          if (name != null) 'name': name,
          if (inputs != null) 'inputs': inputs,
          if (style != null) 'style': style.toJson(),
          if (model != null) 'model': model.id,
          if (editors != null) 'editors': editors,
        }) as Map<String, dynamic>,
      );

  Future<void> deleteProject(String id) => call('projects.delete', {'id': id});

  // --- runs ----------------------------------------------------------------

  /// Start a run of a saved project. Returns the job id to poll. [fresh]
  /// looks at the footage again rather than reusing earlier analysis.
  Future<String> startRun(String projectId, {bool fresh = false}) async {
    final data = await call('run.start', {
      'projectId': projectId,
      if (fresh) 'fresh': true,
    }) as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// What a run of this project is about to do that the user should decide
  /// first: today, music shorter than the cut.
  Future<RunPreflight> preflight(String projectId) async =>
      RunPreflight.fromJson(
        await call('run.preflight', {'projectId': projectId})
            as Map<String, dynamic>,
      );

  /// How much earlier analysis the library keeps.
  Future<CacheInfo> cacheInfo() async =>
      CacheInfo.fromJson(await call('cache.info') as Map<String, dynamic>);

  /// Forget every earlier analysis; the next run looks at everything again.
  Future<CacheInfo> clearCache() async {
    final data = await call('cache.clear') as Map<String, dynamic>;
    return CacheInfo(
      entries: (data['cleared'] as num?)?.toInt() ?? 0,
      bytes: (data['bytes'] as num?)?.toInt() ?? 0,
    );
  }

  /// Save how editor drafts point at footage: `link` or `reference`.
  Future<AppSettings> saveDraftMedia(String mode) async => AppSettings.fromJson(
        await call('settings.update', {'draftMedia': mode})
            as Map<String, dynamic>,
      );

  Future<AppSettings> saveThermalGuard(bool on) async => AppSettings.fromJson(
        await call('settings.update', {'thermalGuard': on}) as Map<String, dynamic>,
      );

  Future<JobStatus> jobStatus(String jobId) async => JobStatus.fromJson(
        await call('run.status', {'jobId': jobId}) as Map<String, dynamic>,
      );

  Future<void> cancelJob(String jobId) => call('run.cancel', {'jobId': jobId});

  // --- models --------------------------------------------------------------

  Future<ModelLibrary> models() async =>
      ModelLibrary.fromJson(await call('models.list') as Map<String, dynamic>);

  /// Start a download. Returns the job id, or null when the model is already
  /// installed.
  Future<String?> downloadModel(String id) async {
    final data =
        await call('models.download', {'id': id}) as Map<String, dynamic>;
    return data['jobId'] as String?;
  }

  Future<void> deleteModel(String id) => call('models.delete', {'id': id});

  /// Run this installed model for its role from now on, in place of the
  /// most capable one. Returns the library, which marks it in use.
  Future<ModelLibrary> useModel(String id) async => ModelLibrary.fromJson(
        await call('models.use', {'id': id}) as Map<String, dynamic>,
      );

  /// Copy the models in a folder -- a model pack -- into the library.
  /// Returns the job id to follow.
  Future<String> importModels(String dir) async {
    final data = await call('models.import', {'dir': dir}) as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  // --- settings ------------------------------------------------------------

  Future<AppSettings> settings() async =>
      AppSettings.fromJson(await call('settings.get') as Map<String, dynamic>);

  /// Save the cloud endpoint. [apiKey] null leaves the stored key alone,
  /// which is what a form that never saw the key must send.
  Future<AppSettings> saveCloud({
    required String baseUrl,
    required String model,
    required String visionModel,
    String? apiKey,
  }) async =>
      AppSettings.fromJson(
        await call('settings.update', {
          'cloud': {
            'baseUrl': baseUrl,
            'model': model,
            'visionModel': visionModel,
            if (apiKey != null) 'apiKey': apiKey,
          },
        }) as Map<String, dynamic>,
      );

  /// Whether the configured endpoint answers, and which models it offers.
  Future<Map<String, dynamic>> checkCloud() async =>
      await call('cloud.check') as Map<String, dynamic>;

  // --- the album -----------------------------------------------------------

  Future<Album> album() async =>
      Album.fromJson(await call('entities.list') as Map<String, dynamic>);

  Future<void> renameSubject(String id, String? label) =>
      call('entities.rename', {'id': id, 'label': label});

  Future<void> forgetSubject(String id) =>
      call('entities.forget', {'id': id});

  /// File one person under another: two groups of faces the user knows are
  /// one person.
  Future<void> mergeSubject(String id, String into) =>
      call('entities.merge', {'id': id, 'into': into});

  // --- the video butler ------------------------------------------------------

  Future<HostingOverview> hosting() async =>
      HostingOverview.fromJson(await call('hosting.get') as Map<String, dynamic>);

  /// Hand the footage to the butler: look through every folder, file each
  /// video, find the memories and cut them. Returns the job to follow; a
  /// pass already running is that pass.
  Future<String> startHosting({List<String>? roots}) async {
    final data = await call('hosting.start', {if (roots != null) 'roots': roots})
        as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// Switch hosting on or off, or change the folders kept (empty: the
  /// usual ones).
  Future<HostingOverview> updateHosting({bool? enabled, List<String>? roots}) async =>
      HostingOverview.fromJson(
        await call('hosting.update', {
          if (enabled != null) 'enabled': enabled,
          if (roots != null) 'roots': roots,
        }) as Map<String, dynamic>,
      );

  Future<List<CatalogVideo>> categoryVideos(String id) async => _videos(
        await call('hosting.category', {'id': id}) as Map<String, dynamic>,
      );

  Future<List<CatalogVideo>> personVideos(String id) async => _videos(
        await call('hosting.person', {'id': id}) as Map<String, dynamic>,
      );

  /// Stop filing videos under a category; it can be brought back.
  Future<HostingOverview> removeCategory(String id) async => HostingOverview.fromJson(
        await call('hosting.removeCategory', {'id': id}) as Map<String, dynamic>,
      );

  Future<HostingOverview> restoreCategory(String id) async => HostingOverview.fromJson(
        await call('hosting.restoreCategory', {'id': id}) as Map<String, dynamic>,
      );

  Future<MemoryList> memories({MemoryKind? kind}) async => MemoryList.fromJson(
        await call('memories.list', {if (kind != null) 'kind': kind.id})
            as Map<String, dynamic>,
      );

  Future<Memory> memory(String id) async =>
      Memory.fromJson(await call('memories.get', {'id': id}) as Map<String, dynamic>);

  /// Set a year's order (`time` or `category`) and cut it again. Returns
  /// the job to follow.
  Future<String> reorderMemory(String id, String order) async {
    final data = await call('memories.update', {'id': id, 'order': order})
        as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// Make a memory's project again and cut it. Returns the job to follow.
  Future<String> generateMemory(String id) async {
    final data = await call('memories.generate', {'id': id}) as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// The world map, as the engine locates places on it.
  Future<WorldMap> world() async =>
      WorldMap.fromJson(await call('geo.world') as Map<String, dynamic>);

  // --- the built-in editor ---------------------------------------------------

  /// Open a project's cut in the built-in editor: the person's edit when
  /// there is one, the last run's cut otherwise. [restart] throws the edit
  /// away and starts again from the last run.
  Future<EditState> editOpen(String projectId, {bool restart = false}) async =>
      EditState.fromJson(await call('editor.open', {
        'projectId': projectId,
        if (restart) 'restart': true,
      }) as Map<String, dynamic>);

  /// One change to the cut, as a step that can be undone. [op] is a
  /// `vb-edit` operation: `{'op': 'split', 'atUs': ...}` and the like.
  Future<EditState> editApply(String projectId, Map<String, dynamic> op) async =>
      EditState.fromJson(await call('editor.apply', {'projectId': projectId, 'op': op})
          as Map<String, dynamic>);

  Future<EditState> editUndo(String projectId) async => EditState.fromJson(
      await call('editor.undo', {'projectId': projectId}) as Map<String, dynamic>);

  Future<EditState> editRedo(String projectId) async => EditState.fromJson(
      await call('editor.redo', {'projectId': projectId}) as Map<String, dynamic>);

  /// Back to the cut the AI made, as a step that can be undone.
  Future<EditState> editRevert(String projectId) async => EditState.fromJson(
      await call('editor.revert', {'projectId': projectId}) as Map<String, dynamic>);

  /// The piece at one moment exactly as an exported video shows it, as PNG
  /// bytes, with the cut's version it was drawn from.
  Future<(Uint8List, int)> editFrame(String projectId, int atUs, {int maxSide = 640}) async {
    final data = await call('editor.frame', {
      'projectId': projectId,
      'atUs': atUs,
      'maxSide': maxSide,
    }) as Map<String, dynamic>;
    return (base64Decode(data['png'] as String), (data['version'] as num?)?.toInt() ?? 0);
  }

  /// Pictures of the footage for the timeline's film strip, by
  /// `sourceId@atUs` as asked.
  Future<Map<String, Uint8List>> editStrip(
    String projectId,
    List<(String, int)> items, {
    int height = 64,
  }) async {
    if (items.isEmpty) return const {};
    final data = await call('editor.strip', {
      'projectId': projectId,
      'height': height,
      'items': [
        for (final (source, at) in items) {'sourceId': source, 'atUs': at},
      ],
    }) as Map<String, dynamic>;
    final out = <String, Uint8List>{};
    final list = data['items'] as List<dynamic>? ?? const [];
    for (var i = 0; i < list.length && i < items.length; i++) {
      final item = list[i] as Map<String, dynamic>;
      if (item['png'] is String) {
        out['${items[i].$1}@${items[i].$2}'] = base64Decode(item['png'] as String);
      }
    }
    return out;
  }

  /// The project's footage, to add clips from.
  Future<List<FootageFile>> editFootage(String projectId) async {
    final data = await call('editor.footage', {'projectId': projectId}) as Map<String, dynamic>;
    return [
      for (final s in data['sources'] as List<dynamic>? ?? const [])
        FootageFile.fromJson(s as Map<String, dynamic>),
    ];
  }

  /// Write the cut as a video. Returns the job to follow; its result is
  /// an [ExportDone].
  Future<String> editExportVideo(
    String projectId, {
    int shortSide = 1080,
    double? fps,
    String codec = 'h264',
    String? dir,
  }) async {
    final data = await call('editor.export', {
      'projectId': projectId,
      'target': 'video',
      'shortSide': shortSide,
      if (fps != null) 'fps': fps,
      'codec': codec,
      if (dir != null) 'dir': dir,
    }) as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// Whether the cut can leave without being re-encoded, and why not.
  Future<LosslessCheck> editLossless(String projectId) async => LosslessCheck.fromJson(
      await call('editor.lossless', {'projectId': projectId}) as Map<String, dynamic>);

  /// Write the cut by copying its footage, as filmed: a job.
  Future<String> editExportLossless(String projectId) async {
    final data = await call('editor.export', {'projectId': projectId, 'target': 'lossless'})
        as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// Write each clip as a file of its own: a job.
  Future<String> editExportClips(String projectId) async {
    final data = await call('editor.export', {'projectId': projectId, 'target': 'clips'})
        as Map<String, dynamic>;
    return data['jobId'] as String;
  }

  /// Write the cut as a project file another editor opens: `jianying`,
  /// `capcut` or `otio`.
  Future<ExportDone> editExportDraft(String projectId, String target) async =>
      ExportDone.fromJson(await call('editor.export', {
        'projectId': projectId,
        'target': target,
      }) as Map<String, dynamic>);

  /// Let go of the session: everything is on disk already.
  Future<void> editClose(String projectId) =>
      call('editor.close', {'projectId': projectId});

  /// A project of these videos, saved, ready to open.
  Future<Project> projectOf(String name, List<String> inputs, {StyleChoice? style}) async {
    final project = await createProject(name);
    return updateProject(project.id, inputs: inputs, style: style);
  }
}

List<CatalogVideo> _videos(Map<String, dynamic> data) =>
    (data['videos'] as List<dynamic>? ?? [])
        .map((v) => CatalogVideo.fromJson(v as Map<String, dynamic>))
        .toList(growable: false);

/// The run result the job hands back when it finishes.
RunResult runResultOf(JobStatus status) =>
    RunResult.fromJson(status.result ?? const {});

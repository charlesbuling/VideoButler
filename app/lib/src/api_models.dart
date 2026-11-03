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

/// Data for the operations reached through `Engine.call`: the self-test,
/// projects, run jobs, the model library, settings, the album and editors.
///
/// Parsers are forgiving about missing keys, as in `models.dart`: the UI
/// should degrade rather than crash when the engine gains a field the app has
/// not learned about yet.
library;

/// One line of the start-up self-test.
class CheckResult {
  const CheckResult({
    required this.id,
    required this.label,
    required this.status,
    required this.detail,
  });

  final String id;
  final String label;

  /// `pass`, `warn` or `fail`. `fail` means a run cannot produce a result;
  /// `warn` means it will, with something missing.
  final String status;
  final String detail;

  bool get passed => status == 'pass';
  bool get failed => status == 'fail';

  factory CheckResult.fromJson(Map<String, dynamic> json) => CheckResult(
        id: json['id'] as String? ?? '',
        label: json['label'] as String? ?? '',
        status: json['status'] as String? ?? 'warn',
        detail: json['detail'] as String? ?? '',
      );
}

class SelfTest {
  const SelfTest({
    required this.status,
    required this.canAnalyse,
    required this.checks,
  });

  final String status;
  final bool canAnalyse;
  final List<CheckResult> checks;

  factory SelfTest.fromJson(Map<String, dynamic> json) => SelfTest(
        status: json['status'] as String? ?? 'warn',
        canAnalyse: json['canAnalyse'] as bool? ?? false,
        checks: (json['checks'] as List<dynamic>? ?? [])
            .map((c) => CheckResult.fromJson(c as Map<String, dynamic>))
            .toList(growable: false),
      );
}

/// The emotional register of a cut. Ids match the engine's.
enum Mood {
  happy('happy'),
  sad('sad'),
  humor('humor');

  const Mood(this.id);
  final String id;

  static Mood? parse(Object? value) {
    for (final mood in Mood.values) {
      if (mood.id == value) return mood;
    }
    return null;
  }
}

/// What the footage is about.
enum Topic {
  dance('dance'),
  travel('travel'),
  growth('growth'),

  /// 主体衔接: one action carried across every cut.
  continuity('continuity'),

  /// A year in review: people, scenery, food, the rest.
  annual('annual'),

  /// The same group of people over time.
  together('together');

  const Topic(this.id);
  final String id;

  static Topic? parse(Object? value) {
    for (final topic in Topic.values) {
      if (topic.id == value) return topic;
    }
    return null;
  }
}

class StyleChoice {
  const StyleChoice({
    this.mood,
    this.topic,
    this.beatSync,
    this.music,
    this.targetDurationUs,
    this.musicLoop,
    this.order,
  });

  final Mood? mood;
  final Topic? topic;

  /// Cut on the beat. Null means the style decides.
  final bool? beatSync;

  /// Music to cut to and lay under the result.
  final String? music;

  /// How long the finished piece should be. Null lets the style decide.
  final int? targetDurationUs;

  /// When the cut runs longer than the music: play it again (true) or let
  /// the end fall silent (false). Null until the user has been asked.
  final bool? musicLoop;

  /// `time` or `category` for a cut that can be ordered either way: a
  /// year's memories. Null is the style's own. Kept through every edit, so
  /// changing the music does not lose it.
  final String? order;

  StyleChoice copyWith({
    Mood? mood,
    bool clearMood = false,
    Topic? topic,
    bool clearTopic = false,
    bool? beatSync,
    String? music,
    bool clearMusic = false,
    int? targetDurationUs,
    bool clearDuration = false,
    bool? musicLoop,
    bool clearMusicLoop = false,
  }) =>
      StyleChoice(
        mood: clearMood ? null : (mood ?? this.mood),
        topic: clearTopic ? null : (topic ?? this.topic),
        beatSync: beatSync ?? this.beatSync,
        music: clearMusic ? null : (music ?? this.music),
        targetDurationUs:
            clearDuration ? null : (targetDurationUs ?? this.targetDurationUs),
        musicLoop: clearMusicLoop ? null : (musicLoop ?? this.musicLoop),
        order: order,
      );

  Map<String, dynamic> toJson() => {
        if (mood != null) 'mood': mood!.id,
        if (topic != null) 'topic': topic!.id,
        if (beatSync != null) 'beatSync': beatSync,
        if (music != null && music!.isNotEmpty) 'music': music,
        if (targetDurationUs != null) 'targetDurationUs': targetDurationUs,
        if (musicLoop != null) 'musicLoop': musicLoop,
        if (order != null) 'order': order,
      };

  factory StyleChoice.fromJson(Map<String, dynamic>? json) => StyleChoice(
        mood: Mood.parse(json?['mood']),
        topic: Topic.parse(json?['topic']),
        beatSync: json?['beatSync'] as bool?,
        music: json?['music'] as String?,
        targetDurationUs: (json?['targetDurationUs'] as num?)?.toInt(),
        musicLoop: json?['musicLoop'] as bool?,
        order: json?['order'] as String?,
      );
}

/// What a run is about to do that the user should decide first.
class RunPreflight {
  const RunPreflight({
    this.cutDurationUs,
    this.musicDurationUs,
    this.musicShort = false,
    this.askMusicLoop = false,
  });

  /// How long the cut is set to be, or the style's own length.
  final int? cutDurationUs;
  final int? musicDurationUs;

  /// The music ends before the cut does.
  final bool musicShort;

  /// ...and the user has not yet said whether to play it again.
  final bool askMusicLoop;

  factory RunPreflight.fromJson(Map<String, dynamic> json) => RunPreflight(
        cutDurationUs: (json['cutDurationUs'] as num?)?.toInt(),
        musicDurationUs: (json['musicDurationUs'] as num?)?.toInt(),
        musicShort: json['musicShort'] as bool? ?? false,
        askMusicLoop: json['askMusicLoop'] as bool? ?? false,
      );
}

/// Earlier analysis the library keeps, and what clearing it gives back.
class CacheInfo {
  const CacheInfo({required this.entries, required this.bytes});

  final int entries;
  final int bytes;

  factory CacheInfo.fromJson(Map<String, dynamic> json) => CacheInfo(
        entries: (json['entries'] as num?)?.toInt() ?? 0,
        bytes: (json['bytes'] as num?)?.toInt() ?? 0,
      );
}

/// A piece of music, as the engine heard it.
class MusicInfo {
  const MusicInfo({
    required this.path,
    required this.durationUs,
    required this.bpm,
    required this.steady,
  });

  final String path;
  final int? durationUs;
  final double? bpm;

  /// Whether the beat is clear enough to cut to. Below it, the tempo is a
  /// guess and the UI says there is no clear beat rather than quoting one.
  final bool steady;

  factory MusicInfo.fromJson(Map<String, dynamic> json) => MusicInfo(
        path: json['path'] as String? ?? '',
        durationUs: (json['durationUs'] as num?)?.toInt(),
        bpm: (json['bpm'] as num?)?.toDouble(),
        steady: json['steady'] as bool? ?? false,
      );
}

/// Where a run's models come from.
enum ModelChoice {
  /// Everything on this device. Always available, needs no network.
  local('local'),

  /// A configured endpoint, for description of stills. Falls back to local.
  cloud('cloud');

  const ModelChoice(this.id);
  final String id;

  static ModelChoice parse(Object? value) =>
      value == 'cloud' ? ModelChoice.cloud : ModelChoice.local;
}

class LastRun {
  const LastRun({
    required this.at,
    required this.draftPath,
    required this.durationUs,
    required this.clipsUsed,
    required this.editorDrafts,
  });

  final String at;
  final String draftPath;
  final int durationUs;
  final int clipsUsed;
  final List<String> editorDrafts;

  factory LastRun.fromJson(Map<String, dynamic> json) => LastRun(
        at: json['at'] as String? ?? '',
        draftPath: json['draftPath'] as String? ?? '',
        durationUs: (json['durationUs'] as num?)?.toInt() ?? 0,
        clipsUsed: (json['clipsUsed'] as num?)?.toInt() ?? 0,
        editorDrafts: _strings(json['editorDrafts']),
      );
}

class Project {
  const Project({
    required this.id,
    required this.name,
    required this.inputs,
    required this.style,
    required this.model,
    required this.editors,
    required this.createdAt,
    required this.updatedAt,
    required this.dir,
    this.lastRun,
    this.memory,
    this.editedAt,
    this.exports = const [],
  });

  final String id;
  final String name;

  /// When a person last changed the cut in the built-in editor.
  final String? editedAt;

  /// What the built-in editor has written, newest last.
  final List<ExportRecord> exports;

  /// The memory the video butler made this project for, when it did.
  final String? memory;

  /// What the user picked: files, folders, or both.
  final List<String> inputs;
  final StyleChoice style;
  final ModelChoice model;

  /// Editors to write a native draft for, by id.
  final List<String> editors;
  final String createdAt;
  final String updatedAt;
  final String dir;
  final LastRun? lastRun;

  factory Project.fromJson(Map<String, dynamic> json) => Project(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? '',
        inputs: _strings(json['inputs']),
        style: StyleChoice.fromJson(json['style'] as Map<String, dynamic>?),
        model: ModelChoice.parse(json['model']),
        editors: _strings(json['editors']),
        createdAt: json['createdAt'] as String? ?? '',
        updatedAt: json['updatedAt'] as String? ?? '',
        dir: json['dir'] as String? ?? '',
        lastRun: json['lastRun'] == null
            ? null
            : LastRun.fromJson(json['lastRun'] as Map<String, dynamic>),
        memory: json['memory'] as String?,
        editedAt: json['editedAt'] as String?,
        exports: [
          for (final e in json['exports'] as List<dynamic>? ?? const [])
            ExportRecord.fromJson(e as Map<String, dynamic>),
        ],
      );
}

/// Something the built-in editor wrote: a video, or a draft for another
/// editor.
class ExportRecord {
  const ExportRecord({required this.at, required this.kind, required this.path});
  final String at;

  /// `video`, `jianying`, `capcut` or `otio`.
  final String kind;
  final String path;

  factory ExportRecord.fromJson(Map<String, dynamic> json) => ExportRecord(
        at: json['at'] as String? ?? '',
        kind: json['kind'] as String? ?? '',
        path: json['path'] as String? ?? '',
      );
}

/// A stage of a run, for the list the progress view shows.
class StageInfo {
  const StageInfo(this.id, this.label);
  final String id;
  final String label;
}

/// Where a background job has got to.
class JobStatus {
  const JobStatus({
    required this.jobId,
    required this.state,
    required this.stage,
    required this.stageLabel,
    required this.percent,
    required this.detail,
    required this.sourceIndex,
    required this.sourceCount,
    required this.stages,
    this.step = '',
    this.file,
    this.result,
    this.error,
    this.activeSources = 0,
    this.thermal,
  });

  /// How many clips are being analysed at this moment.
  final int activeSources;

  /// What the heat guard is doing to the run: `slowing`, `cooling`, or null.
  final String? thermal;

  /// What is happening, as a stable id the UI words in its own language.
  final String step;

  /// The file being worked on, when there is one.
  final String? file;

  final String jobId;

  /// `running`, `done`, `failed` or `cancelled`.
  final String state;
  final String stage;
  final String stageLabel;

  /// 0 to 100 over the whole job.
  final double percent;
  final String detail;
  final int sourceIndex;
  final int sourceCount;
  final List<StageInfo> stages;
  final Map<String, dynamic>? result;
  final String? error;

  bool get finished => state != 'running';
  bool get succeeded => state == 'done';

  factory JobStatus.fromJson(Map<String, dynamic> json) => JobStatus(
        jobId: json['jobId'] as String? ?? '',
        state: json['state'] as String? ?? 'running',
        stage: json['stage'] as String? ?? '',
        stageLabel: json['stageLabel'] as String? ?? '',
        percent: (json['percent'] as num?)?.toDouble() ?? 0,
        detail: json['detail'] as String? ?? '',
        sourceIndex: (json['sourceIndex'] as num?)?.toInt() ?? 0,
        sourceCount: (json['sourceCount'] as num?)?.toInt() ?? 0,
        activeSources: (json['activeSources'] as num?)?.toInt() ?? 0,
        thermal: json['thermal'] as String?,
        stages: (json['stages'] as List<dynamic>? ?? [])
            .map((s) => s as Map<String, dynamic>)
            .map((s) => StageInfo(
                  s['id'] as String? ?? '',
                  s['label'] as String? ?? '',
                ))
            .toList(growable: false),
        step: json['step'] as String? ?? '',
        file: json['file'] as String?,
        result: json['result'] as Map<String, dynamic>?,
        error: json['error'] as String?,
      );
}

class ModelInfo {
  const ModelInfo({
    required this.id,
    required this.name,
    required this.role,
    required this.roleLabel,
    required this.bytes,
    required this.license,
    required this.note,
    required this.memoryMb,
    required this.installed,
    required this.path,
    this.downloadable = true,
    this.fits = true,
    this.files = const [],
    this.bundled = false,
    this.inUse = false,
  });

  /// Came with the app, in the installer, so it cannot be deleted here.
  final bool bundled;

  /// The one a run uses for its role: the user's choice, or the most
  /// capable installed one that fits.
  final bool inUse;

  /// Whether this device has the memory to run it.
  final bool fits;

  /// The files it is made of, as a model pack carries them.
  final List<({String name, int bytes})> files;

  final String id;
  final String name;
  final String role;
  final String roleLabel;
  final int bytes;
  final String license;
  final String note;
  final int memoryMb;
  final bool installed;
  final String path;

  /// False for a model the app can use but not yet fetch: converted on the
  /// development side and placed in the library by hand.
  final bool downloadable;

  factory ModelInfo.fromJson(Map<String, dynamic> json) => ModelInfo(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? '',
        role: json['role'] as String? ?? '',
        roleLabel: json['roleLabel'] as String? ?? '',
        bytes: (json['bytes'] as num?)?.toInt() ?? 0,
        license: json['license'] as String? ?? '',
        note: json['note'] as String? ?? '',
        memoryMb: (json['memoryMb'] as num?)?.toInt() ?? 0,
        installed: json['installed'] as bool? ?? false,
        path: json['path'] as String? ?? '',
        downloadable: json['downloadable'] as bool? ?? true,
        fits: json['fits'] as bool? ?? true,
        bundled: json['bundled'] as bool? ?? false,
        inUse: json['inUse'] as bool? ?? false,
        files: [
          for (final f in json['files'] as List<dynamic>? ?? const [])
            (
              name: (f as Map<String, dynamic>)['name'] as String? ?? '',
              bytes: (f['bytes'] as num?)?.toInt() ?? -1,
            ),
        ],
      );
}

class ModelLibrary {
  const ModelLibrary({required this.directory, required this.models});
  final String directory;
  final List<ModelInfo> models;

  List<ModelInfo> get installed =>
      models.where((m) => m.installed).toList(growable: false);

  factory ModelLibrary.fromJson(Map<String, dynamic> json) => ModelLibrary(
        directory: json['directory'] as String? ?? '',
        models: (json['models'] as List<dynamic>? ?? [])
            .map((m) => ModelInfo.fromJson(m as Map<String, dynamic>))
            .toList(growable: false),
      );
}

class CloudSettings {
  const CloudSettings({
    required this.baseUrl,
    required this.model,
    required this.visionModel,
    required this.apiKeySet,
    required this.configured,
  });

  final String baseUrl;
  final String model;
  final String visionModel;

  /// Whether a key is stored. The key itself never leaves the engine.
  final bool apiKeySet;
  final bool configured;

  factory CloudSettings.fromJson(Map<String, dynamic>? json) => CloudSettings(
        baseUrl: json?['baseUrl'] as String? ?? '',
        model: json?['model'] as String? ?? '',
        visionModel: json?['visionModel'] as String? ?? '',
        apiKeySet: json?['apiKeySet'] as bool? ?? false,
        configured: json?['configured'] as bool? ?? false,
      );
}

class AppSettings {
  const AppSettings({
    required this.cloud,
    required this.defaultEditors,
    required this.libraryPath,
    this.language = 'zh',
    this.draftMedia = 'link',
    this.thermalGuard = true,
  });

  final CloudSettings cloud;
  final List<String> defaultEditors;
  final String libraryPath;

  /// `zh` or `en`.
  final String language;

  /// How an editor draft points at footage the editor may not read where
  /// it is: `link` (a hard link in the draft) or `reference` (the original).
  final String draftMedia;

  /// Slow a run down when the device gets hot.
  final bool thermalGuard;

  factory AppSettings.fromJson(Map<String, dynamic> json) => AppSettings(
        cloud: CloudSettings.fromJson(json['cloud'] as Map<String, dynamic>?),
        defaultEditors: _strings(json['defaultEditors']),
        libraryPath: json['libraryPath'] as String? ?? '',
        language: json['language'] as String? ?? 'zh',
        draftMedia: json['draftMedia'] as String? ?? 'link',
        thermalGuard: json['thermalGuard'] as bool? ?? true,
      );
}

/// Where a subject was seen.
class Appearance {
  const Appearance({
    required this.projectId,
    required this.projectName,
    required this.sourcePath,
    required this.shots,
  });

  final String projectId;
  final String projectName;
  final String sourcePath;
  final int shots;

  factory Appearance.fromJson(Map<String, dynamic> json) => Appearance(
        projectId: json['projectId'] as String? ?? '',
        projectName: json['projectName'] as String? ?? '',
        sourcePath: json['sourcePath'] as String? ?? '',
        shots: (json['shots'] as num?)?.toInt() ?? 0,
      );
}

/// One subject in the album: faces grouped by how alike they look.
class Subject {
  const Subject({
    required this.id,
    required this.label,
    required this.kind,
    required this.thumbnail,
    required this.clips,
    required this.shots,
    required this.appearances,
  });

  final String id;

  /// What the user called them. Null until they say.
  final String? label;
  final String kind;
  final String? thumbnail;
  final int clips;
  final int shots;
  final List<Appearance> appearances;

  factory Subject.fromJson(Map<String, dynamic> json) => Subject(
        id: json['id'] as String? ?? '',
        label: json['label'] as String?,
        kind: json['kind'] as String? ?? 'person',
        thumbnail: json['thumbnail'] as String?,
        clips: (json['clips'] as num?)?.toInt() ?? 0,
        shots: (json['shots'] as num?)?.toInt() ?? 0,
        appearances: (json['appearances'] as List<dynamic>? ?? [])
            .map((a) => Appearance.fromJson(a as Map<String, dynamic>))
            .toList(growable: false),
      );
}

class Album {
  const Album({required this.subjects, required this.grouping});
  final List<Subject> subjects;

  /// The engine's own sentence on what the grouping is, shown as is.
  final String grouping;

  factory Album.fromJson(Map<String, dynamic> json) => Album(
        subjects: (json['subjects'] as List<dynamic>? ?? [])
            .map((s) => Subject.fromJson(s as Map<String, dynamic>))
            .toList(growable: false),
        grouping: json['grouping'] as String? ?? '',
      );
}

/// An editor that can take a draft.
class EditorInfo {
  const EditorInfo({
    required this.id,
    required this.name,
    required this.installed,
    required this.version,
    required this.draftRoot,
    required this.canExport,
    required this.running,
    this.phone = false,
    this.appPath,
  });

  final String id;
  final String name;
  final bool installed;
  final String? version;
  final String? draftRoot;
  final bool canExport;

  /// A draft written while the editor is open appears after it restarts.
  final bool running;

  /// The phone app, which keeps its drafts where no other app may write:
  /// found installed or not, it takes no draft from here.
  final bool phone;

  /// Where the editor is: an app bundle on a desktop, the package on a phone.
  final String? appPath;

  factory EditorInfo.fromJson(Map<String, dynamic> json) => EditorInfo(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? '',
        installed: json['installed'] as bool? ?? false,
        version: json['version'] as String?,
        draftRoot: json['draftRoot'] as String?,
        canExport: json['canExport'] as bool? ?? false,
        running: json['running'] as bool? ?? false,
        phone: json['phone'] as bool? ?? false,
        appPath: json['appPath'] as String?,
      );
}

List<String> _strings(Object? value) => (value as List<dynamic>? ?? [])
    .map((v) => v.toString())
    .toList(growable: false);

// --- the video butler -------------------------------------------------------

double _num(Object? v) => (v as num?)?.toDouble() ?? 0;

/// A point on the earth, in degrees.
class GeoPoint {
  const GeoPoint(this.lat, this.lon);
  final double lat;
  final double lon;

  static GeoPoint? fromJson(Object? json) {
    if (json is! Map<String, dynamic>) return null;
    return GeoPoint(_num(json['lat']), _num(json['lon']));
  }
}

/// One video or photo the butler keeps.
class CatalogVideo {
  const CatalogVideo({
    required this.path,
    required this.name,
    required this.capturedAt,
    required this.fromCamera,
    required this.durationUs,
    required this.width,
    required this.height,
    required this.place,
    required this.categories,
    required this.subjects,
    this.isPhoto = false,
  });

  final String path;
  final String name;

  /// A photo: one picture, shown rather than played.
  final bool isPhoto;
  final String? capturedAt;

  /// The time is the camera's, not the file's.
  final bool fromCamera;
  final int durationUs;
  final int width;
  final int height;

  /// Where it was filmed, by name, when its camera said.
  final String? place;
  final List<String> categories;
  final List<String> subjects;

  DateTime? get captured =>
      capturedAt == null ? null : DateTime.tryParse(capturedAt!)?.toLocal();

  factory CatalogVideo.fromJson(Map<String, dynamic> json) => CatalogVideo(
        path: json['path'] as String? ?? '',
        name: json['name'] as String? ?? '',
        isPhoto: json['mediaKind'] == 'image',
        capturedAt: json['capturedAt'] as String?,
        fromCamera: json['capturedFrom'] == 'camera',
        durationUs: (json['durationUs'] as num?)?.toInt() ?? 0,
        width: (json['width'] as num?)?.toInt() ?? 0,
        height: (json['height'] as num?)?.toInt() ?? 0,
        place: (json['place'] as Map<String, dynamic>?)?['name'] as String?,
        categories: _strings(json['categories']),
        subjects: _strings(json['subjects']),
      );
}

/// A category on the butler's page.
class CategorySummary {
  const CategorySummary({
    required this.id,
    required this.name,
    required this.count,
    required this.samples,
    required this.unverified,
    this.photos = 0,
  });

  final String id;
  final String name;

  /// Videos and photos together; [photos] of them are photos.
  final int count;
  final int photos;
  int get videos => count - photos;

  /// A few of its videos, newest first, for the card.
  final List<String> samples;

  /// Its rules had no real footage to be checked against.
  final bool unverified;

  factory CategorySummary.fromJson(Map<String, dynamic> json) => CategorySummary(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? '',
        count: (json['count'] as num?)?.toInt() ?? 0,
        photos: (json['photos'] as num?)?.toInt() ?? 0,
        samples: _strings(json['samples']),
        unverified: json['unverified'] as bool? ?? false,
      );
}

/// A person the butler found, with how many videos and photos they are the
/// subject of.
class ButlerPerson {
  const ButlerPerson({
    required this.id,
    required this.label,
    required this.thumbnail,
    required this.videos,
    this.photos = 0,
  });

  final String id;
  final String? label;
  final String? thumbnail;
  final int videos;
  final int photos;

  factory ButlerPerson.fromJson(Map<String, dynamic> json) => ButlerPerson(
        id: json['id'] as String? ?? '',
        label: json['label'] as String?,
        thumbnail: json['thumbnail'] as String?,
        videos: (json['videos'] as num?)?.toInt() ?? 0,
        photos: (json['photos'] as num?)?.toInt() ?? 0,
      );
}

/// The butler's page.
class HostingOverview {
  const HostingOverview({
    required this.enabled,
    required this.roots,
    required this.customRoots,
    required this.videos,
    required this.scannedAt,
    required this.categories,
    required this.removed,
    required this.people,
    required this.notes,
    required this.memories,
    this.jobId,
    this.volumes = const [],
    this.warnings = const [],
    this.photos = 0,
  });

  final bool enabled;
  final List<String> roots;
  final bool customRoots;
  final int videos;
  final int photos;
  final String? scannedAt;
  final List<CategorySummary> categories;

  /// Categories the user removed, as `{id, name}`.
  final List<(String, String)> removed;
  final List<ButlerPerson> people;
  final List<String> notes;
  final int memories;

  /// The pass running now, if one is.
  final String? jobId;

  /// The disks plugged in, as `(name, path)`, for adding one in a tap.
  final List<(String, String)> volumes;

  /// What stands in the way of a folder kept: a disk not plugged in, or one
  /// macOS has not let the app read. Shown whatever else is.
  final List<String> warnings;

  factory HostingOverview.fromJson(Map<String, dynamic> json) => HostingOverview(
        enabled: json['enabled'] as bool? ?? false,
        roots: _strings(json['roots']),
        customRoots: json['customRoots'] as bool? ?? false,
        videos: (json['videos'] as num?)?.toInt() ?? 0,
        photos: (json['photos'] as num?)?.toInt() ?? 0,
        scannedAt: json['scannedAt'] as String?,
        categories: (json['categories'] as List<dynamic>? ?? [])
            .map((c) => CategorySummary.fromJson(c as Map<String, dynamic>))
            .toList(growable: false),
        removed: (json['removed'] as List<dynamic>? ?? [])
            .map((c) => (
                  (c as Map<String, dynamic>)['id'] as String? ?? '',
                  c['name'] as String? ?? '',
                ))
            .toList(growable: false),
        people: (json['people'] as List<dynamic>? ?? [])
            .map((p) => ButlerPerson.fromJson(p as Map<String, dynamic>))
            .toList(growable: false),
        notes: _strings(json['notes']),
        memories: (json['memories'] as num?)?.toInt() ?? 0,
        jobId: json['jobId'] as String?,
        volumes: [
          for (final v in json['volumes'] as List<dynamic>? ?? const [])
            (
              (v as Map<String, dynamic>)['name'] as String? ?? '',
              v['path'] as String? ?? '',
            ),
        ],
        warnings: _strings(json['warnings']),
      );
}

/// The kinds of memory the map filters by.
enum MemoryKind {
  travel('travel'),
  growth('growth'),
  annual('annual'),
  group('group');

  const MemoryKind(this.id);
  final String id;

  static MemoryKind parse(Object? value) =>
      MemoryKind.values.firstWhere((k) => k.id == value, orElse: () => MemoryKind.travel);
}

/// One memory: a trip, someone growing up, a year, a group.
class Memory {
  const Memory({
    required this.id,
    required this.kind,
    required this.title,
    required this.start,
    required this.end,
    required this.label,
    required this.place,
    required this.pin,
    required this.regions,
    required this.points,
    required this.videoCount,
    required this.cover,
    required this.people,
    required this.order,
    required this.projectId,
    required this.projectRun,
    required this.status,
    required this.error,
    this.videos = const [],
    this.photoCount = 0,
  });

  final String id;
  final MemoryKind kind;
  final String title;
  final String start;
  final String end;

  /// What its pin says: `2023.09`.
  final String label;
  final String? place;
  final GeoPoint? pin;
  final List<String> regions;
  final List<GeoPoint> points;
  final int videoCount;
  final int photoCount;
  final String? cover;
  final List<ButlerPerson> people;

  /// A year's order: `category` or `time`.
  final String? order;
  final String? projectId;

  /// Its project has been cut.
  final bool projectRun;

  /// `pending`, `done`, `failed` or `dismissed`.
  final String? status;
  final String? error;
  final List<CatalogVideo> videos;

  factory Memory.fromJson(Map<String, dynamic> json) => Memory(
        id: json['id'] as String? ?? '',
        kind: MemoryKind.parse(json['kind']),
        title: json['title'] as String? ?? '',
        start: json['start'] as String? ?? '',
        end: json['end'] as String? ?? '',
        label: json['label'] as String? ?? '',
        place: json['place'] as String?,
        pin: GeoPoint.fromJson(json['pin']),
        regions: _strings(json['regions']),
        points: (json['points'] as List<dynamic>? ?? [])
            .map(GeoPoint.fromJson)
            .whereType<GeoPoint>()
            .toList(growable: false),
        videoCount: (json['videoCount'] as num?)?.toInt() ?? 0,
        photoCount: (json['photoCount'] as num?)?.toInt() ?? 0,
        cover: json['cover'] as String?,
        people: (json['people'] as List<dynamic>? ?? [])
            .map((p) => ButlerPerson.fromJson(p as Map<String, dynamic>))
            .toList(growable: false),
        order: json['order'] as String?,
        projectId: json['projectId'] as String?,
        projectRun: json['projectRun'] as bool? ?? false,
        status: json['status'] as String?,
        error: json['error'] as String?,
        videos: (json['videos'] as List<dynamic>? ?? [])
            .map((v) => CatalogVideo.fromJson(v as Map<String, dynamic>))
            .toList(growable: false),
      );
}

class MemoryList {
  const MemoryList({required this.memories, required this.regions, required this.notes});
  final List<Memory> memories;

  /// Every region a memory lights.
  final List<String> regions;
  final List<String> notes;

  factory MemoryList.fromJson(Map<String, dynamic> json) => MemoryList(
        memories: (json['memories'] as List<dynamic>? ?? [])
            .map((m) => Memory.fromJson(m as Map<String, dynamic>))
            .toList(growable: false),
        regions: _strings(json['regions']),
        notes: _strings(json['notes']),
      );
}

/// A country or a province, as the map draws it.
class MapRegion {
  const MapRegion({
    required this.id,
    required this.province,
    required this.parent,
    required this.zh,
    required this.en,
    required this.label,
    required this.rings,
  });

  final String id;
  final bool province;
  final String? parent;
  final String zh;
  final String en;
  final GeoPoint label;

  /// Outlines as longitude, latitude pairs, flat.
  final List<List<double>> rings;
}

/// The world the memory map is drawn on: the engine's own outlines, so the
/// region a memory lights is the region it was named after.
class WorldMap {
  const WorldMap(this.regions);
  final List<MapRegion> regions;

  factory WorldMap.fromJson(Map<String, dynamic> json) {
    final scale = _num(json['scale']) == 0 ? 100.0 : _num(json['scale']);
    return WorldMap([
      for (final r in json['regions'] as List<dynamic>? ?? const [])
        MapRegion(
          id: (r as Map<String, dynamic>)['id'] as String? ?? '',
          province: r['kind'] == 'province',
          parent: r['parent'] as String?,
          zh: r['zh'] as String? ?? '',
          en: r['en'] as String? ?? '',
          label: GeoPoint(
            _num((r['label'] as List<dynamic>?)?.elementAtOrNull(1)) / scale,
            _num((r['label'] as List<dynamic>?)?.elementAtOrNull(0)) / scale,
          ),
          rings: [
            for (final ring in r['rings'] as List<dynamic>? ?? const [])
              [for (final v in ring as List<dynamic>) (v as num).toDouble() / scale],
          ],
        ),
    ]);
  }
}


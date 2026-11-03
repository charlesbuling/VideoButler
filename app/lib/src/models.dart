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

/// Data the engine returns.
///
/// Every field here mirrors something the Rust side already produces. The
/// parsers are deliberately forgiving about missing keys, because the UI
/// should degrade rather than crash when the engine gains a field the app has
/// not learned about yet.
library;

class EngineVersion {
  const EngineVersion({
    required this.version,
    required this.footageGraph,
    required this.editingDsl,
  });

  final String version;
  final String footageGraph;
  final String editingDsl;

  factory EngineVersion.fromJson(Map<String, dynamic> json) => EngineVersion(
        version: json['version'] as String? ?? 'unknown',
        footageGraph: json['footageGraph'] as String? ?? '',
        editingDsl: json['editingDsl'] as String? ?? '',
      );
}

class AdapterInfo {
  const AdapterInfo({required this.role, required this.name, required this.mock});

  final String role;
  final String name;

  /// True when this adapter produces synthetic output. The UI must say so:
  /// a cut assembled without the system having listened to or looked at the
  /// footage is not a finished cut.
  final bool mock;

  factory AdapterInfo.fromJson(Map<String, dynamic> json) => AdapterInfo(
        role: json['role'] as String? ?? '',
        name: json['name'] as String? ?? '',
        mock: json['mock'] as bool? ?? false,
      );
}

class DoctorReport {
  const DoctorReport({
    required this.mediaBackend,
    required this.mediaAvailable,
    required this.mediaVersion,
    required this.mediaNote,
    required this.adapters,
    required this.canAnalyse,
  });

  final String mediaBackend;
  final bool mediaAvailable;
  final String? mediaVersion;
  final String mediaNote;
  final List<AdapterInfo> adapters;

  /// Whether a real analysis is possible at all on this platform.
  final bool canAnalyse;

  List<AdapterInfo> get mockedAdapters =>
      adapters.where((a) => a.mock).toList(growable: false);

  factory DoctorReport.fromJson(Map<String, dynamic> json) => DoctorReport(
        mediaBackend: json['mediaBackend'] as String? ?? 'unknown',
        mediaAvailable: json['mediaAvailable'] as bool? ?? false,
        mediaVersion: json['mediaVersion'] as String?,
        mediaNote: json['mediaNote'] as String? ?? '',
        adapters: (json['adapters'] as List<dynamic>? ?? [])
            .map((a) => AdapterInfo.fromJson(a as Map<String, dynamic>))
            .toList(growable: false),
        canAnalyse: json['canAnalyse'] as bool? ?? false,
      );
}

class EditTemplate {
  const EditTemplate({
    required this.id,
    required this.name,
    required this.kind,
    required this.tags,
    required this.targetDurationUs,
    required this.minSlotUs,
    required this.maxSlotUs,
    required this.transition,
    required this.subtitles,
  });

  final String id;
  final String name;
  final String kind;
  final List<String> tags;
  final int targetDurationUs;
  final int minSlotUs;
  final int maxSlotUs;
  final String transition;
  final bool subtitles;

  factory EditTemplate.fromJson(Map<String, dynamic> json) {
    final params = json['params'] as Map<String, dynamic>? ?? const {};
    return EditTemplate(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? '',
      kind: json['kind'] as String? ?? '',
      tags: (json['tags'] as List<dynamic>? ?? [])
          .map((t) => t.toString())
          .toList(growable: false),
      targetDurationUs: params['targetDurationUs'] as int? ?? 0,
      minSlotUs: params['minSlotUs'] as int? ?? 0,
      maxSlotUs: params['maxSlotUs'] as int? ?? 0,
      transition: params['transition'] as String? ?? 'cut',
      subtitles: params['subtitles'] as bool? ?? false,
    );
  }
}

/// Is this path a photo, by its name? The engine decides by the same list
/// (`vb_media::STILL_EXTENSIONS`); this is only for choosing an icon.
bool looksLikePhoto(String path) {
  final dot = path.lastIndexOf('.');
  if (dot < 0) return false;
  return const {'jpg', 'jpeg', 'png', 'heic', 'heif', 'webp'}
      .contains(path.substring(dot + 1).toLowerCase());
}

class DirectoryEntry {
  const DirectoryEntry({
    required this.name,
    required this.path,
    required this.kind,
    required this.sizeBytes,
  });

  final String name;
  final String path;
  final String kind;
  final int sizeBytes;

  bool get isDirectory => kind == 'dir';

  factory DirectoryEntry.fromJson(Map<String, dynamic> json) => DirectoryEntry(
        name: json['name'] as String? ?? '',
        path: json['path'] as String? ?? '',
        kind: json['kind'] as String? ?? 'dir',
        sizeBytes: json['sizeBytes'] as int? ?? 0,
      );
}

/// When and how one clip was filmed, from the engine's `media.inspect`.
///
/// The picker filters by these: the day, from the camera's own record where
/// there is one, and the frame's shape as it is shown, a phone's sideways
/// recording already turned upright.
class MediaDetails {
  const MediaDetails({
    required this.path,
    this.capturedAt,
    this.capturedByCamera = false,
    this.durationUs = 0,
    this.width = 0,
    this.height = 0,
    this.orientation = 'unknown',
    this.ratio = '?',
    this.error,
  });

  final String path;

  /// Local time. Null when neither the camera nor the file says.
  final DateTime? capturedAt;

  /// False when [capturedAt] is only the file's date, which a copy resets.
  final bool capturedByCamera;
  final int durationUs;
  final int width;
  final int height;

  /// `landscape`, `portrait`, `square` or `unknown`.
  final String orientation;

  /// The shape as people name it: `16:9`, `9:16`, `4:3`, `1:1`...
  final String ratio;
  final String? error;

  factory MediaDetails.fromJson(Map<String, dynamic> json) => MediaDetails(
        path: json['path'] as String? ?? '',
        capturedAt:
            DateTime.tryParse(json['capturedAt'] as String? ?? '')?.toLocal(),
        capturedByCamera: json['capturedFrom'] == 'camera',
        durationUs: (json['durationUs'] as num?)?.toInt() ?? 0,
        width: (json['width'] as num?)?.toInt() ?? 0,
        height: (json['height'] as num?)?.toInt() ?? 0,
        orientation: json['orientation'] as String? ?? 'unknown',
        ratio: json['ratio'] as String? ?? '?',
        error: json['error'] as String?,
      );
}

class DirectoryListing {
  const DirectoryListing({
    required this.path,
    required this.parent,
    required this.entries,
    required this.error,
    this.places = const [],
  });

  final String path;
  final String? parent;
  final List<DirectoryEntry> entries;
  final String? error;

  /// Shortcuts to where this kind of file usually is, only those that
  /// exist: for music the Music folder and the music apps' downloads; for
  /// footage the usual folders, then every disk plugged in (`id` `volume`,
  /// with the disk's `name`).
  final List<({String id, String path, String? name})> places;

  factory DirectoryListing.fromJson(Map<String, dynamic> json) => DirectoryListing(
        path: json['path'] as String? ?? '',
        parent: json['parent'] as String?,
        entries: (json['entries'] as List<dynamic>? ?? [])
            .map((e) => DirectoryEntry.fromJson(e as Map<String, dynamic>))
            .toList(growable: false),
        error: json['error'] as String?,
        places: [
          for (final p in json['places'] as List<dynamic>? ?? const [])
            (
              id: (p as Map<String, dynamic>)['id'] as String? ?? '',
              path: p['path'] as String? ?? '',
              name: p['name'] as String?,
            ),
        ],
      );
}

class DetectedShot {
  const DetectedShot({
    required this.id,
    required this.startUs,
    required this.endUs,
    required this.durationUs,
    required this.score,
    required this.keyframes,
    required this.used,
    required this.description,
    this.sourceId = '',
  });

  final String id;

  /// Which clip of the project this shot is in.
  final String sourceId;
  final int startUs;
  final int endUs;
  final int durationUs;
  final double score;
  final int keyframes;

  /// Whether this shot made the cut. Showing the ones that did not is what
  /// lets a user judge the selection rather than just accept it.
  final bool used;
  final String? description;

  factory DetectedShot.fromJson(Map<String, dynamic> json) => DetectedShot(
        id: json['id'] as String? ?? '',
        startUs: json['startUs'] as int? ?? 0,
        endUs: json['endUs'] as int? ?? 0,
        durationUs: json['durationUs'] as int? ?? 0,
        score: (json['score'] as num?)?.toDouble() ?? 0,
        keyframes: json['keyframes'] as int? ?? 0,
        used: json['used'] as bool? ?? false,
        description: json['description'] as String?,
        sourceId: json['sourceId'] as String? ?? '',
      );
}

class CutSlot {
  const CutSlot({
    required this.id,
    required this.kind,
    required this.durationUs,
    required this.timelineStartUs,
    required this.shotId,
    required this.reason,
    required this.score,
    this.sourceId,
  });

  final String id;
  final String kind;
  final int durationUs;
  final int timelineStartUs;
  final String? shotId;

  /// The video or photo the clip is taken from, one of the run's sources.
  final String? sourceId;

  /// Why the orchestrator chose this clip. Surfacing it is what makes the
  /// result reviewable instead of a black box.
  final String? reason;
  final double? score;

  bool get isTitleCard => kind == 'titlecard';

  factory CutSlot.fromJson(Map<String, dynamic> json) => CutSlot(
        id: json['id'] as String? ?? '',
        kind: json['kind'] as String? ?? 'clip',
        durationUs: json['durationUs'] as int? ?? 0,
        timelineStartUs: json['timelineStartUs'] as int? ?? 0,
        shotId: json['shotId'] as String?,
        reason: json['reason'] as String?,
        score: (json['score'] as num?)?.toDouble(),
        sourceId: json['sourceId'] as String?,
      );
}

/// One file a run analysed: where it is, and whether it is a video or a
/// photo.
class SourceFile {
  const SourceFile({required this.path, required this.kind});
  final String path;

  /// `video` or `image`.
  final String kind;

  bool get isPhoto => kind == 'image';
  String get name => path.split(RegExp(r'[/\\]')).last;

  factory SourceFile.fromJson(Map<String, dynamic> json) => SourceFile(
        path: json['path'] as String? ?? '',
        kind: json['kind'] as String? ?? 'video',
      );
}

class Rejection {
  const Rejection({required this.slotId, required this.reason});
  final String slotId;
  final String reason;

  factory Rejection.fromJson(Map<String, dynamic> json) => Rejection(
        slotId: json['slotId'] as String? ?? '',
        reason: json['reason'] as String? ?? '',
      );
}

/// A draft written into an editor's own draft folder.
class EditorDraft {
  const EditorDraft({
    this.editor = '',
    this.editorName = '',
    required this.name,
    required this.path,
    required this.clips,
    required this.hasMusic,
    required this.transitionsDropped,
    this.mediaLinked = 0,
    this.mediaUnreadable = 0,
  });

  /// Files hard-linked inside the draft because the editor may not read
  /// where they are. A link is the same file under a second name: no copy,
  /// no space.
  final int mediaLinked;

  /// Files referenced where they are although the editor may not read them
  /// there; it will ask to relink them.
  final int mediaUnreadable;

  /// Which editor, by id and by the name the user knows it by.
  final String editor;
  final String editorName;
  final String name;
  final String path;
  final int clips;
  final bool hasMusic;

  /// Transitions written as hard cuts, because the editor keys them by ids
  /// this build does not carry.
  final int transitionsDropped;

  factory EditorDraft.fromJson(Map<String, dynamic> json) => EditorDraft(
        editor: json['editor'] as String? ?? '',
        editorName: json['editorName'] as String? ?? '',
        name: json['name'] as String? ?? '',
        path: json['path'] as String? ?? '',
        clips: (json['clips'] as num?)?.toInt() ?? 0,
        hasMusic: json['hasMusic'] as bool? ?? false,
        transitionsDropped: (json['transitionsDropped'] as num?)?.toInt() ?? 0,
        mediaLinked: (json['mediaLinked'] as num?)?.toInt() ?? 0,
        mediaUnreadable: (json['mediaUnreadable'] as num?)?.toInt() ?? 0,
      );
}

/// The beat a cut followed.
class BeatInfo {
  const BeatInfo({
    required this.bpm,
    required this.origin,
    required this.confidence,
    required this.music,
  });

  final double bpm;

  /// `music`, `source` or `fixed`. A fixed grid is a chosen tempo, not a
  /// detected one, and is described that way.
  final String origin;
  final double? confidence;
  final String? music;

  factory BeatInfo.fromJson(Map<String, dynamic> json) => BeatInfo(
        bpm: (json['bpm'] as num?)?.toDouble() ?? 0,
        origin: json['origin'] as String? ?? 'fixed',
        confidence: (json['confidence'] as num?)?.toDouble(),
        music: json['music'] as String?,
      );
}

class RunResult {
  const RunResult({
    required this.draftPath,
    required this.footageGraphPath,
    required this.editingDslPath,
    required this.projectDir,
    required this.templateId,
    required this.durationUs,
    required this.shotsDetected,
    required this.clipsUsed,
    required this.verified,
    required this.rejected,
    required this.rejections,
    required this.reusedSources,
    required this.totalSources,
    required this.warnings,
    required this.shots,
    required this.slots,
    this.templateName = '',
    this.editorDrafts = const [],
    this.beat,
    this.adapterNotes = const [],
    this.subjects = 0,
    this.handoff = const Handoff(),
    this.sources = const {},
  });

  /// The files the run analysed, by source id.
  final Map<String, SourceFile> sources;

  final String templateName;
  final List<EditorDraft> editorDrafts;
  final BeatInfo? beat;

  /// What each model choice came to, in the engine's words: which ran on
  /// the device, which went to an endpoint, which are missing.
  final List<String> adapterNotes;

  /// Subjects found in this run, for the album.
  final int subjects;

  /// The cut as a phone editor can take it.
  final Handoff handoff;

  final String draftPath;
  final String footageGraphPath;
  final String editingDslPath;
  final String projectDir;
  final String templateId;
  final int durationUs;
  final int shotsDetected;
  final int clipsUsed;
  final int verified;
  final int rejected;
  final List<Rejection> rejections;

  /// How many of the project's videos came back from the cache, out of how
  /// many it holds.
  final int reusedSources;
  final int totalSources;
  final List<String> warnings;
  final List<DetectedShot> shots;
  final List<CutSlot> slots;

  factory RunResult.fromJson(Map<String, dynamic> json) => RunResult(
        draftPath: json['draftPath'] as String? ?? '',
        footageGraphPath: json['footageGraphPath'] as String? ?? '',
        editingDslPath: json['editingDslPath'] as String? ?? '',
        projectDir: json['projectDir'] as String? ?? '',
        templateId: json['templateId'] as String? ?? '',
        durationUs: json['durationUs'] as int? ?? 0,
        shotsDetected: json['shotsDetected'] as int? ?? 0,
        clipsUsed: json['clipsUsed'] as int? ?? 0,
        verified: json['verified'] as int? ?? 0,
        rejected: json['rejected'] as int? ?? 0,
        rejections: (json['rejections'] as List<dynamic>? ?? [])
            .map((r) => Rejection.fromJson(r as Map<String, dynamic>))
            .toList(growable: false),
        reusedSources: json['reusedSources'] as int? ?? 0,
        totalSources: json['totalSources'] as int? ?? 0,
        warnings: (json['warnings'] as List<dynamic>? ?? [])
            .map((w) => w.toString())
            .toList(growable: false),
        shots: (json['shots'] as List<dynamic>? ?? [])
            .map((s) => DetectedShot.fromJson(s as Map<String, dynamic>))
            .toList(growable: false),
        slots: (json['slots'] as List<dynamic>? ?? [])
            .map((s) => CutSlot.fromJson(s as Map<String, dynamic>))
            .toList(growable: false),
        templateName: json['templateName'] as String? ?? '',
        editorDrafts: (json['editorDrafts'] as List<dynamic>? ?? [])
            .map((d) => EditorDraft.fromJson(d as Map<String, dynamic>))
            .toList(growable: false),
        beat: json['beat'] == null
            ? null
            : BeatInfo.fromJson(json['beat'] as Map<String, dynamic>),
        adapterNotes: (json['adapterNotes'] as List<dynamic>? ?? [])
            .map((n) => n.toString())
            .toList(growable: false),
        subjects: (json['subjects'] as num?)?.toInt() ?? 0,
        handoff: json['handoff'] is Map<String, dynamic>
            ? Handoff.fromJson(json['handoff'] as Map<String, dynamic>)
            : const Handoff(),
        sources: {
          for (final source in json['sources'] as List<dynamic>? ?? const [])
            if (source is Map<String, dynamic> && source['id'] is String)
              source['id'] as String: SourceFile.fromJson(source),
        },
      );
}

/// The cut as a phone editor can take it: its clips in order, each with the
/// span to cut it to, and a count of what cannot go with them. A phone
/// editor opens shared videos as a new project but takes no draft, so
/// titles, text, transitions and music stay behind.
class Handoff {
  const Handoff({
    this.clips = const [],
    this.frame,
    this.titleCards = 0,
    this.texts = 0,
    this.transitions = 0,
    this.music,
    this.musicInUs,
  });

  /// Each clip, and whether it is a photo the host makes a clip of.
  final List<({String path, int inUs, int outUs, int? width, int? height, bool still})> clips;

  /// The cut's frame, upright. A phone editor takes its frame from the
  /// first clip, so a first clip the other way up is fitted into this one.
  final ({int width, int height})? frame;

  final int titleCards;
  final int texts;
  final int transitions;
  final String? music;

  /// Where in the music the cut starts: on a beat cut, the first beat.
  final int? musicInUs;

  factory Handoff.fromJson(Map<String, dynamic> json) => Handoff(
        clips: [
          for (final c in json['clips'] as List<dynamic>? ?? const [])
            (
              path: (c as Map<String, dynamic>)['path'] as String? ?? '',
              inUs: (c['inUs'] as num?)?.toInt() ?? 0,
              outUs: (c['outUs'] as num?)?.toInt() ?? 0,
              width: (c['width'] as num?)?.toInt(),
              height: (c['height'] as num?)?.toInt(),
              still: c['still'] == true,
            ),
        ],
        frame: json['frame'] is Map<String, dynamic>
            ? (
                width: ((json['frame'] as Map<String, dynamic>)['width'] as num).toInt(),
                height: ((json['frame'] as Map<String, dynamic>)['height'] as num).toInt(),
              )
            : null,
        titleCards: (json['titleCards'] as num?)?.toInt() ?? 0,
        texts: (json['texts'] as num?)?.toInt() ?? 0,
        transitions: (json['transitions'] as num?)?.toInt() ?? 0,
        music: json['music'] as String?,
        musicInUs: (json['musicInUs'] as num?)?.toInt(),
      );
}

/// Format microseconds as a timecode. The engine's canonical unit is
/// microseconds, so the UI converts only at the point of display.
String formatDuration(int micros) {
  final totalMs = micros ~/ 1000;
  final ms = totalMs % 1000;
  final totalSeconds = totalMs ~/ 1000;
  final seconds = totalSeconds % 60;
  final minutes = (totalSeconds ~/ 60) % 60;
  final hours = totalSeconds ~/ 3600;
  String two(int n) => n.toString().padLeft(2, '0');
  if (hours > 0) {
    return '${two(hours)}:${two(minutes)}:${two(seconds)}';
  }
  return '${two(minutes)}:${two(seconds)}.${ms.toString().padLeft(3, '0')}';
}

/// The directory holding a path, or null when there is not one.
///
/// Handles both separators and a path with no separator at all, so reopening
/// the file picker cannot throw on a bare filename or on a Windows path.
String? parentDirectoryOf(String? path) {
  if (path == null || path.isEmpty) return null;
  final slash = path.lastIndexOf('/');
  final backslash = path.lastIndexOf(r'\');
  final cut = slash > backslash ? slash : backslash;
  return cut <= 0 ? null : path.substring(0, cut);
}

String formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  const units = ['KB', 'MB', 'GB', 'TB'];
  var value = bytes / 1024;
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return '${value.toStringAsFixed(value < 10 ? 1 : 0)} ${units[unit]}';
}

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

/// The built-in editor's view of a cut, as the engine reports it.
///
/// The cut itself is the Editing DSL, held and changed by the engine; this
/// is a typed reading of what `editor.*` calls return. Absolute positions --
/// where each clip, text and piece of music sits on the finished timeline --
/// come from the engine's own resolution of the DSL (`timeline`), never from
/// adding durations up here: the DSL stores no positions, and two ways of
/// deriving them is how the drift the project was built to avoid comes back.
library;

/// What kind of slot a clip on the spine is.
enum SlotKind {
  clip,
  titleCard,
  gap;

  static SlotKind parse(Object? value) => switch (value) {
        'titleCard' => SlotKind.titleCard,
        'gap' => SlotKind.gap,
        _ => SlotKind.clip,
      };
}

/// How one clip joins the next.
enum TransitionKind {
  cut('cut'),
  xfade('xfade'),
  fadeToBlack('fadeToBlack'),
  wipeLeft('wipeLeft'),
  wipeRight('wipeRight');

  const TransitionKind(this.id);
  final String id;

  static TransitionKind parse(Object? value) => switch (value) {
        'xfade' || 'dissolve' => TransitionKind.xfade,
        'fadeToBlack' => TransitionKind.fadeToBlack,
        'wipeLeft' => TransitionKind.wipeLeft,
        'wipeRight' => TransitionKind.wipeRight,
        _ => TransitionKind.cut,
      };
}

int _int(Object? v, [int fallback = 0]) => (v as num?)?.toInt() ?? fallback;
double _double(Object? v, [double fallback = 0]) => (v as num?)?.toDouble() ?? fallback;
Map<String, dynamic> _map(Object? v) => v is Map<String, dynamic> ? v : const {};
List<dynamic> _list(Object? v) => v is List<dynamic> ? v : const [];

/// Where a clip's picture sits: enlarged, moved and turned, held for the
/// whole clip. 1, 0, 0, 0 is the picture fitted to the frame.
class Placement {
  const Placement({this.scale = 1, this.x = 0, this.y = 0, this.rotationDeg = 0});
  final double scale;
  final double x;
  final double y;
  final double rotationDeg;

  bool get isFitted => scale == 1 && x == 0 && y == 0 && rotationDeg == 0;

  static const fitted = Placement();
}

/// A clip's picture adjustments, as the engine's `eq` filter carries them.
class Adjustment {
  const Adjustment({this.brightness = 0, this.contrast = 1, this.saturation = 1});
  final double brightness;
  final double contrast;
  final double saturation;

  bool get isNeutral => brightness == 0 && contrast == 1 && saturation == 1;
}

/// What fills the frame round a picture that does not cover it.
class Backdrop {
  const Backdrop(this.kind, [this.color]);

  /// `black`, `color` or `blur`.
  final String kind;
  final String? color;

  static const black = Backdrop('black');
}

/// One slot of the spine, with what the editor shows about it.
class EditClip {
  const EditClip({
    required this.id,
    required this.kind,
    required this.durationUs,
    required this.startUs,
    required this.endUs,
    required this.overlapUs,
    this.sourceId,
    this.inUs = 0,
    this.outUs = 0,
    this.speed = 1,
    this.reason,
    this.placement = Placement.fitted,
    this.adjustment = const Adjustment(),
    this.backdrop,
    this.gainDb = 0,
    this.muted = false,
  });

  final String id;
  final SlotKind kind;
  final int durationUs;

  /// Where it sits on the finished piece, from the engine's resolution.
  final int startUs;
  final int endUs;

  /// How much of it the transition before it overlaps.
  final int overlapUs;

  final String? sourceId;
  final int inUs;
  final int outUs;
  final double speed;

  /// Why the orchestrator chose it, when it did.
  final String? reason;
  final Placement placement;
  final Adjustment adjustment;
  final Backdrop? backdrop;

  /// Its own sound.
  final double gainDb;
  final bool muted;

  bool get isClip => kind == SlotKind.clip && sourceId != null;

  /// Where `atUs` of the finished piece falls in the footage.
  int sourceTimeAt(int atUs) =>
      inUs + ((atUs - startUs).clamp(0, endUs - startUs) * speed).round();
}

/// A text laid over the piece.
class EditText {
  const EditText({
    required this.id,
    required this.content,
    required this.startUs,
    required this.endUs,
    required this.durationUs,
    required this.slotId,
    this.role,
    this.style = const TextLook(),
  });

  final String id;
  final String content;
  final int startUs;
  final int endUs;
  final int durationUs;

  /// The clip it is anchored to: it moves with it.
  final String slotId;
  final String? role;
  final TextLook style;
}

/// How a text looks, in the contract's terms: sizes in points of a frame
/// whose shorter side is 540 points.
class TextLook {
  const TextLook({
    this.fontSizePt = 36,
    this.color = '#FFFFFF',
    this.outlineColor = '#000000',
    this.outlineWidth = 1.5,
    this.alignment = 'middleCenter',
    this.bold = false,
    this.backgroundColor,
    this.x,
    this.y,
    this.marginVPct,
    this.fontFamily,
  });

  final double fontSizePt;
  final String color;
  final String? outlineColor;
  final double outlineWidth;
  final String alignment;
  final bool bold;
  final String? backgroundColor;

  /// Where its centre sits, 0..1 across and down, once placed by hand.
  final double? x;
  final double? y;
  final double? marginVPct;
  final String? fontFamily;

  factory TextLook.fromJson(Map<String, dynamic> json) => TextLook(
        fontSizePt: _double(json['fontSizePt'], 36),
        color: json['primaryColor'] as String? ?? '#FFFFFF',
        outlineColor: json['outlineColor'] as String?,
        outlineWidth: _double(json['outlineWidth'], 0),
        alignment: json['alignment'] as String? ?? 'middleCenter',
        bold: json['bold'] == true,
        backgroundColor: json['backgroundColor'] as String?,
        x: (json['x'] as num?)?.toDouble(),
        y: (json['y'] as num?)?.toDouble(),
        marginVPct: (json['marginVPct'] as num?)?.toDouble(),
        fontFamily: json['fontFamily'] as String?,
      );

  Map<String, dynamic> toJson() => {
        if (fontFamily != null) 'fontFamily': fontFamily,
        'fontSizePt': fontSizePt,
        'primaryColor': color,
        if (outlineColor != null && outlineWidth > 0) 'outlineColor': outlineColor,
        if (outlineColor != null && outlineWidth > 0) 'outlineWidth': outlineWidth,
        'alignment': alignment,
        if (marginVPct != null) 'marginVPct': marginVPct,
        if (bold) 'bold': true,
        if (backgroundColor != null) 'backgroundColor': backgroundColor,
        if (x != null) 'x': x,
        if (y != null) 'y': y,
      };

  TextLook copyWith({
    double? fontSizePt,
    String? color,
    bool? bold,
    String? backgroundColor,
    bool clearBackground = false,
    double? outlineWidth,
    String? alignment,
    double? x,
    double? y,
    bool clearPlace = false,
  }) =>
      TextLook(
        fontSizePt: fontSizePt ?? this.fontSizePt,
        color: color ?? this.color,
        outlineColor: outlineColor ?? '#000000',
        outlineWidth: outlineWidth ?? this.outlineWidth,
        alignment: alignment ?? this.alignment,
        bold: bold ?? this.bold,
        backgroundColor: clearBackground ? null : backgroundColor ?? this.backgroundColor,
        x: clearPlace ? null : x ?? this.x,
        y: clearPlace ? null : y ?? this.y,
        marginVPct: clearPlace ? marginVPct : marginVPct,
        fontFamily: fontFamily,
      );
}

/// A piece of music, or a sound effect, laid under the piece.
class EditSound {
  const EditSound({
    required this.id,
    required this.kind,
    required this.startUs,
    required this.endUs,
    this.path,
    this.assetInUs = 0,
    this.gainDb = 0,
    this.muted = false,
    this.fadeInUs = 0,
    this.fadeOutUs = 0,
  });

  final String id;

  /// `bgm` or `sfx`; a clip's own sound is on the clip.
  final String kind;
  final int startUs;
  final int endUs;
  final String? path;

  /// Where in the file it starts playing.
  final int assetInUs;
  final double gainDb;
  final bool muted;
  final int fadeInUs;
  final int fadeOutUs;

  String get name => (path ?? '').split(RegExp(r'[/\\]')).last;
}

/// The join between a clip and the next.
class EditJoin {
  const EditJoin({required this.fromSlotId, required this.toSlotId, required this.kind, required this.durationUs});
  final String fromSlotId;
  final String toSlotId;
  final TransitionKind kind;
  final int durationUs;
}

/// A file of the project's footage.
class EditSource {
  const EditSource({
    required this.id,
    required this.path,
    required this.isImage,
    required this.durationUs,
    required this.width,
    required this.height,
    required this.exists,
  });

  final String id;
  final String path;
  final bool isImage;
  final int durationUs;

  /// Upright.
  final int width;
  final int height;
  final bool exists;

  String get name => path.split(RegExp(r'[/\\]')).last;

  factory EditSource.fromJson(Map<String, dynamic> json) => EditSource(
        id: json['id'] as String? ?? '',
        path: json['path'] as String? ?? '',
        isImage: json['kind'] == 'image',
        durationUs: _int(json['durationUs']),
        width: _int(json['width']),
        height: _int(json['height']),
        exists: json['exists'] != false,
      );
}

/// What was selected after an edit: the clip split off, the text added.
class EditSelection {
  const EditSelection(this.kind, this.id);

  /// `slot`, `text` or `music`.
  final String kind;
  final String id;

  static EditSelection? fromJson(Object? json) {
    final m = _map(json);
    final kind = m['kind'] as String?;
    final id = m['id'] as String?;
    return kind == null || id == null ? null : EditSelection(kind, id);
  }

  @override
  bool operator ==(Object other) => other is EditSelection && other.kind == kind && other.id == id;

  @override
  int get hashCode => Object.hash(kind, id);
}

/// The whole of what the editor shows.
class EditState {
  const EditState({
    required this.projectId,
    required this.title,
    required this.version,
    required this.totalUs,
    required this.clips,
    required this.texts,
    required this.sounds,
    required this.joins,
    required this.sources,
    required this.frameWidth,
    required this.frameHeight,
    required this.fps,
    required this.canUndo,
    required this.canRedo,
    required this.edited,
    required this.changeRatio,
    required this.newerRun,
    required this.aspectRatio,
    this.canRenderVideo = true,
    this.undoLabel,
    this.redoLabel,
    this.selection,
    this.bpm,
    this.firstBeatUs,
    this.minSlotUs = 100000,
    this.minTextUs = 200000,
    this.minSpeed = 0.25,
    this.maxSpeed = 4,
  });

  final String projectId;
  final String title;

  /// Counts every change; a frame asked for at another version is stale.
  final int version;
  final int totalUs;
  final List<EditClip> clips;
  final List<EditText> texts;
  final List<EditSound> sounds;
  final List<EditJoin> joins;
  final Map<String, EditSource> sources;
  final int frameWidth;
  final int frameHeight;
  final double fps;
  final bool canUndo;
  final bool canRedo;
  final String? undoLabel;
  final String? redoLabel;

  /// Whether the cut differs from the one the AI made, and by how much of
  /// its footage.
  final bool edited;
  final double changeRatio;

  /// The project was cut again since this edit began.
  final bool newerRun;

  /// Whether this device can write a video; where it cannot, a cut leaves
  /// as a project file only.
  final bool canRenderVideo;

  /// The DSL's own aspect ratio, when one was set; the first video's
  /// otherwise.
  final String? aspectRatio;
  final EditSelection? selection;
  final double? bpm;
  final int? firstBeatUs;
  final int minSlotUs;
  final int minTextUs;
  final double minSpeed;
  final double maxSpeed;

  double get frameAspect => frameHeight == 0 ? 16 / 9 : frameWidth / frameHeight;

  /// Whether every clip's own sound is off.
  bool get allMuted => clips.where((c) => c.isClip).every((c) => c.muted);

  EditClip? clip(String id) {
    for (final c in clips) {
      if (c.id == id) return c;
    }
    return null;
  }

  EditText? text(String id) {
    for (final t in texts) {
      if (t.id == id) return t;
    }
    return null;
  }

  EditSound? sound(String id) {
    for (final s in sounds) {
      if (s.id == id) return s;
    }
    return null;
  }

  /// The clip on screen at `atUs`: the later of two a transition overlaps,
  /// the last at the very end.
  EditClip? clipAt(int atUs) {
    EditClip? found;
    for (final c in clips) {
      if (c.startUs <= atUs && atUs < c.endUs) found = c;
    }
    return found ?? (clips.isNotEmpty && atUs >= totalUs ? clips.last : null);
  }

  /// The clips on screen at `atUs`, earliest first: two inside a
  /// transition, one elsewhere.
  List<EditClip> clipsAt(int atUs) =>
      [for (final c in clips) if (c.startUs <= atUs && atUs < c.endUs) c];

  EditJoin? joinAfter(String slotId) {
    for (final j in joins) {
      if (j.fromSlotId == slotId) return j;
    }
    return null;
  }

  int indexOf(String slotId) => clips.indexWhere((c) => c.id == slotId);

  factory EditState.fromJson(Map<String, dynamic> json) {
    final dsl = _map(json['dsl']);
    final timeline = _map(json['timeline']);
    final tracks = _map(dsl['tracks']);

    final slotsById = {
      for (final s in _list(dsl['slots']))
        if (s is Map<String, dynamic>) s['id'] as String: s,
    };
    final videoBySlot = {
      for (final v in _list(tracks['video']))
        if (v is Map<String, dynamic>) v['slotId'] as String: v,
    };
    final ownSound = <String, Map<String, dynamic>>{};
    for (final a in _list(tracks['audio'])) {
      if (a is Map<String, dynamic> && a['kind'] == 'source') {
        ownSound[_map(a['anchor'])['slotId'] as String? ?? ''] = a;
      }
    }

    final clips = <EditClip>[];
    for (final r in _list(timeline['slots'])) {
      final resolved = _map(r);
      final id = resolved['slotId'] as String? ?? '';
      final slot = slotsById[id] ?? const {};
      final ref = _map(slot['sourceRef']);
      final video = videoBySlot[id] ?? const {};
      final keyframes = _list(_map(video['transform'])['keyframes']);
      final first = keyframes.isEmpty ? const <String, dynamic>{} : _map(keyframes.first);
      Adjustment adjustment = const Adjustment();
      for (final f in _list(video['filters'])) {
        final filter = _map(f);
        if (filter['name'] == 'eq') {
          final p = _map(filter['params']);
          adjustment = Adjustment(
            brightness: _double(p['brightness']),
            contrast: _double(p['contrast'], 1),
            saturation: _double(p['saturation'], 1),
          );
        }
      }
      final background = _map(video['background']);
      final sound = ownSound[id] ?? const {};
      clips.add(EditClip(
        id: id,
        kind: SlotKind.parse(slot['kind']),
        durationUs: _int(slot['durationUs']),
        startUs: _int(resolved['startUs']),
        endUs: _int(resolved['endUs']),
        overlapUs: _int(resolved['overlapUs']),
        sourceId: ref['sourceId'] as String?,
        inUs: _int(ref['inUs']),
        outUs: _int(ref['outUs']),
        speed: _double(slot['speed'], 1),
        reason: slot['reason'] as String?,
        placement: keyframes.isEmpty
            ? Placement.fitted
            : Placement(
                scale: _double(first['scale'], 1),
                x: _double(first['x']),
                y: _double(first['y']),
                rotationDeg: _double(first['rotationDeg']),
              ),
        adjustment: adjustment,
        backdrop: background.isEmpty
            ? null
            : Backdrop(background['kind'] as String? ?? 'black', background['color'] as String?),
        gainDb: _double(sound['gainDb']),
        muted: sound['muted'] == true,
      ));
    }

    final textsById = {
      for (final t in _list(tracks['text']))
        if (t is Map<String, dynamic>) t['id'] as String: t,
    };
    final texts = <EditText>[
      for (final r in _list(timeline['texts']))
        if (textsById[_map(r)['id']] case final t?)
          EditText(
            id: t['id'] as String,
            content: t['content'] as String? ?? '',
            startUs: _int(_map(r)['startUs']),
            endUs: _int(_map(r)['endUs']),
            durationUs: _int(t['durationUs']),
            slotId: _map(t['anchor'])['slotId'] as String? ?? '',
            role: t['role'] as String?,
            style: TextLook.fromJson(_map(t['style'])),
          ),
    ];

    final audioById = {
      for (final a in _list(tracks['audio']))
        if (a is Map<String, dynamic>) a['id'] as String: a,
    };
    final sounds = <EditSound>[
      for (final r in _list(timeline['audio']))
        if (_map(r)['kind'] != 'source')
          if (audioById[_map(r)['id']] case final a?)
            EditSound(
              id: a['id'] as String,
              kind: a['kind'] as String? ?? 'bgm',
              startUs: _int(_map(r)['startUs']),
              endUs: _int(_map(r)['endUs']),
              path: a['assetPath'] as String?,
              assetInUs: _int(a['assetInUs']),
              gainDb: _double(a['gainDb']),
              muted: a['muted'] == true,
              fadeInUs: _int(a['fadeInUs']),
              fadeOutUs: _int(a['fadeOutUs']),
            ),
    ];

    final joins = <EditJoin>[
      for (final t in _list(dsl['transitions']))
        if (t is Map<String, dynamic>)
          EditJoin(
            fromSlotId: t['fromSlotId'] as String? ?? '',
            toSlotId: t['toSlotId'] as String? ?? '',
            kind: TransitionKind.parse(t['type']),
            durationUs: _int(t['durationUs']),
          ),
    ];

    final frame = _map(json['frame']);
    final beat = _map(json['beat']);
    final limits = _map(json['limits']);
    return EditState(
      projectId: json['projectId'] as String? ?? '',
      title: json['title'] as String? ?? '',
      version: _int(json['version']),
      totalUs: _int(timeline['totalUs']),
      clips: clips,
      texts: texts,
      sounds: sounds,
      joins: joins,
      sources: {
        for (final s in _list(json['sources']))
          if (s is Map<String, dynamic>) s['id'] as String: EditSource.fromJson(s),
      },
      frameWidth: _int(frame['width'], 1920),
      frameHeight: _int(frame['height'], 1080),
      fps: _double(json['fps'], 30),
      canUndo: json['canUndo'] == true,
      canRedo: json['canRedo'] == true,
      undoLabel: json['undoLabel'] as String?,
      redoLabel: json['redoLabel'] as String?,
      edited: json['edited'] == true,
      changeRatio: _double(json['changeRatio']),
      newerRun: json['newerRun'] == true,
      canRenderVideo: json['canRenderVideo'] != false,
      aspectRatio: _map(dsl['meta'])['aspectRatio'] as String?,
      selection: EditSelection.fromJson(json['selection']),
      bpm: (beat['bpm'] as num?)?.toDouble(),
      firstBeatUs: (beat['firstBeatUs'] as num?)?.toInt(),
      minSlotUs: _int(limits['minSlotUs'], 100000),
      minTextUs: _int(limits['minTextUs'], 200000),
      minSpeed: _double(limits['minSpeed'], 0.25),
      maxSpeed: _double(limits['maxSpeed'], 4),
    );
  }
}

/// One usable stretch of a file of footage, to add from.
class FootageStretch {
  const FootageStretch({required this.startUs, required this.endUs, this.quality, this.label});
  final int startUs;
  final int endUs;
  final double? quality;
  final String? label;
}

/// A file of the project's footage, as the "add a clip" sheet lists it.
class FootageFile {
  const FootageFile({required this.source, required this.stretches, required this.used, this.capturedAt});
  final EditSource source;
  final List<FootageStretch> stretches;

  /// What of it the cut already shows, as footage ranges.
  final List<(int, int)> used;
  final String? capturedAt;

  factory FootageFile.fromJson(Map<String, dynamic> json) => FootageFile(
        source: EditSource.fromJson(json),
        capturedAt: json['capturedAt'] as String?,
        stretches: [
          for (final s in _list(json['stretches']))
            if (s is Map<String, dynamic>)
              FootageStretch(
                startUs: _int(s['usableStartUs'] ?? s['startUs']),
                endUs: _int(s['usableEndUs'] ?? s['endUs']),
                quality: (s['quality'] as num?)?.toDouble(),
                label: s['label'] as String?,
              ),
        ].where((s) => s.endUs > s.startUs).toList(growable: false),
        used: [
          for (final u in _list(json['used']))
            if (u is List && u.length == 2) (_int(u[0]), _int(u[1])),
        ],
      );
}

/// What an export wrote.
class ExportDone {
  const ExportDone({
    required this.path,
    this.warnings = const [],
    this.bytes,
    this.durationUs,
    this.copied,
    this.rendered,
  });
  final String path;
  final List<String> warnings;
  final int? bytes;
  final int? durationUs;

  /// For clips exported one by one: how many were copied as filmed, and
  /// how many rendered.
  final int? copied;
  final int? rendered;

  factory ExportDone.fromJson(Map<String, dynamic> json) => ExportDone(
        path: json['path'] as String? ?? '',
        warnings: [for (final w in _list(json['warnings'])) '$w'],
        bytes: (json['bytes'] as num?)?.toInt(),
        durationUs: (json['durationUs'] as num?)?.toInt(),
        copied: (json['copied'] as num?)?.toInt(),
        rendered: (json['rendered'] as num?)?.toInt(),
      );
}

/// Whether a cut can leave without being re-encoded, as the engine finds:
/// how much the copy adds, keyframe by keyframe, or why it cannot.
class LosslessCheck {
  const LosslessCheck({
    required this.possible,
    this.reasons = const [],
    this.extraUs = 0,
    this.totalUs = 0,
  });

  final bool possible;

  /// Why not, worded by the engine in the user's language.
  final List<String> reasons;

  /// How much earlier than their in points the clips start, together.
  final int extraUs;
  final int totalUs;

  factory LosslessCheck.fromJson(Map<String, dynamic> json) => LosslessCheck(
        possible: json['possible'] == true,
        reasons: [for (final r in _list(json['reasons'])) '$r'],
        extraUs: _int(json['extraUs']),
        totalUs: _int(json['totalUs']),
      );
}


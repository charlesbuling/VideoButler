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

/// The built-in editor (0.2.0).
///
/// A project's cut, opened to be finished here rather than in another app:
/// preview on top, the timeline under it with the playhead in the middle,
/// and the tools for whatever is selected along the bottom, as phone
/// editors lay it out. Every change is an operation the engine applies to
/// the Editing DSL and keeps on disk, so it can be undone, survives the app
/// closing, and leaves as a video or as a project file another editor
/// opens.
library;

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import '../models.dart';
import '../ui/file_browser.dart';
import 'edit_export.dart';
import 'edit_player.dart';
import 'edit_sheets.dart';
import 'edit_state.dart';
import 'edit_timeline.dart';
import 'picture_rules.dart';

class EditorPage extends StatefulWidget {
  const EditorPage({
    super.key,
    required this.engine,
    required this.projectId,
    required this.projectName,
    this.livePlayback = true,
  });

  final Engine engine;
  final String projectId;
  final String projectName;

  /// Play footage through the platform's player. Off in tests, where there
  /// is no player; playing then steps through the engine's frames, as the
  /// web build does.
  final bool livePlayback;

  @override
  State<EditorPage> createState() => _EditorPageState();
}

class _EditorPageState extends State<EditorPage> {
  EditState? _state;
  final ValueNotifier<EditState?> _live = ValueNotifier(null);
  String? _error;
  int _working = 0;
  int _playhead = 0;
  EditSelection? _selection;
  double _zoom = 60;
  List<EditorInfo> _editors = const [];

  // The still preview: the engine's own frame at the playhead.
  Uint8List? _frame;
  int? _frameWanted;
  bool _frameLoading = false;
  int _previewMaxSide = 640;

  // Film-strip pictures.
  final Map<String, Uint8List> _strip = {};
  final Set<String> _stripPending = {};

  // Playing.
  EditPlayer? _player;
  Timer? _stepper;
  final Stopwatch _stepClock = Stopwatch();
  int _stepOrigin = 0;

  // What the adjust sheet is trying, shown before it is committed.
  Adjustment? _adjustTrial;

  // A text being dragged in the preview.
  Offset? _dragCentre;
  final GlobalKey _textBox = GlobalKey();
  final GlobalKey _previewBox = GlobalKey();

  final FocusNode _focus = FocusNode();

  String get _id => widget.projectId;
  bool get _playing => (_player?.playing ?? false) || _stepper != null;

  @override
  void initState() {
    super.initState();
    unawaited(_open());
  }

  @override
  void dispose() {
    _stepper?.cancel();
    _player?.removeListener(_playerMoved);
    _player?.dispose();
    _live.dispose();
    _focus.dispose();
    unawaited(widget.engine.editClose(_id).catchError((_) {}));
    super.dispose();
  }

  Future<void> _open({bool restart = false}) async {
    try {
      final state = await widget.engine.editOpen(_id, restart: restart);
      if (!mounted) return;
      _take(state);
      _selection = null;
      _playhead = 0;
      _requestFrame();
      final editors = await widget.engine.editors().catchError((_) => const <EditorInfo>[]);
      if (mounted) setState(() => _editors = editors);
    } on EngineException catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  /// The cut as the engine now has it.
  void _take(EditState state) {
    setState(() {
      _state = state;
      _error = null;
      if (state.selection != null) {
        _selection = state.selection;
      } else if (_selection != null && !_exists(state, _selection!)) {
        _selection = null;
      }
      _playhead = _playhead.clamp(0, math.max(0, state.totalUs - 1));
    });
    _live.value = state;
    _player?.state = state;
  }

  static bool _exists(EditState state, EditSelection s) => switch (s.kind) {
        'slot' => state.clip(s.id) != null,
        'text' => state.text(s.id) != null,
        'music' => state.sound(s.id) != null,
        _ => false,
      };

  /// Send one operation; the engine answers with the new cut, or why not.
  Future<bool> _apply(Map<String, dynamic> op) async {
    if (_state == null) return false;
    if (_playing) _pause();
    setState(() => _working++);
    try {
      _take(await widget.engine.editApply(_id, op));
      _requestFrame();
      return true;
    } on EngineException catch (e) {
      _toast(e.message);
      return false;
    } finally {
      if (mounted) setState(() => _working--);
    }
  }

  Future<void> _step(Future<EditState> Function() call) async {
    if (_playing) _pause();
    setState(() => _working++);
    try {
      _take(await call());
      _requestFrame();
    } on EngineException catch (e) {
      _toast(e.message);
    } finally {
      if (mounted) setState(() => _working--);
    }
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  // --- the preview ------------------------------------------------------------

  /// Ask for the frame at the playhead; while one is on its way, only the
  /// latest moment asked for is fetched next.
  void _requestFrame() {
    if (_state == null) return;
    _frameWanted = _playhead;
    if (_frameLoading) return;
    _frameLoading = true;
    () async {
      while (_frameWanted != null && mounted) {
        final at = _frameWanted!;
        _frameWanted = null;
        try {
          final (png, _) = await widget.engine.editFrame(_id, at, maxSide: _previewMaxSide);
          if (!mounted) break;
          if (!_playing || _player == null) setState(() => _frame = png);
        } on EngineException catch (e) {
          if (mounted && _frameWanted == null) _toast(e.message);
        }
      }
      _frameLoading = false;
    }();
  }

  void _scrub(int atUs) {
    if (_playing) _pause();
    setState(() => _playhead = atUs);
    _requestFrame();
  }

  void _togglePlay() {
    final state = _state;
    if (state == null) return;
    if (_playing) {
      _pause();
      return;
    }
    final from = _playhead >= state.totalUs - 1000 ? 0 : _playhead;
    if (widget.livePlayback && EditPlayer.available) {
      // Photographs drawn by the engine at a size the preview cannot
      // outgrow, as the picker draws them.
      final player = _player ??= EditPlayer(
        state,
        drawStills: (paths) => widget.engine.thumbnails(paths, width: 1280),
      )..addListener(_playerMoved);
      player.state = state;
      player.play(from);
    } else {
      // No platform player: step through the engine's frames on the clock.
      _stepOrigin = from;
      _stepClock
        ..reset()
        ..start();
      _stepper = Timer.periodic(const Duration(milliseconds: 200), (_) {
        final at = _stepOrigin + _stepClock.elapsedMicroseconds;
        if (at >= state.totalUs) {
          _pause();
          return;
        }
        setState(() => _playhead = at);
        _requestFrame();
      });
    }
    setState(() {});
  }

  void _playerMoved() {
    final player = _player;
    if (player == null || !mounted) return;
    setState(() => _playhead = player.positionUs.clamp(0, math.max(0, (_state?.totalUs ?? 1) - 1)));
    if (!player.playing) _requestFrame();
  }

  void _pause() {
    _stepper?.cancel();
    _stepper = null;
    _stepClock.stop();
    final player = _player;
    if (player != null && player.playing) {
      player.pause();
      _playhead = player.positionUs.clamp(0, math.max(0, (_state?.totalUs ?? 1) - 1));
    }
    if (mounted) setState(() {});
    _requestFrame();
  }

  void _nudge(int frames) {
    final state = _state;
    if (state == null) return;
    final frame = (1e6 / (state.fps <= 0 ? 30 : state.fps)).round();
    _scrub((_playhead + frames * frame).clamp(0, math.max(0, state.totalUs - 1)));
  }

  // --- film strip ---------------------------------------------------------------

  void _needStrip(List<(String, int)> wanted) {
    final fresh = <(String, int)>[];
    for (final w in wanted) {
      final key = stripKey(w.$1, w.$2);
      if (_strip.containsKey(key) || _stripPending.contains(key)) continue;
      _stripPending.add(key);
      fresh.add(w);
    }
    if (fresh.isEmpty) return;
    final height = (56 * MediaQuery.of(context).devicePixelRatio).round().clamp(32, 200);
    for (var i = 0; i < fresh.length; i += 24) {
      final batch = fresh.sublist(i, math.min(fresh.length, i + 24));
      unawaited(widget.engine.editStrip(_id, batch, height: height).then((got) {
        if (!mounted) return;
        setState(() => _strip.addAll(got));
      }).catchError((_) {}).whenComplete(() {
        for (final (s, at) in batch) {
          _stripPending.remove(stripKey(s, at));
        }
      }));
    }
  }

  // --- what the tools do ----------------------------------------------------------

  EditClip? get _selectedClip =>
      _selection?.kind == 'slot' ? _state?.clip(_selection!.id) : null;
  EditText? get _selectedText =>
      _selection?.kind == 'text' ? _state?.text(_selection!.id) : null;
  EditSound? get _selectedSound =>
      _selection?.kind == 'music' ? _state?.sound(_selection!.id) : null;

  /// The clip a tool acts on: the one selected, else the one at the
  /// playhead.
  EditClip? get _targetClip => _selectedClip ?? _state?.clipAt(_playhead);

  Future<void> _split() async {
    final state = _state;
    if (state == null) return;
    final selected = _selectedClip;
    final inside = selected != null && selected.startUs < _playhead && _playhead < selected.endUs;
    await _apply({
      'op': 'split',
      'atUs': _playhead,
      if (inside) 'slotId': selected.id,
    });
  }

  Future<void> _deleteSelection() async {
    final selection = _selection;
    if (selection == null) return;
    switch (selection.kind) {
      case 'slot':
        await _apply({'op': 'delete', 'slotId': selection.id});
      case 'text':
        await _apply({'op': 'deleteText', 'id': selection.id});
      case 'music':
        await _apply({'op': 'deleteMusic', 'id': selection.id});
    }
  }

  Future<void> _move(EditClip clip, int by) async {
    final state = _state!;
    final index = state.indexOf(clip.id) + by;
    if (index < 0 || index >= state.clips.length) return;
    await _apply({'op': 'move', 'slotId': clip.id, 'toIndex': index});
  }

  Future<void> _addText() async {
    final s = S.of(context);
    final ok = await _apply({'op': 'addText', 'atUs': _playhead, 'content': s.edNewText});
    if (ok && mounted && _selectedText != null) _openText(_selectedText!);
  }

  void _openText(EditText text) {
    unawaited(showEditSheet(
      context,
      (_) => ValueListenableBuilder<EditState?>(
        valueListenable: _live,
        builder: (context, state, _) {
          final current = state?.text(text.id);
          if (state == null || current == null) return const SizedBox.shrink();
          return TextSheet(text: current, state: state, apply: _apply);
        },
      ),
    ));
  }

  Future<void> _chooseMusic() async {
    final chosen = await FileBrowserSheet.show(context, engine: widget.engine, kind: 'audio');
    if (chosen == null || chosen.isEmpty) return;
    await _apply({'op': 'setMusic', 'path': chosen.first, 'loop': true});
  }

  Future<void> _addClip() async {
    final state = _state;
    if (state == null) return;
    final List<FootageFile> files;
    try {
      files = await widget.engine.editFootage(_id);
    } on EngineException catch (e) {
      _toast(e.message);
      return;
    }
    if (!mounted) return;
    final pictures = <String, Uint8List>{};
    final wanted = [
      for (final f in files)
        (f.source.id, f.stretches.isEmpty ? 0 : f.source.isImage ? 0 : f.stretches.first.startUs ~/ 250000 * 250000),
    ];
    final got = await widget.engine.editStrip(_id, wanted, height: 108).catchError((_) => <String, Uint8List>{});
    for (final (id, at) in wanted) {
      final png = got[stripKey(id, at)];
      if (png != null) pictures[id] = png;
    }
    if (!mounted) return;
    final current = state.clipAt(_playhead);
    final atIndex = current == null ? state.clips.length : state.indexOf(current.id) + 1;
    await showEditSheet(
      context,
      (sheetContext) => FootageSheet(
        files: files,
        pictures: pictures,
        onChosen: (file, stretch) {
          Navigator.of(sheetContext).pop();
          final longest = file.source.isImage ? 3000000 : 5000000;
          unawaited(_apply({
            'op': 'insert',
            'sourceId': file.source.id,
            'inUs': stretch.startUs,
            'outUs': math.min(stretch.endUs, stretch.startUs + longest),
            'atIndex': atIndex,
          }));
        },
      ),
    );
  }

  Future<void> _rename() async {
    final state = _state;
    if (state == null) return;
    final s = S.of(context);
    final controller = TextEditingController(text: state.title);
    final title = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.edTitle),
        content: TextField(
          key: const Key('title-field'),
          controller: controller,
          autofocus: true,
          onSubmitted: (v) => Navigator.of(context).pop(v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(s.cancel)),
          FilledButton(onPressed: () => Navigator.of(context).pop(controller.text), child: Text(s.save)),
        ],
      ),
    );
    controller.dispose();
    if (title != null && title.trim().isNotEmpty && title.trim() != state.title) {
      await _apply({'op': 'rename', 'title': title.trim()});
    }
  }

  Future<void> _revert() async {
    final s = S.of(context);
    final sure = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.edRevertAi),
        content: Text(s.edRevertAiBody),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(false), child: Text(s.cancel)),
          FilledButton(onPressed: () => Navigator.of(context).pop(true), child: Text(s.edRevertAi)),
        ],
      ),
    );
    if (sure == true) await _step(() => widget.engine.editRevert(_id));
  }

  Future<void> _restartFromRun() async {
    final s = S.of(context);
    final sure = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.edRestartFromRun),
        content: Text(s.edRestartBody),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(false), child: Text(s.cancel)),
          FilledButton(onPressed: () => Navigator.of(context).pop(true), child: Text(s.edRestartFromRun)),
        ],
      ),
    );
    if (sure == true) {
      if (_playing) _pause();
      await _open(restart: true);
    }
  }

  Future<void> _export() async {
    if (_playing) _pause();
    final choice = await chooseExport(
      context,
      _editors,
      canRenderVideo: _state?.canRenderVideo ?? true,
      lossless: widget.engine.editLossless(_id),
    );
    if (choice == null || !mounted) return;
    await runExport(context, widget.engine, _id, choice);
  }

  void _sheet(Widget Function(BuildContext context, EditState state) build) {
    unawaited(showEditSheet(
      context,
      (_) => ValueListenableBuilder<EditState?>(
        valueListenable: _live,
        builder: (context, state, _) => state == null ? const SizedBox.shrink() : build(context, state),
      ),
    ));
  }

  void _clipSheet(String slotId, Widget Function(BuildContext, EditClip, EditState) build) => _sheet(
        (context, state) {
          final clip = state.clip(slotId);
          return clip == null ? const SizedBox.shrink() : build(context, clip, state);
        },
      );

  // --- layout -----------------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final state = _state;
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.space): _togglePlay,
        const SingleActivator(LogicalKeyboardKey.keyZ, meta: true): () {
          if (state?.canUndo ?? false) unawaited(_step(() => widget.engine.editUndo(_id)));
        },
        const SingleActivator(LogicalKeyboardKey.keyZ, control: true): () {
          if (state?.canUndo ?? false) unawaited(_step(() => widget.engine.editUndo(_id)));
        },
        const SingleActivator(LogicalKeyboardKey.keyZ, meta: true, shift: true): () {
          if (state?.canRedo ?? false) unawaited(_step(() => widget.engine.editRedo(_id)));
        },
        const SingleActivator(LogicalKeyboardKey.keyY, control: true): () {
          if (state?.canRedo ?? false) unawaited(_step(() => widget.engine.editRedo(_id)));
        },
        const SingleActivator(LogicalKeyboardKey.keyB, meta: true): () => unawaited(_split()),
        const SingleActivator(LogicalKeyboardKey.keyB, control: true): () => unawaited(_split()),
        const SingleActivator(LogicalKeyboardKey.delete): () => unawaited(_deleteSelection()),
        const SingleActivator(LogicalKeyboardKey.backspace): () => unawaited(_deleteSelection()),
        const SingleActivator(LogicalKeyboardKey.arrowLeft): () => _nudge(-1),
        const SingleActivator(LogicalKeyboardKey.arrowRight): () => _nudge(1),
      },
      child: Focus(
        focusNode: _focus,
        autofocus: true,
        child: Scaffold(
          appBar: AppBar(
            title: GestureDetector(
              onTap: state == null ? null : () => unawaited(_rename()),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(state?.title ?? widget.projectName, overflow: TextOverflow.ellipsis),
                  if (state != null)
                    Text(
                      state.edited ? s.edEdited((state.changeRatio * 100).round()) : s.edAsMade,
                      style: Theme.of(context).textTheme.labelSmall,
                    ),
                ],
              ),
            ),
            actions: [
              IconButton(
                key: const Key('editor-undo'),
                tooltip: state?.undoLabel == null ? s.edUndo : '${s.edUndo} · ${state!.undoLabel}',
                onPressed: state?.canUndo ?? false
                    ? () => unawaited(_step(() => widget.engine.editUndo(_id)))
                    : null,
                icon: const Icon(Icons.undo_rounded),
              ),
              IconButton(
                key: const Key('editor-redo'),
                tooltip: state?.redoLabel == null ? s.edRedo : '${s.edRedo} · ${state!.redoLabel}',
                onPressed: state?.canRedo ?? false
                    ? () => unawaited(_step(() => widget.engine.editRedo(_id)))
                    : null,
                icon: const Icon(Icons.redo_rounded),
              ),
              PopupMenuButton<String>(
                key: const Key('editor-menu'),
                onSelected: (v) => switch (v) {
                  'revert' => unawaited(_revert()),
                  'restart' => unawaited(_restartFromRun()),
                  'rename' => unawaited(_rename()),
                  _ => null,
                },
                itemBuilder: (_) => [
                  PopupMenuItem(value: 'rename', child: Text(s.edTitle)),
                  PopupMenuItem(value: 'revert', enabled: state?.edited ?? false, child: Text(s.edRevertAi)),
                  PopupMenuItem(value: 'restart', child: Text(s.edRestartFromRun)),
                ],
              ),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                child: FilledButton.icon(
                  key: const Key('editor-export'),
                  style: FilledButton.styleFrom(padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10)),
                  onPressed: state == null ? null : () => unawaited(_export()),
                  icon: const Icon(Icons.ios_share_rounded, size: 18),
                  label: Text(s.edExport),
                ),
              ),
            ],
            bottom: _working > 0
                ? const PreferredSize(
                    preferredSize: Size.fromHeight(2),
                    child: LinearProgressIndicator(minHeight: 2),
                  )
                : null,
          ),
          body: _body(context, s),
        ),
      ),
    );
  }

  Widget _body(BuildContext context, S s) {
    final state = _state;
    if (_error != null && state == null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(_error!, textAlign: TextAlign.center),
        ),
      );
    }
    if (state == null) {
      return Center(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          const CircularProgressIndicator(),
          const SizedBox(height: 12),
          Text(s.edOpening),
        ]),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (state.newerRun)
          MaterialBanner(
            content: Text(s.edNewerRun),
            actions: [
              TextButton(onPressed: () => unawaited(_restartFromRun()), child: Text(s.edRestartFromRun)),
            ],
          ),
        Expanded(child: _preview(context, state)),
        _transport(context, s, state),
        SizedBox(
          height: 176,
          child: EditTimeline(
            state: state,
            playheadUs: _playhead,
            zoom: _zoom,
            selection: _selection,
            strip: _strip,
            following: _playing,
            onScrub: _scrub,
            onSelect: (selection) => setState(() => _selection = selection),
            onTrim: (slotId, {inUs, outUs}) => unawaited(_apply({
              'op': 'trim',
              'slotId': slotId,
              if (inUs != null) 'inUs': inUs,
              if (outUs != null) 'outUs': outUs,
            })),
            onDuration: (slotId, durationUs) =>
                unawaited(_apply({'op': 'setDuration', 'slotId': slotId, 'durationUs': durationUs})),
            onJoin: (from) => _sheet((context, state) => transitionSheet(context, state, from, _apply)),
            onZoom: (z) => setState(() => _zoom = z),
            onNeedStrip: _needStrip,
            onAdd: () => unawaited(_addClip()),
          ),
        ),
        const Divider(height: 1),
        _toolbar(context, s, state),
      ],
    );
  }

  Widget _preview(BuildContext context, EditState state) {
    return ColoredBox(
      color: const Color(0xFF111111),
      child: LayoutBuilder(builder: (context, box) {
        final aspect = state.frameAspect;
        var w = box.maxWidth - 24;
        var h = w / aspect;
        if (h > box.maxHeight - 16) {
          h = box.maxHeight - 16;
          w = h * aspect;
        }
        if (w <= 0 || h <= 0) return const SizedBox.shrink();
        final dpr = MediaQuery.of(context).devicePixelRatio;
        final wantSide = (math.max(w, h) * dpr).round().clamp(320, 1280);
        if ((wantSide - _previewMaxSide).abs() > 64) {
          _previewMaxSide = wantSide;
          WidgetsBinding.instance.addPostFrameCallback((_) => _requestFrame());
        }
        final frameSize = Size(w, h);
        final player = _player;
        Widget picture;
        if (player != null && player.playing) {
          picture = LivePicture(player: player, atUs: _playhead);
        } else if (_frame != null) {
          picture = Image.memory(_frame!, fit: BoxFit.fill, gaplessPlayback: true);
          final trial = _adjustTrial;
          if (trial != null) {
            picture = ColorFiltered(colorFilter: ColorFilter.matrix(adjustMatrix(trial)), child: picture);
          }
        } else {
          picture = const Center(child: CircularProgressIndicator());
        }
        final text = _selectedText;
        final textShown = text != null && !_playing && text.startUs <= _playhead && _playhead < text.endUs;
        return Center(
          child: SizedBox(
            key: _previewBox,
            width: w,
            height: h,
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => setState(() => _selection = null),
              onPanStart: textShown ? (d) => _dragStart(d.localPosition, frameSize) : null,
              onPanUpdate: textShown
                  ? (d) => setState(() => _dragCentre = (_dragCentre ?? d.localPosition) + d.delta)
                  : null,
              onPanEnd: textShown ? (_) => _dragEnd(text, frameSize) : null,
              child: Stack(fit: StackFit.expand, children: [
                ClipRect(child: picture),
                if (textShown) ..._textHandle(text, frameSize),
              ]),
            ),
          ),
        );
      }),
    );
  }

  /// The selected text's outline over the still frame, and the text itself
  /// where it is being dragged to.
  List<Widget> _textHandle(EditText text, Size frame) {
    final centre = _dragCentre;
    if (centre == null) {
      return [
        Positioned.fill(
          child: IgnorePointer(
            child: TextOverlay(boxKey: _textBox, text: text, frame: frame, ghost: true),
          ),
        ),
      ];
    }
    final placed = EditText(
      id: text.id,
      content: text.content,
      startUs: text.startUs,
      endUs: text.endUs,
      durationUs: text.durationUs,
      slotId: text.slotId,
      role: text.role,
      style: text.style.copyWith(
        x: (centre.dx / frame.width).clamp(0.0, 1.0),
        y: (centre.dy / frame.height).clamp(0.0, 1.0),
      ),
    );
    return [
      Positioned.fill(child: IgnorePointer(child: TextOverlay(text: placed, frame: frame, selected: true))),
    ];
  }

  void _dragStart(Offset at, Size frame) {
    // Start from the text's own centre, wherever on it the drag began, so
    // it moves with the pointer rather than jumping to it.
    final box = _textBox.currentContext?.findRenderObject() as RenderBox?;
    final preview = _previewBox.currentContext?.findRenderObject() as RenderBox?;
    Offset? centre;
    if (box != null && preview != null && box.hasSize) {
      final topLeft = box.localToGlobal(Offset.zero, ancestor: preview);
      centre = topLeft + box.size.center(Offset.zero);
    }
    setState(() => _dragCentre = centre ?? at);
  }

  void _dragEnd(EditText text, Size frame) {
    final centre = _dragCentre;
    setState(() => _dragCentre = null);
    if (centre == null) return;
    unawaited(_apply({
      'op': 'updateText',
      'id': text.id,
      'style': text.style
          .copyWith(
            x: ((centre.dx / frame.width).clamp(0.0, 1.0) * 1000).round() / 1000,
            y: ((centre.dy / frame.height).clamp(0.0, 1.0) * 1000).round() / 1000,
          )
          .toJson(),
    }));
  }

  Widget _transport(BuildContext context, S s, EditState state) {
    final theme = Theme.of(context);
    final mono = theme.textTheme.bodySmall?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(children: [
        Text('${formatDuration(_playhead)} / ${formatDuration(state.totalUs)}', style: mono),
        const Spacer(),
        IconButton(
          tooltip: s.edPrevFrame,
          onPressed: () => _nudge(-1),
          icon: const Icon(Icons.chevron_left_rounded),
        ),
        IconButton.filled(
          key: const Key('editor-play'),
          tooltip: _playing ? s.edPause : s.edPlay,
          onPressed: _togglePlay,
          icon: Icon(_playing ? Icons.pause_rounded : Icons.play_arrow_rounded),
        ),
        IconButton(
          tooltip: s.edNextFrame,
          onPressed: () => _nudge(1),
          icon: const Icon(Icons.chevron_right_rounded),
        ),
        const Spacer(),
        IconButton(
          tooltip: s.edZoomOut,
          onPressed: () => setState(() => _zoom = (_zoom / 1.5).clamp(minZoom, maxZoom)),
          icon: const Icon(Icons.zoom_out_rounded),
        ),
        IconButton(
          tooltip: s.edZoomIn,
          onPressed: () => setState(() => _zoom = (_zoom * 1.5).clamp(minZoom, maxZoom)),
          icon: const Icon(Icons.zoom_in_rounded),
        ),
      ]),
    );
  }

  Widget _tool(String key, IconData icon, String label, VoidCallback? onTap) => SizedBox(
        width: 72,
        child: InkWell(
          key: Key('tool-$key'),
          onTap: onTap,
          borderRadius: BorderRadius.circular(10),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              Icon(icon, size: 22, color: onTap == null ? Theme.of(context).disabledColor : null),
              const SizedBox(height: 4),
              Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                      color: onTap == null ? Theme.of(context).disabledColor : null,
                    ),
              ),
            ]),
          ),
        ),
      );

  Widget _toolbar(BuildContext context, S s, EditState state) {
    final tools = <Widget>[];
    final clip = _selectedClip;
    final text = _selectedText;
    final sound = _selectedSound;
    if (clip != null) {
      final source = clip.sourceId == null ? null : state.sources[clip.sourceId];
      final photo = source?.isImage ?? false;
      final index = state.indexOf(clip.id);
      tools.addAll([
        _tool('back', Icons.arrow_back_rounded, s.edBack, () => setState(() => _selection = null)),
        if (clip.isClip) _tool('split', Icons.content_cut_rounded, s.edSplit, () => unawaited(_split())),
        _tool('delete', Icons.delete_outline_rounded, s.edDelete, () => unawaited(_deleteSelection())),
        if (clip.isClip && !photo)
          _tool('speed', Icons.speed_rounded, s.edSpeed,
              () => _clipSheet(clip.id, (c, cl, st) => speedSheet(c, cl, st, _apply))),
        if (clip.isClip && !photo)
          _tool('volume', clip.muted ? Icons.volume_off_rounded : Icons.volume_up_rounded, s.edVolume,
              () => _clipSheet(clip.id, (c, cl, st) => volumeSheet(c, cl, st, _apply))),
        if (!clip.isClip || photo)
          _tool('duration', Icons.timer_outlined, s.edDuration,
              () => _clipSheet(clip.id, (c, cl, st) => durationSheet(c, cl, st, _apply))),
        if (clip.isClip)
          _tool('adjust', Icons.tune_rounded, s.edAdjust, () => _clipSheet(
                clip.id,
                (c, cl, st) => adjustSheet(c, cl, _apply, onPreview: (a) => setState(() => _adjustTrial = a)),
              )),
        if (clip.isClip)
          _tool('position', Icons.open_with_rounded, s.edPosition,
              () => _clipSheet(clip.id, (c, cl, st) => positionSheet(c, cl, _apply))),
        _tool('background', Icons.wallpaper_rounded, s.edBackground,
            () => _clipSheet(clip.id, (c, cl, st) => canvasSheet(c, st, cl, _apply))),
        if (clip.isClip) _tool('duplicate', Icons.copy_rounded, s.edDuplicate,
            () => unawaited(_apply({'op': 'duplicate', 'slotId': clip.id}))),
        _tool('left', Icons.keyboard_double_arrow_left_rounded, s.edMoveEarlier,
            index > 0 ? () => unawaited(_move(clip, -1)) : null),
        _tool('right', Icons.keyboard_double_arrow_right_rounded, s.edMoveLater,
            index < state.clips.length - 1 ? () => unawaited(_move(clip, 1)) : null),
        if (index < state.clips.length - 1)
          _tool('transition', Icons.compare_arrows_rounded, s.edTransition,
              () => _sheet((c, st) => transitionSheet(c, st, clip.id, _apply))),
      ]);
    } else if (text != null) {
      tools.addAll([
        _tool('back', Icons.arrow_back_rounded, s.edBack, () => setState(() => _selection = null)),
        _tool('text-edit', Icons.edit_rounded, s.edEditText, () => _openText(text)),
        _tool('text-here', Icons.vertical_align_center_rounded, s.edTextToPlayhead,
            () => unawaited(_apply({'op': 'moveText', 'id': text.id, 'atUs': _playhead}))),
        _tool('delete', Icons.delete_outline_rounded, s.edDelete, () => unawaited(_deleteSelection())),
      ]);
    } else if (sound != null) {
      tools.addAll([
        _tool('back', Icons.arrow_back_rounded, s.edBack, () => setState(() => _selection = null)),
        _tool('music-edit', Icons.graphic_eq_rounded, s.edVolume, () => _sheet((c, st) {
              final current = st.sound(sound.id);
              return current == null
                  ? const SizedBox.shrink()
                  : musicSheet(c, current, _apply, onReplace: () => unawaited(_chooseMusic()));
            })),
        _tool('music-replace', Icons.swap_horiz_rounded, s.edReplaceMusic, () => unawaited(_chooseMusic())),
        _tool('delete', Icons.delete_outline_rounded, s.edDelete, () => unawaited(_deleteSelection())),
      ]);
    } else {
      final target = _targetClip;
      tools.addAll([
        _tool('add', Icons.add_box_outlined, s.edAddClip, () => unawaited(_addClip())),
        _tool('split', Icons.content_cut_rounded, s.edSplit,
            target?.isClip ?? false ? () => unawaited(_split()) : null),
        _tool('text', Icons.text_fields_rounded, s.edText, () => unawaited(_addText())),
        _tool('music', Icons.music_note_rounded, s.edMusic, () => unawaited(_chooseMusic())),
        _tool('canvas', Icons.aspect_ratio_rounded, s.edCanvas, () => _sheet((c, st) => canvasSheet(c, st, null, _apply))),
        _tool('transitions', Icons.compare_arrows_rounded, s.edTransitionAll,
            () => _sheet((c, st) => transitionSheet(c, st, null, _apply))),
        _tool('sound', state.allMuted ? Icons.volume_off_rounded : Icons.volume_up_rounded,
            state.allMuted ? s.edSoundOn : s.edSoundOff,
            () => unawaited(_apply({'op': 'muteAll', 'muted': !state.allMuted}))),
      ]);
    }
    return SafeArea(
      top: false,
      child: SizedBox(
        height: 72,
        child: ListView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          children: tools,
        ),
      ),
    );
  }
}

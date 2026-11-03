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

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../api_models.dart';
import '../editor/edit_export.dart';
import '../editor/editor_page.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../engine/platform_editors.dart';
import '../engine/platform_photos.dart';
import '../l10n/strings.dart';
import '../models.dart';
import '../theme.dart';
import 'file_browser.dart';
import 'models_page.dart';
import 'result_view.dart';
import 'run_progress.dart';
import 'settings_page.dart';
import 'widgets.dart';

/// The lengths offered for a finished piece, in seconds.
///
/// Short-video platforms settle on a handful of lengths, and a slider to the
/// second would be precision nobody needs for a rough cut.
const durationChoices = [15, 30, 60, 120, 180, 300];

/// One project, from footage to a draft in the editor.
///
/// The page reads top to bottom in the order the decisions are made: what
/// footage, in what style, how long, to what music, with which models, into
/// which editor, then run. Every choice is saved as it is made, so closing the
/// app mid-way loses nothing and reopening the project picks up where it was.
class ProjectPage extends StatefulWidget {
  const ProjectPage({super.key, required this.engine, required this.project});

  final Engine engine;
  final Project project;

  @override
  State<ProjectPage> createState() => _ProjectPageState();
}

class _ProjectPageState extends State<ProjectPage> {
  late Project _project;

  List<EditorInfo> _editors = const [];
  ModelLibrary? _models;
  AppSettings? _settings;
  DoctorReport? _doctor;

  /// What the engine heard in the chosen music, once it has listened.
  MusicInfo? _music;
  bool _readingMusic = false;

  /// Look at every clip again rather than reuse what earlier runs found.
  /// A one-off choice for the next run, not saved with the project: a
  /// project that kept it would re-analyse hours of footage every time.
  bool _fresh = false;

  String? _jobId;
  JobStatus? _status;
  Timer? _poll;
  bool _cancelling = false;
  RunResult? _result;
  String? _runError;

  @override
  void initState() {
    super.initState();
    _project = widget.project;
    unawaited(_loadEnvironment());
    unawaited(_inspectMusic());
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  Future<void> _loadEnvironment() async {
    try {
      final results = await Future.wait<Object>([
        widget.engine.editors(),
        widget.engine.models(),
        widget.engine.settings(),
        widget.engine.doctor(),
      ]);
      if (!mounted) return;
      setState(() {
        _editors = results[0] as List<EditorInfo>;
        _models = results[1] as ModelLibrary;
        _settings = results[2] as AppSettings;
        _doctor = results[3] as DoctorReport;
      });
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  bool get _running => _jobId != null && !(_status?.finished ?? false);

  // --- saving -------------------------------------------------------------

  Future<void> _save({
    String? name,
    List<String>? inputs,
    StyleChoice? style,
    ModelChoice? model,
    List<String>? editors,
  }) async {
    try {
      final updated = await widget.engine.updateProject(
        _project.id,
        name: name,
        inputs: inputs,
        style: style,
        model: model,
        editors: editors,
      );
      if (!mounted) return;
      setState(() => _project = updated);
    } on EngineException catch (e) {
      _toast(S.of(context).couldNotSave(e.message));
    }
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  // --- footage and music ----------------------------------------------------

  Future<void> _addFootage() async {
    // On iPhone the footage is in the photo library; the files the app
    // holds are one tap further, for what was put there through Files.
    if (PlatformPhotos.available) {
      final s = S.of(context);
      final fromLibrary = await showModalBottomSheet<bool>(
        context: context,
        showDragHandle: true,
        builder: (context) => SafeArea(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ListTile(
                key: const Key('import-photos'),
                leading: const Icon(Icons.photo_library_outlined),
                title: Text(s.importFromPhotos),
                subtitle: Text(s.importFromPhotosNote),
                onTap: () => Navigator.of(context).pop(true),
              ),
              ListTile(
                key: const Key('browse-files'),
                leading: const Icon(Icons.folder_outlined),
                title: Text(s.browseAppFiles),
                onTap: () => Navigator.of(context).pop(false),
              ),
            ],
          ),
        ),
      );
      if (fromLibrary == null || !mounted) return;
      if (fromLibrary) {
        final imported = await PlatformPhotos.importFromLibrary();
        if (imported.isEmpty) return;
        await _save(inputs: [
          ..._project.inputs,
          for (final path in imported)
            if (!_project.inputs.contains(path)) path,
        ]);
        return;
      }
    }
    if (!mounted) return;
    final chosen = await FileBrowserSheet.show(
      context,
      engine: widget.engine,
      startDirectory: _project.inputs.isEmpty
          ? null
          : parentDirectoryOf(_project.inputs.last),
    );
    if (chosen == null || chosen.isEmpty) return;
    final merged = [
      ..._project.inputs,
      for (final path in chosen)
        if (!_project.inputs.contains(path)) path,
    ];
    await _save(inputs: merged);
  }

  Future<void> _removeInput(String path) async {
    await _save(inputs: _project.inputs.where((p) => p != path).toList());
  }

  Future<void> _chooseMusic() async {
    final chosen = await FileBrowserSheet.show(
      context,
      engine: widget.engine,
      kind: 'audio',
      startDirectory: parentDirectoryOf(_project.style.music),
    );
    if (chosen == null || chosen.isEmpty) return;
    await _save(style: _project.style.copyWith(music: chosen.first));
    await _inspectMusic();
  }

  Future<void> _removeMusic() async {
    await _save(style: _project.style.copyWith(clearMusic: true));
    setState(() => _music = null);
  }

  /// Listen to the chosen music for its length and tempo, so the card can
  /// say "3:04 · 120 BPM" before a run -- and say "no clear beat" before a
  /// user builds a beat cut on a track that has none.
  Future<void> _inspectMusic() async {
    final path = _project.style.music;
    if (path == null || path.isEmpty) return;
    setState(() => _readingMusic = true);
    try {
      final info = await widget.engine.inspectMusic(path);
      if (!mounted) return;
      setState(() => _music = info);
    } on EngineException catch (e) {
      _toast(e.message);
    } finally {
      if (mounted) setState(() => _readingMusic = false);
    }
  }

  // --- running ------------------------------------------------------------

  /// Music shorter than the cut: ask, once per project, whether to play it
  /// again. False when the user backed out of the question.
  Future<bool> _settleMusicLength() async {
    final RunPreflight check;
    try {
      check = await widget.engine.preflight(_project.id);
    } on EngineException {
      // The question is a courtesy; a run without it still works.
      return true;
    }
    if (!check.askMusicLoop || !mounted) return true;
    final s = S.of(context);
    final loop = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('music-short'),
        icon: const Icon(Icons.music_off_outlined),
        title: Text(s.musicShortTitle),
        content: Text(s.musicShortBody(
          formatDuration(check.musicDurationUs ?? 0).split('.').first,
          formatDuration(check.cutDurationUs ?? 0).split('.').first,
        )),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(s.cancel),
          ),
          TextButton(
            key: const Key('music-no-loop'),
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(s.dontLoopMusic),
          ),
          FilledButton(
            key: const Key('music-loop'),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.loopMusic),
          ),
        ],
      ),
    );
    if (loop == null) return false;
    await _save(style: _project.style.copyWith(musicLoop: loop));
    return true;
  }

  Future<void> _start() async {
    if (!await _settleMusicLength() || !mounted) return;
    setState(() {
      _runError = null;
      _result = null;
      _status = null;
      _cancelling = false;
    });
    try {
      final id = await widget.engine.startRun(_project.id, fresh: _fresh);
      if (!mounted) return;
      // Asked for once; the run after this one reuses what this one finds.
      setState(() {
        _jobId = id;
        _fresh = false;
      });
      _poll?.cancel();
      _poll = Timer.periodic(
        const Duration(milliseconds: 400),
        (_) => unawaited(_check()),
      );
      unawaited(_check());
    } on EngineException catch (e) {
      setState(() => _runError = e.message);
    }
  }

  Future<void> _check() async {
    final id = _jobId;
    if (id == null) return;
    try {
      final status = await widget.engine.jobStatus(id);
      if (!mounted) return;
      setState(() => _status = status);
      if (!status.finished) return;

      _poll?.cancel();
      final s = S.of(context);
      setState(() {
        switch (status.state) {
          case 'done':
            _result = runResultOf(status);
          case 'cancelled':
            _runError = s.stoppedKept;
          default:
            _runError = status.error ?? s.runFailed;
        }
        _jobId = null;
      });
      // The engine updated the project's record of its last run.
      final refreshed = await widget.engine.projects();
      if (!mounted) return;
      for (final project in refreshed) {
        if (project.id == _project.id) setState(() => _project = project);
      }
    } on EngineException catch (e) {
      _poll?.cancel();
      if (!mounted) return;
      setState(() {
        _runError = e.message;
        _jobId = null;
      });
    }
  }

  /// The project as the engine now has it: a run, an edit or an export
  /// changes what it records.
  Future<void> _refreshProject() async {
    try {
      final refreshed = await widget.engine.projects();
      if (!mounted) return;
      for (final project in refreshed) {
        if (project.id == _project.id) setState(() => _project = project);
      }
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  /// Finish the cut here, in the built-in editor.
  Future<void> _openCutEditor() async {
    await Navigator.of(context).push(MaterialPageRoute<void>(
      builder: (_) => EditorPage(
        engine: widget.engine,
        projectId: _project.id,
        projectName: _project.name,
      ),
    ));
    await _refreshProject();
  }

  /// Send the cut out as it is -- the person's edit when there is one, the
  /// AI's cut otherwise: a video, or a project file for another editor.
  Future<void> _exportCut() async {
    // The export goes through the cut's editor session, which also says
    // whether this device can write a video.
    final bool canRenderVideo;
    try {
      canRenderVideo = (await widget.engine.editOpen(_project.id)).canRenderVideo;
    } on EngineException catch (e) {
      _toast(e.message);
      return;
    }
    if (!mounted) return;
    final choice = await chooseExport(
      context,
      _editors,
      canRenderVideo: canRenderVideo,
      lossless: widget.engine.editLossless(_project.id),
    );
    if (choice == null || !mounted) return;
    await runExport(context, widget.engine, _project.id, choice);
    await _refreshProject();
  }

  Future<void> _cancel() async {
    final id = _jobId;
    if (id == null) return;
    setState(() => _cancelling = true);
    try {
      await widget.engine.cancelJob(id);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  // --- other actions --------------------------------------------------------

  Future<void> _rename() async {
    final s = S.of(context);
    final controller = TextEditingController(text: _project.name);
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.renameProject),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: InputDecoration(labelText: s.name),
          onSubmitted: (value) => Navigator.of(context).pop(value),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(s.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(controller.text),
            child: Text(s.save),
          ),
        ],
      ),
    );
    if (name != null && name.trim().isNotEmpty) await _save(name: name.trim());
  }

  Future<void> _delete() async {
    final s = S.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.deleteProjectTitle(_project.name)),
        content: Text(s.deleteProjectBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(s.keepIt),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.delete),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await widget.engine.deleteProject(_project.id);
      if (mounted) Navigator.of(context).pop();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _openEditor(String id) async {
    try {
      await widget.engine.openEditor(id);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  /// Cut the clips of the last run and open them in a phone editor, with
  /// the clips counted off while the phone cuts them.
  Future<void> _handOff(EditorInfo editor) async {
    final result = _result;
    final package = editor.appPath;
    if (result == null || package == null) return;
    final s = S.of(context);
    final progress = ValueNotifier<(int, int)>((0, result.handoff.clips.length));
    unawaited(showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (context) => AlertDialog(
        title: Text(s.handOffTitle(editor.name)),
        content: ValueListenableBuilder<(int, int)>(
          valueListenable: progress,
          builder: (context, value, _) => Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(s.handOffCutting(value.$1, value.$2)),
              const SizedBox(height: 12),
              LinearProgressIndicator(value: value.$2 == 0 ? null : value.$1 / value.$2),
            ],
          ),
        ),
      ),
    ));
    String message;
    try {
      final folder = await PlatformEditors.handOff(
        package: package,
        folder: _project.name,
        handoff: result.handoff,
        onProgress: (done, total) => progress.value = (done, total),
      );
      message = s.handOffDone(editor.name, folder ?? '');
    } on PlatformException catch (e) {
      message = s.handOffFailed(editor.name, e.message ?? e.code);
    }
    if (!mounted) return;
    Navigator.of(context, rootNavigator: true).pop();
    progress.dispose();
    _toast(message);
  }

  Future<void> _reveal(String path) async {
    try {
      await widget.engine.reveal(path);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _openPage(Widget page) async {
    await Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => page));
    // Whatever was changed there -- a model downloaded, an endpoint set --
    // changes what this page offers.
    await _loadEnvironment();
  }

  // --- layout ---------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return Scaffold(
      appBar: AppBar(
        title: Text(_project.name),
        actions: [
          IconButton(
            tooltip: s.rename,
            icon: const Icon(Icons.edit_outlined),
            onPressed: _running ? null : () => unawaited(_rename()),
          ),
          if (_project.dir.isNotEmpty)
            IconButton(
              tooltip: s.showProjectFolder,
              icon: const Icon(Icons.folder_open_outlined),
              onPressed: () => unawaited(_reveal(_project.dir)),
            ),
          IconButton(
            tooltip: s.deleteProject,
            icon: const Icon(Icons.delete_outline_rounded),
            onPressed: _running ? null : () => unawaited(_delete()),
          ),
          const SizedBox(width: 4),
        ],
      ),
      body: Align(
        alignment: Alignment.topCenter,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 880),
          child: ListView(
            padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
            children: [
              _footageCard(theme, s),
              const SizedBox(height: 16),
              _styleCard(theme, s),
              const SizedBox(height: 16),
              _lengthCard(theme, s),
              const SizedBox(height: 16),
              _musicCard(theme, s),
              const SizedBox(height: 16),
              _modelCard(theme, s),
              const SizedBox(height: 16),
              _exportCard(theme, s),
              const SizedBox(height: 20),
              if (!_running) ...[
                _analysisChoice(theme, s),
                const SizedBox(height: 12),
              ],
              if (_status != null && _running)
                RunProgressCard(
                  status: _status!,
                  onCancel: () => unawaited(_cancel()),
                  cancelling: _cancelling,
                )
              else
                _runButton(theme, s),
              if (_runError != null) ...[
                const SizedBox(height: 16),
                NoticeBanner(
                  title: s.noDraftThisTime,
                  lines: [_runError!],
                  tone: Tone.bad,
                  icon: Icons.error_outline_rounded,
                ),
              ],
              if (!_running && (_result != null || _project.lastRun != null)) ...[
                const SizedBox(height: 28),
                _cutCard(theme, s),
              ],
              if (_result != null) ...[
                const SizedBox(height: 16),
                ResultView(
                  result: _result!,
                  onOpenEditor: (id) => unawaited(_openEditor(id)),
                  onReveal: (path) => unawaited(_reveal(path)),
                  phoneEditors: [
                    for (final editor in _editors)
                      if (editor.phone && editor.installed && editor.appPath != null) editor,
                  ],
                  onHandOff: PlatformEditors.available
                      ? (editor) => unawaited(_handOff(editor))
                      : null,
                ),
              ] else if (_project.lastRun != null && !_running) ...[
                const SizedBox(height: 20),
                _lastRunCard(theme, s, _project.lastRun!),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _footageCard(ThemeData theme, S s) {
    const shown = 6;
    final inputs = _project.inputs;
    return SectionCard(
      title: s.footage,
      subtitle: inputs.isEmpty ? s.footageEmptySubtitle : s.itemsChosen(inputs.length),
      icon: Icons.video_library_outlined,
      trailing: OutlinedButton.icon(
        key: const Key('add-footage'),
        onPressed: _running ? null : () => unawaited(_addFootage()),
        icon: const Icon(Icons.add_rounded, size: 18),
        label: Text(s.addFootage),
      ),
      child: inputs.isEmpty
          ? Text(
              s.noVideosYet,
              style: theme.textTheme.bodyMedium
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final path in inputs.take(shown))
                  Row(
                    children: [
                      Icon(
                        _looksLikeFolder(path)
                            ? Icons.folder_rounded
                            : looksLikePhoto(path)
                                ? Icons.photo_outlined
                                : Icons.movie_creation_outlined,
                        size: 16,
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          path,
                          style: monoStyle(context, size: 12),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      IconButton(
                        tooltip: s.remove,
                        visualDensity: VisualDensity.compact,
                        icon: const Icon(Icons.close_rounded, size: 16),
                        onPressed:
                            _running ? null : () => unawaited(_removeInput(path)),
                      ),
                    ],
                  ),
                if (inputs.length > shown)
                  Text(
                    s.andMore(inputs.length - shown),
                    style: theme.textTheme.bodySmall
                        ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                  ),
              ],
            ),
    );
  }

  /// Videos have extensions; the picker returns folders without one.
  bool _looksLikeFolder(String path) {
    final name = path.split(RegExp(r'[/\\]')).last;
    return !name.contains('.');
  }

  Widget _styleCard(ThemeData theme, S s) {
    final style = _project.style;
    final music = style.music;
    return SectionCard(
      title: s.style,
      subtitle: s.styleSubtitle,
      icon: Icons.palette_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(s.mood, style: theme.textTheme.labelLarge),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              ChoiceChip(
                label: Text(s.automatic),
                selected: style.mood == null,
                onSelected: _running
                    ? null
                    : (_) => unawaited(_save(style: style.copyWith(clearMood: true))),
              ),
              for (final mood in Mood.values)
                ChoiceChip(
                  key: Key('mood-${mood.id}'),
                  label: Text(s.moodLabel(mood.id)),
                  selected: style.mood == mood,
                  onSelected: _running
                      ? null
                      : (_) => unawaited(_save(style: style.copyWith(mood: mood))),
                ),
            ],
          ),
          const SizedBox(height: 16),
          Text(s.topic, style: theme.textTheme.labelLarge),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              ChoiceChip(
                label: Text(s.automatic),
                selected: style.topic == null,
                onSelected: _running
                    ? null
                    : (_) => unawaited(_save(style: style.copyWith(clearTopic: true))),
              ),
              for (final topic in Topic.values)
                ChoiceChip(
                  key: Key('topic-${topic.id}'),
                  label: Text(s.topicLabel(topic.id)),
                  selected: style.topic == topic,
                  onSelected: _running
                      ? null
                      : (_) => unawaited(_save(style: style.copyWith(topic: topic))),
                ),
            ],
          ),
          if (style.topic == Topic.continuity) ...[
            const SizedBox(height: 10),
            Text(
              s.continuityHint,
              key: const Key('continuity-hint'),
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ],
          const SizedBox(height: 12),
          SwitchListTile(
            key: const Key('beat-sync'),
            contentPadding: EdgeInsets.zero,
            title: Text(s.cutOnTheBeat),
            subtitle: Text(
              music != null && music.isNotEmpty ? s.beatWithMusic : s.beatWithoutMusic,
            ),
            value: style.beatSync ?? false,
            onChanged: _running
                ? null
                : (value) => unawaited(_save(style: style.copyWith(beatSync: value))),
          ),
        ],
      ),
    );
  }

  Widget _lengthCard(ThemeData theme, S s) {
    final style = _project.style;
    final chosen = style.targetDurationUs == null ? null : style.targetDurationUs! ~/ 1000000;
    return SectionCard(
      title: s.length,
      subtitle: s.lengthSubtitle,
      icon: Icons.timer_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              ChoiceChip(
                key: const Key('duration-auto'),
                label: Text(s.automatic),
                selected: chosen == null,
                onSelected: _running
                    ? null
                    : (_) => unawaited(_save(style: style.copyWith(clearDuration: true))),
              ),
              for (final seconds in durationChoices)
                ChoiceChip(
                  key: Key('duration-$seconds'),
                  label: Text(s.durationChoice(seconds)),
                  selected: chosen == seconds,
                  onSelected: _running
                      ? null
                      : (_) => unawaited(_save(
                            style: style.copyWith(targetDurationUs: seconds * 1000000),
                          )),
                ),
            ],
          ),
          const SizedBox(height: 10),
          Text(
            style.topic == Topic.continuity ? s.lengthNoteContinuity : s.lengthNote,
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        ],
      ),
    );
  }

  Widget _musicCard(ThemeData theme, S s) {
    final music = _project.style.music;
    final hasMusic = music != null && music.isNotEmpty;
    final info = _music;
    return SectionCard(
      title: s.music,
      subtitle: s.musicSubtitle,
      icon: Icons.music_note_rounded,
      trailing: FilledButton.tonalIcon(
        key: const Key('import-music'),
        onPressed: _running ? null : () => unawaited(_chooseMusic()),
        icon: const Icon(Icons.library_music_outlined, size: 18),
        label: Text(hasMusic ? s.change : s.importMusic),
      ),
      child: !hasMusic
          ? Text(
              s.noMusic,
              style: theme.textTheme.bodyMedium
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    Icon(Icons.audiotrack_rounded, size: 18, color: theme.colorScheme.primary),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            music.split(RegExp(r'[/\\]')).last,
                            style: theme.textTheme.bodyMedium
                                ?.copyWith(fontWeight: FontWeight.w600),
                            overflow: TextOverflow.ellipsis,
                          ),
                          const SizedBox(height: 2),
                          Text(
                            _readingMusic
                                ? s.musicReading
                                : info == null
                                    ? music
                                    : s.musicFacts(
                                        formatDuration(info.durationUs ?? 0).split('.').first,
                                        info.bpm,
                                        info.steady,
                                      ),
                            key: const Key('music-facts'),
                            style: theme.textTheme.bodySmall
                                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ],
                      ),
                    ),
                    TextButton(
                      onPressed: _running ? null : () => unawaited(_removeMusic()),
                      child: Text(s.remove),
                    ),
                  ],
                ),
                SwitchListTile(
                  key: const Key('music-loop-switch'),
                  contentPadding: EdgeInsets.zero,
                  value: _project.style.musicLoop ?? false,
                  onChanged: _running
                      ? null
                      : (on) => unawaited(_save(style: _project.style.copyWith(musicLoop: on))),
                  title: Text(s.musicLoopSwitch),
                  // A 主体衔接 cut's automatic length is the action's, unknown
                  // until analysis, so the run never asks: the switch says
                  // what each way does.
                  subtitle: _project.style.topic == Topic.continuity
                      ? Text(s.musicLoopContinuity, key: const Key('music-loop-continuity'))
                      : Text(s.musicLoopSwitchNote),
                ),
              ],
            ),
    );
  }

  Widget _modelCard(ThemeData theme, S s) {
    final cloud = _settings?.cloud;
    final speech =
        _models?.installed.where((m) => m.role == 'speech').toList() ?? const [];
    final describe =
        _models?.installed.where((m) => m.role == 'describe').toList() ?? const [];
    final pose = _models?.installed.where((m) => m.role == 'pose').toList() ?? const [];
    final vision = _doctor?.adapters.where((a) => a.role == 'Vision').firstOrNull;
    final framing = _doctor?.adapters.where((a) => a.role == 'Framing').firstOrNull;
    final framingReady = framing != null && !framing.mock;
    final cloudReady = _project.model == ModelChoice.cloud && (cloud?.configured ?? false);

    return SectionCard(
      title: s.models,
      subtitle: s.modelsSubtitle,
      icon: Icons.memory_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SegmentedButton<ModelChoice>(
            segments: [
              ButtonSegment(
                value: ModelChoice.local,
                icon: const Icon(Icons.laptop_mac_rounded),
                label: Text(s.local),
              ),
              ButtonSegment(
                value: ModelChoice.cloud,
                icon: const Icon(Icons.cloud_outlined),
                label: Text(s.cloudApi),
              ),
            ],
            selected: {_project.model},
            onSelectionChanged: _running
                ? null
                : (choice) => unawaited(_save(model: choice.first)),
          ),
          const SizedBox(height: 14),
          _modelLine(
            theme,
            s.roleSpeech,
            speech.isEmpty ? s.noSpeechModel : s.speechOnThisMac(speech.first.name),
            ok: speech.isNotEmpty,
            action: TextButton(
              onPressed: () => unawaited(_openPage(ModelsPage(engine: widget.engine))),
              child: Text(speech.isEmpty ? s.getAModel : s.models),
            ),
          ),
          _modelLine(
            theme,
            s.roleVision,
            cloudReady
                ? s.visionOverEndpoint(
                    cloud!.visionModel.isEmpty ? cloud.model : cloud.visionModel,
                    cloud.baseUrl,
                  )
                : vision == null || vision.mock
                    ? s.visionNone
                    : vision.name == 'SigLIP2'
                        ? s.visionSiglip
                        : s.visionApple,
            ok: vision != null && !vision.mock || cloudReady,
          ),
          _modelLine(
            theme,
            s.roleFraming,
            framingReady ? s.framingOnThisMac : s.noFramingModel,
            ok: framingReady,
            action: framingReady
                ? null
                : TextButton(
                    onPressed: () => unawaited(_openPage(ModelsPage(engine: widget.engine))),
                    child: Text(s.getAModel),
                  ),
          ),
          _modelLine(
            theme,
            s.roleCaptions,
            describe.isEmpty ? s.noCaptionModel : s.captionsOnThisMac(describe.first.name),
            ok: describe.isNotEmpty,
            action: describe.isNotEmpty
                ? null
                : TextButton(
                    onPressed: () => unawaited(_openPage(ModelsPage(engine: widget.engine))),
                    child: Text(s.getAModel),
                  ),
          ),
          _modelLine(
            theme,
            s.rolePose,
            pose.isEmpty ? s.noPoseModel : s.poseOnThisMac(pose.first.name),
            ok: pose.isNotEmpty,
            action: pose.isNotEmpty
                ? null
                : TextButton(
                    onPressed: () => unawaited(_openPage(ModelsPage(engine: widget.engine))),
                    child: Text(s.getAModel),
                  ),
          ),
          if (_project.model == ModelChoice.cloud && !(cloud?.configured ?? false))
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: NoticeBanner(
                title: s.noEndpoint,
                lines: [s.noEndpointBody],
                icon: Icons.cloud_off_outlined,
              ),
            ),
          if (_project.model == ModelChoice.cloud)
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                onPressed: () => unawaited(_openPage(SettingsPage(engine: widget.engine))),
                icon: const Icon(Icons.settings_outlined, size: 18),
                label: Text(s.cloudSettings),
              ),
            ),
        ],
      ),
    );
  }

  Widget _modelLine(
    ThemeData theme,
    String role,
    String detail, {
    required bool ok,
    Widget? action,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Icon(
            ok ? Icons.check_circle_rounded : Icons.info_outline_rounded,
            size: 18,
            color: ok ? theme.colorScheme.primary : theme.colorScheme.tertiary,
          ),
          const SizedBox(width: 10),
          SizedBox(width: 64, child: Text(role, style: theme.textTheme.labelLarge)),
          Expanded(child: Text(detail, style: theme.textTheme.bodySmall)),
          if (action != null) action,
        ],
      ),
    );
  }

  Widget _exportCard(ThemeData theme, S s) {
    return SectionCard(
      title: s.whereDraftGoes,
      subtitle: s.whereDraftGoesSubtitle,
      icon: Icons.output_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final editor in _editors)
            CheckboxListTile(
              key: Key('editor-${editor.id}'),
              contentPadding: EdgeInsets.zero,
              value: _project.editors.contains(editor.id),
              onChanged: !editor.canExport || _running
                  ? null
                  : (on) => unawaited(_save(
                        editors: [
                          ..._project.editors.where((e) => e != editor.id),
                          if (on ?? false) editor.id,
                        ],
                      )),
              title: Text(
                editor.version == null ? editor.name : '${editor.name} ${editor.version}',
              ),
              subtitle: Text(
                !editor.installed
                    ? s.editorNotInstalled
                    : editor.phone
                        ? s.editorOnPhone(editor.name)
                        : !editor.canExport
                            ? s.editorNoFolder
                        : editor.running
                            ? s.editorRunning
                            : s.editorReady,
              ),
            ),
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.check_box_rounded),
            title: Text(s.otioDraft),
            subtitle: Text(s.otioAlways),
            enabled: false,
          ),
        ],
      ),
    );
  }

  /// Reuse earlier analysis, or look at everything again.
  Widget _analysisChoice(ThemeData theme, S s) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SegmentedButton<bool>(
          key: const Key('analysis-choice'),
          segments: [
            ButtonSegment(
              value: false,
              icon: const Icon(Icons.history_rounded),
              label: Text(s.analysisReuse),
            ),
            ButtonSegment(
              value: true,
              icon: const Icon(Icons.refresh_rounded),
              label: Text(s.analysisFresh),
            ),
          ],
          selected: {_fresh},
          onSelectionChanged: (choice) => setState(() => _fresh = choice.first),
        ),
        const SizedBox(height: 6),
        Text(
          s.analysisChoiceNote,
          style: theme.textTheme.bodySmall
              ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
        ),
      ],
    );
  }

  Widget _runButton(ThemeData theme, S s) {
    final ready = _project.inputs.isNotEmpty && (_doctor?.canAnalyse ?? true);
    final editorNames = [
      for (final editor in _editors)
        if (_project.editors.contains(editor.id)) editor.name,
    ];
    return SizedBox(
      height: 56,
      child: FilledButton.icon(
        key: const Key('run'),
        onPressed: ready && !_running ? () => unawaited(_start()) : null,
        icon: _jobId != null
            ? const SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2.4),
              )
            : const Icon(Icons.auto_fix_high_rounded),
        label: Text(
          editorNames.isEmpty ? s.createDraft : s.createDraftFor(editorNames.join('、')),
          style: theme.textTheme.titleMedium?.copyWith(color: theme.colorScheme.onPrimary),
        ),
      ),
    );
  }

  /// What to do with the cut, once there is one: finish it here, or send
  /// it out as a video or as a project file another editor opens.
  Widget _cutCard(ThemeData theme, S s) {
    final edited = _project.editedAt;
    final exports = _project.exports.reversed.take(3).toList();
    return SectionCard(
      key: const Key('cut-card'),
      title: s.edCardTitle,
      subtitle: edited == null ? null : s.edEditedOn(edited.replaceFirst('T', ' ').split('.').first),
      icon: Icons.movie_edit,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            s.edCardBody,
            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 12,
            runSpacing: 8,
            children: [
              FilledButton.icon(
                key: const Key('open-editor'),
                onPressed: () => unawaited(_openCutEditor()),
                icon: const Icon(Icons.content_cut_rounded, size: 18),
                label: Text(s.edEditInApp),
              ),
              OutlinedButton.icon(
                key: const Key('export-cut'),
                onPressed: () => unawaited(_exportCut()),
                icon: const Icon(Icons.ios_share_rounded, size: 18),
                label: Text(s.edExportProject),
              ),
            ],
          ),
          if (exports.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text(s.edExportsTitle, style: theme.textTheme.labelLarge),
            for (final e in exports) CopyablePath(label: s.edExportKind(e.kind), path: e.path),
          ],
        ],
      ),
    );
  }

  Widget _lastRunCard(ThemeData theme, S s, LastRun last) {
    return SectionCard(
      title: s.lastRun,
      subtitle: last.at.replaceFirst('T', ' ').split('.').first,
      icon: Icons.history_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Wrap(
            spacing: 40,
            runSpacing: 16,
            children: [
              Stat(label: s.durationStat, value: formatDuration(last.durationUs)),
              Stat(label: s.clipsStat, value: '${last.clipsUsed}'),
            ],
          ),
          const SizedBox(height: 12),
          CopyablePath(label: s.draftLabel, path: last.draftPath),
          for (final draft in last.editorDrafts)
            CopyablePath(label: s.editorDraftLabel, path: draft),
        ],
      ),
    );
  }
}

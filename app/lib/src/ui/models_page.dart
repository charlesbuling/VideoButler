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

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../engine/platform_models.dart';
import '../l10n/strings.dart';
import '../models.dart';
import '../theme.dart';
import 'file_browser.dart';
import 'widgets.dart';

/// The model library: downloaded once, used by every project after.
///
/// Models live in the app's library folder rather than in a project, so a
/// speech model fetched for one holiday is there for the next, and deleting
/// a project never deletes one.
class ModelsPage extends StatefulWidget {
  const ModelsPage({super.key, required this.engine, this.embedded = false});

  final Engine engine;

  /// Shown inside the app shell rather than pushed as its own screen.
  final bool embedded;

  @override
  State<ModelsPage> createState() => _ModelsPageState();
}

class _ModelsPageState extends State<ModelsPage> {
  ModelLibrary? _library;
  String? _error;

  /// A model pack being imported: what it is doing, and how far through.
  String? _importing;
  double? _importFraction;
  String? _importJob;

  /// Downloads in flight, by model id.
  final Map<String, String> _jobs = {};
  final Map<String, JobStatus> _progress = {};
  Timer? _poll;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final library = await widget.engine.models();
      if (!mounted) return;
      setState(() {
        _library = library;
        _error = null;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _download(ModelInfo model) async {
    try {
      final job = await widget.engine.downloadModel(model.id);
      if (job == null) {
        await _load();
        return;
      }
      setState(() => _jobs[model.id] = job);
      _poll ??= Timer.periodic(
        const Duration(milliseconds: 500),
        (_) => unawaited(_check()),
      );
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _check() async {
    var anyFinished = false;
    for (final entry in Map.of(_jobs).entries) {
      try {
        final status = await widget.engine.jobStatus(entry.value);
        if (!mounted) return;
        setState(() => _progress[entry.key] = status);
        if (status.finished) {
          anyFinished = true;
          _jobs.remove(entry.key);
          if (status.state == 'failed') {
            _toast(S.of(context).downloadFailed(status.error ?? ''));
          }
        }
      } on EngineException {
        _jobs.remove(entry.key);
      }
    }
    if (_jobs.isEmpty) {
      _poll?.cancel();
      _poll = null;
    }
    if (anyFinished) await _load();
  }

  /// Bring models in from a folder: a model pack for a device with no
  /// network. Android picks the folder with the system picker and copies in
  /// the host; everywhere else the engine copies, checking each file.
  Future<void> _import() async {
    final s = S.of(context);
    final library = _library;
    if (library == null) return;
    if (PlatformModels.handledByHost) {
      setState(() {
        _importing = s.importPicking;
        _importFraction = null;
      });
      try {
        // Caption models are gigabytes and a pack carries three sizes: only
        // the largest this phone can run is taken, whichever the folder has,
        // and none smaller than one it has already. What is installed --
        // from the installer's OBB folder too -- is not copied again, and
        // what this device cannot run is not copied at all, but said.
        final fitting = library.models.where((m) => m.role == 'describe' && m.fits).toList()
          ..sort((a, b) => b.memoryMb.compareTo(a.memoryMb));
        final captions = fitting.takeWhile((m) => !m.installed).toList();
        final tooBig = library.models.where((m) => !m.fits && !m.installed).toList();
        final result = await PlatformModels.importFolder(
          [
            for (final m in library.models)
              if (m.role != 'describe' && !m.installed && m.fits) ...m.files,
          ],
          prefer: [for (final m in captions) m.files],
          skip: [
            for (final m in tooBig)
              for (final f in m.files) f.name,
          ],
          onProgress: (name, done, total) {
            if (!mounted) return;
            setState(() {
              _importing = name;
              _importFraction = done / total;
            });
          },
        );
        if (result != null) {
          final left = {...(result['tooBig'] as List? ?? const []).cast<String>()};
          _reportImport(
            result,
            tooBig: [
              for (final m in tooBig)
                if (m.files.any((f) => left.contains(f.name))) m.name,
            ],
          );
        }
      } on Exception catch (e) {
        _toast(e.toString());
      }
      if (mounted) setState(() => _importing = null);
      await _load();
      return;
    }
    final chosen = await FileBrowserSheet.show(context, engine: widget.engine);
    if (chosen == null || chosen.isEmpty || !mounted) return;
    try {
      final job = await widget.engine.importModels(chosen.first);
      setState(() {
        _importJob = job;
        _importing = s.importPicking;
        _importFraction = 0;
      });
      while (mounted) {
        await Future<void>.delayed(const Duration(milliseconds: 400));
        final status = await widget.engine.jobStatus(job);
        if (!mounted) return;
        setState(() {
          _importing = status.detail;
          _importFraction = status.percent / 100;
        });
        if (status.finished) {
          if (status.state == 'failed') {
            _toast(status.error ?? '');
          } else {
            _reportImport(status.result ?? const {});
          }
          break;
        }
      }
    } on EngineException catch (e) {
      _toast(e.message);
    }
    if (mounted) {
      setState(() {
        _importing = null;
        _importJob = null;
      });
    }
    await _load();
  }

  /// Say what an import did. `tooBig` names models the folder had that this
  /// device cannot run: left out, and said, so that a folder of only those
  /// does not read as a folder of nothing.
  void _reportImport(Map<String, dynamic> result, {List<String> tooBig = const []}) {
    final s = S.of(context);
    final imported = (result['imported'] as List?)?.length ?? 0;
    final already = (result['alreadyInstalled'] as List?)?.length ?? 0;
    final failed = [
      for (final f in result['failed'] as List? ?? const [])
        f is Map ? '${f['file']}' : '$f',
    ];
    final lines = [
      if (failed.isNotEmpty)
        s.importFailed(failed.join('、'))
      else if (imported + already > 0)
        s.importDone(imported, already),
      if (tooBig.isNotEmpty) s.importTooBig(tooBig.join('、')),
    ];
    _toast(lines.isEmpty ? s.importNothing : lines.join('\n'));
  }

  /// Run this one for its role from now on: every size came with the
  /// installer, and the largest is not always the one to test.
  Future<void> _use(ModelInfo model) async {
    try {
      final library = await widget.engine.useModel(model.id);
      if (!mounted) return;
      setState(() => _library = library);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _cancel(String modelId) async {
    final job = _jobs[modelId];
    if (job == null) return;
    try {
      await widget.engine.cancelJob(job);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _delete(ModelInfo model) async {
    final s = S.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.deleteModelTitle(model.name)),
        content: Text(s.deleteModelBody(formatBytes(model.bytes))),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(s.keepIt),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.delete),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await widget.engine.deleteModel(model.id);
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final body = Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
          children: [
            if (_error != null)
              NoticeBanner(
                title: s.modelsUnreadable,
                lines: [_error!],
                tone: Tone.bad,
              ),
            if (_library == null && _error == null)
              const Padding(
                padding: EdgeInsets.all(40),
                child: Center(child: CircularProgressIndicator()),
              ),
            if (_library != null) ..._content(context, _library!),
          ],
        ),
      ),
    );
    if (widget.embedded) return body;
    return Scaffold(appBar: AppBar(title: Text(s.models)), body: body);
  }

  /// The director's models: depth finds what blocks the view, faces and
  /// people tell a subject from an obstruction.
  static const _framing = {'depth', 'faces', 'people', 'composition'};

  /// Captions from the picture, and the body poses a dance cut is made from.
  static const _captions = {'describe', 'pose'};

  /// Scene labels, which only a platform without Apple Vision uses: on a
  /// Mac or an iPhone the model would download and never run. The web build
  /// talks to an engine on the same machine, so the browser's platform is
  /// the engine's.
  static bool get _needsLabelModel =>
      defaultTargetPlatform != TargetPlatform.macOS &&
      defaultTargetPlatform != TargetPlatform.iOS;

  List<Widget> _content(BuildContext context, ModelLibrary library) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return [
      NoticeBanner(
        title: s.downloadedOnce,
        lines: [
          s.modelsKeptIn(library.directory),
          PlatformModels.handledByHost
              ? s.visionOnPhone
              : s.visionNeedsNoDownload,
        ],
        tone: Tone.good,
        icon: Icons.download_done_rounded,
      ),
      const SizedBox(height: 16),
      SectionCard(
        title: s.importModels,
        subtitle: s.importModelsNote,
        icon: Icons.drive_folder_upload_outlined,
        trailing: FilledButton.tonalIcon(
          key: const Key('import-models'),
          onPressed: _importing != null ? null : () => unawaited(_import()),
          icon: const Icon(Icons.folder_open_rounded, size: 18),
          label: Text(s.chooseFolder),
        ),
        child: _importing == null
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  LinearProgressIndicator(value: _importFraction),
                  const SizedBox(height: 6),
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          _importing!,
                          style: monoStyle(context, size: 11),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      if (_importJob != null)
                        TextButton(
                          onPressed: () => unawaited(widget.engine.cancelJob(_importJob!)),
                          child: Text(s.cancel),
                        ),
                    ],
                  ),
                ],
              ),
      ),
      const SizedBox(height: 16),
      SectionCard(
        title: s.roleSpeech,
        subtitle: s.speechSubtitle,
        icon: Icons.record_voice_over_outlined,
        child: Column(
          children: [
            for (final model in library.models.where((m) => m.role == 'speech'))
              _modelRow(theme, model, several: _several(library, model.role)),
          ],
        ),
      ),
      const SizedBox(height: 16),
      SectionCard(
        title: s.roleFraming,
        subtitle: s.framingSubtitle,
        icon: Icons.crop_free_rounded,
        child: Column(
          children: [
            for (final model in library.models.where((m) => _framing.contains(m.role)))
              _modelRow(theme, model, several: _several(library, model.role)),
          ],
        ),
      ),
      const SizedBox(height: 16),
      SectionCard(
        title: s.captionsAndDance,
        subtitle: s.captionsAndDanceSubtitle,
        icon: Icons.subtitles_outlined,
        child: Column(
          children: [
            for (final model in library.models.where((m) => _captions.contains(m.role)))
              _modelRow(theme, model, several: _several(library, model.role)),
          ],
        ),
      ),
      if (_needsLabelModel && library.models.any((m) => m.role == 'labels')) ...[
        const SizedBox(height: 16),
        SectionCard(
          title: s.roleLabels,
          subtitle: s.labelsSubtitle,
          icon: Icons.label_outline_rounded,
          child: Column(
            children: [
              for (final model in library.models.where((m) => m.role == 'labels'))
                _modelRow(theme, model, several: _several(library, model.role)),
            ],
          ),
        ),
      ],
    ];
  }

  /// Whether a role has more than one model installed, and so a choice of
  /// which runs.
  static bool _several(ModelLibrary library, String role) =>
      library.models.where((m) => m.role == role && m.installed).length > 1;

  Widget _modelRow(ThemeData theme, ModelInfo model, {required bool several}) {
    final s = S.of(context);
    final downloading = _jobs.containsKey(model.id);
    final progress = _progress[model.id];
    final choosable = several && model.installed && model.fits && !model.inUse;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    // Wrapped, not a row: on a phone the name and its chips
                    // are wider than the space beside the button, and a row
                    // ran them under it.
                    Wrap(
                      spacing: 6,
                      runSpacing: 4,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Padding(
                          padding: const EdgeInsets.only(right: 4),
                          child: Text(
                            model.name,
                            style: theme.textTheme.bodyMedium
                                ?.copyWith(fontWeight: FontWeight.w600),
                          ),
                        ),
                        StatusChip(label: formatBytes(model.bytes)),
                        StatusChip(label: model.license),
                        if (model.installed)
                          StatusChip(
                            label: s.installed,
                            tone: Tone.good,
                            icon: Icons.check_rounded,
                          ),
                        if (several && model.inUse)
                          StatusChip(
                            key: Key('in-use-${model.id}'),
                            label: s.inUse,
                            tone: Tone.good,
                            icon: Icons.play_arrow_rounded,
                          ),
                        if (model.bundled) StatusChip(label: s.bundledModel),
                        if (!model.fits) StatusChip(label: s.tooBigForDevice, tone: Tone.warning),
                      ],
                    ),
                    const SizedBox(height: 3),
                    Text(
                      model.note,
                      style: theme.textTheme.bodySmall
                          ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                    ),
                  ],
                ),
              ),
              if (choosable)
                TextButton(
                  key: Key('use-${model.id}'),
                  onPressed: () => unawaited(_use(model)),
                  child: Text(s.useModel),
                ),
              // What came with the app stays with it.
              if (model.installed)
                model.bundled
                    ? const SizedBox.shrink()
                    : IconButton(
                        tooltip: s.delete,
                        icon: const Icon(Icons.delete_outline_rounded),
                        onPressed: () => unawaited(_delete(model)),
                      )
              else if (downloading)
                TextButton(
                  onPressed: () => unawaited(_cancel(model.id)),
                  child: Text(s.cancel),
                )
              else if (!model.downloadable)
                StatusChip(label: s.notDownloadable)
              else
                FilledButton.tonalIcon(
                  key: Key('download-${model.id}'),
                  onPressed: () => unawaited(_download(model)),
                  icon: const Icon(Icons.download_rounded, size: 18),
                  label: Text(s.download),
                ),
            ],
          ),
          if (downloading && progress != null) ...[
            const SizedBox(height: 8),
            LinearProgressIndicator(value: progress.percent / 100),
            const SizedBox(height: 4),
            Text(progress.detail, style: monoStyle(context, size: 11)),
          ],
        ],
      ),
    );
  }
}

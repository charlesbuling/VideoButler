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

import '../engine/engine.dart';
import '../l10n/strings.dart';
import '../models.dart';
import '../theme.dart';
import 'file_browser.dart';
import 'result_view.dart';
import 'widgets.dart';

/// One run with no project behind it: pick footage, pick a template, get a
/// draft. Kept beside projects for trying a clip out without saving anything.
class HomePage extends StatefulWidget {
  const HomePage({super.key, required this.engine, this.embedded = false});

  final Engine engine;

  /// Shown inside the app shell, which supplies the app bar.
  final bool embedded;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  EngineVersion? _version;
  DoctorReport? _doctor;
  List<EditTemplate> _templates = const [];

  /// The project's videos, as the user chose them: files, folders, or both.
  /// The engine expands a folder, so this stays what was picked.
  List<String> _inputs = const [];
  String? _templateId;

  bool _loading = true;
  bool _running = false;
  String? _startupError;
  String? _runError;
  RunResult? _result;

  @override
  void initState() {
    super.initState();
    // Fire and forget: the page shows a spinner until this lands.
    unawaited(_loadEngineState());
  }

  Future<void> _loadEngineState() async {
    setState(() {
      _loading = true;
      _startupError = null;
    });
    try {
      // Explicitly typed rather than inferred: these three futures have
      // different types, and Future.wait is also what keeps a failure in one
      // from surfacing as an unhandled error from the other two.
      final results = await Future.wait<Object>([
        widget.engine.version(),
        widget.engine.doctor(),
        widget.engine.templates(),
      ]);
      if (!mounted) return;
      setState(() {
        _version = results[0] as EngineVersion;
        _doctor = results[1] as DoctorReport;
        _templates = results[2] as List<EditTemplate>;
        _loading = false;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() {
        _startupError = e.message;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _startupError = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _pickFile() async {
    final chosen = await FileBrowserSheet.show(
      context,
      engine: widget.engine,
      startDirectory:
          _inputs.isEmpty ? null : parentDirectoryOf(_inputs.first),
    );
    if (chosen != null && chosen.isNotEmpty && mounted) {
      setState(() {
        _inputs = chosen;
        _result = null;
        _runError = null;
      });
    }
  }

  Future<void> _run() async {
    if (_inputs.isEmpty) return;

    setState(() {
      _running = true;
      _runError = null;
      _result = null;
    });

    try {
      final result = await widget.engine.run(
        inputs: _inputs,
        templateId: _templateId,
      );
      if (!mounted) return;
      setState(() {
        _result = result;
        _running = false;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() {
        _runError = e.message;
        _running = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _runError = '$e';
        _running = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final body = _body(Theme.of(context));
    if (widget.embedded) return body;
    return Scaffold(
      appBar: AppBar(
        title: Text(s.appName),
        actions: [
          if (_version != null)
            Padding(
              padding: const EdgeInsets.only(right: 8),
              child: Center(
                child: StatusChip(
                  label: 'v${_version!.version}  ${widget.engine.transportLabel}',
                ),
              ),
            ),
          IconButton(
            icon: const Icon(Icons.refresh_rounded),
            tooltip: s.reloadEngine,
            onPressed: _loading ? null : () => unawaited(_loadEngineState()),
          ),
          const SizedBox(width: 4),
        ],
      ),
      body: body,
    );
  }

  Widget _body(ThemeData theme) {
    final s = S.of(context);
    if (_loading) return const Center(child: CircularProgressIndicator());
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
          children: [
            if (_startupError != null) ...[
              NoticeBanner(
                title: s.engineUnreachable,
                lines: [
                  _startupError!,
                  s.serveHint,
                ],
                tone: Tone.bad,
                icon: Icons.cloud_off_rounded,
              ),
              const SizedBox(height: 16),
            ],
            _engineCard(theme),
            const SizedBox(height: 16),
            _sourceCard(theme),
            const SizedBox(height: 16),
            _templateCard(theme),
            const SizedBox(height: 20),
            _runButton(theme),
            if (_runError != null) ...[
              const SizedBox(height: 16),
              NoticeBanner(
                title: s.theRunFailed,
                lines: [_runError!],
                tone: Tone.bad,
                icon: Icons.error_outline_rounded,
              ),
            ],
            if (_result != null) ...[
              const SizedBox(height: 28),
              ResultView(result: _result!),
            ],
          ],
        ),
      ),
    );
  }

  Widget _engineCard(ThemeData theme) {
    final doctor = _doctor;
    if (doctor == null) return const SizedBox.shrink();
    final s = S.of(context);

    return SectionCard(
      title: s.engine,
      subtitle: doctor.mediaVersion == null
          ? doctor.mediaBackend
          : '${doctor.mediaBackend} · ffmpeg ${doctor.mediaVersion}',
      icon: Icons.memory_rounded,
      trailing: StatusChip(
        label: doctor.canAnalyse ? s.ready : s.cannotAnalyse,
        tone: doctor.canAnalyse ? Tone.good : Tone.bad,
        icon: doctor.canAnalyse
            ? Icons.check_rounded
            : Icons.priority_high_rounded,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (!doctor.mediaAvailable) ...[
            NoticeBanner(
              title: s.analysisUnavailable,
              lines: [doctor.mediaNote],
              tone: Tone.bad,
              icon: Icons.videocam_off_outlined,
            ),
            const SizedBox(height: 16),
          ],
          Text(
            s.perceptionAdapters,
            style: theme.textTheme.labelMedium
                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
          const SizedBox(height: 10),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final adapter in doctor.adapters)
                StatusChip(
                  label: s.adapterChip(adapter.role, adapter.name),
                  tone: adapter.mock ? Tone.warning : Tone.good,
                  icon: adapter.mock
                      ? Icons.science_outlined
                      : Icons.check_rounded,
                ),
            ],
          ),
          if (doctor.mockedAdapters.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text(
              s.mockNote,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ],
        ],
      ),
    );
  }

  Widget _sourceCard(ThemeData theme) {
    final s = S.of(context);
    // Long lists are summarised rather than scrolled: a folder of forty clips
    // would otherwise push every other control off the screen.
    const shown = 4;
    return SectionCard(
      title: s.sourceVideos(_inputs.length),
      icon: Icons.movie_outlined,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: _inputs.isEmpty
                ? Text(
                    s.noVideosYet,
                    style: theme.textTheme.bodyMedium
                        ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      for (final path in _inputs.take(shown))
                        Padding(
                          padding: const EdgeInsets.only(bottom: 2),
                          child: SelectableText(
                            path,
                            style: monoStyle(context, size: 12),
                          ),
                        ),
                      if (_inputs.length > shown)
                        Text(
                          s.andMore(_inputs.length - shown),
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                    ],
                  ),
          ),
          const SizedBox(width: 16),
          OutlinedButton.icon(
            onPressed: _running ? null : () => unawaited(_pickFile()),
            icon: const Icon(Icons.folder_open_rounded, size: 18),
            label: Text(_inputs.isEmpty ? s.choose : s.change),
          ),
        ],
      ),
    );
  }

  Widget _templateCard(ThemeData theme) {
    final s = S.of(context);
    EditTemplate? selected;
    for (final template in _templates) {
      if (template.id == _templateId) {
        selected = template;
        break;
      }
    }

    return SectionCard(
      title: s.editingStyle,
      subtitle: _templateId == null ? s.chosenFromFootage : null,
      icon: Icons.tune_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          DropdownButtonFormField<String?>(
            initialValue: _templateId,
            decoration: InputDecoration(labelText: s.template),
            items: [
              DropdownMenuItem<String?>(
                value: null,
                child: Text(s.automatic),
              ),
              for (final template in _templates)
                DropdownMenuItem<String?>(
                  value: template.id,
                  child: Text(s.styleName(template.id, template.name)),
                ),
            ],
            onChanged: _running
                ? null
                : (value) => setState(() => _templateId = value),
          ),
          if (selected != null) ...[
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                StatusChip(
                  label: s.target(formatDuration(selected.targetDurationUs)),
                ),
                StatusChip(
                  label: s.clipsRange(
                    formatDuration(selected.minSlotUs),
                    formatDuration(selected.maxSlotUs),
                  ),
                ),
                StatusChip(label: selected.transition),
                if (selected.subtitles) StatusChip(label: s.subtitles),
                for (final tag in selected.tags) StatusChip(label: tag),
              ],
            ),
          ],
        ],
      ),
    );
  }

  Widget _runButton(ThemeData theme) {
    final s = S.of(context);
    final ready = _inputs.isNotEmpty && (_doctor?.canAnalyse ?? false);
    return SizedBox(
      height: 56,
      child: FilledButton.icon(
        onPressed: ready && !_running ? () => unawaited(_run()) : null,
        icon: _running
            ? const SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2.4),
              )
            : const Icon(Icons.auto_fix_high_rounded),
        label: Text(
          _running ? s.analysing : s.createADraft,
          style: theme.textTheme.titleMedium
              ?.copyWith(color: theme.colorScheme.onPrimary),
        ),
      ),
    );
  }
}

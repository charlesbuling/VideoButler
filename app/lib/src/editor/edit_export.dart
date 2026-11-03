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

/// Sending a cut out of the editor: as a finished video the engine renders,
/// or as a project file another editor opens -- 剪映, CapCut, OTIO -- which
/// keeps every cut a decision that can still be changed there.
library;

import 'dart:async';

import 'package:flutter/material.dart';

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import '../models.dart';
import '../ui/widgets.dart';
import 'edit_state.dart';

/// What a person can export.
enum ExportKind {
  /// A video the engine renders frame by frame.
  video,

  /// A video copied from the footage as filmed, nothing re-encoded.
  lossless,

  /// Each clip as a file of its own.
  clips,

  /// A project file another editor opens.
  draft,
}

/// What the person chose to write.
class ExportChoice {
  const ExportChoice.video(
      {this.shortSide = 1080, this.fps, this.codec = 'h264'})
      : kind = ExportKind.video,
        draft = null;
  const ExportChoice.lossless()
      : kind = ExportKind.lossless,
        draft = null,
        shortSide = 0,
        fps = null,
        codec = '';
  const ExportChoice.clips()
      : kind = ExportKind.clips,
        draft = null,
        shortSide = 0,
        fps = null,
        codec = '';
  const ExportChoice.draft(String this.draft)
      : kind = ExportKind.draft,
        shortSide = 0,
        fps = null,
        codec = '';

  final ExportKind kind;

  /// `jianying`, `capcut` or `otio`, for a draft.
  final String? draft;
  final int shortSide;
  final double? fps;
  final String codec;
}

/// Ask what to export. [editors] says which editors are here to take a
/// draft; [canRenderVideo], whether this device can write a video at all;
/// [lossless], whether the cut can leave without re-encoding, as the engine
/// finds while the sheet opens.
Future<ExportChoice?> chooseExport(
  BuildContext context,
  List<EditorInfo> editors, {
  bool canRenderVideo = true,
  Future<LosslessCheck>? lossless,
}) =>
    showModalBottomSheet<ExportChoice>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      constraints: const BoxConstraints(maxWidth: 640),
      builder: (_) => _ExportSheet(
        editors: editors,
        canRenderVideo: canRenderVideo,
        lossless: lossless,
      ),
    );

class _ExportSheet extends StatefulWidget {
  const _ExportSheet({
    required this.editors,
    required this.canRenderVideo,
    this.lossless,
  });
  final List<EditorInfo> editors;
  final bool canRenderVideo;
  final Future<LosslessCheck>? lossless;

  @override
  State<_ExportSheet> createState() => _ExportSheetState();
}

class _ExportSheetState extends State<_ExportSheet> {
  int _shortSide = 1080;
  double? _fps;
  String _codec = 'h264';

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    Widget row(String label, Widget choice) => Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Row(children: [
            SizedBox(
                width: 72,
                child: Text(label, style: theme.textTheme.bodyMedium)),
            Expanded(child: choice),
          ]),
        );
    EditorInfo? editor(String id) {
      for (final e in widget.editors) {
        if (e.id == id) return e;
      }
      return null;
    }

    return SafeArea(
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(20, 0, 20, 20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (widget.lossless != null) ...[
              _losslessSection(context, s, theme),
              const SizedBox(height: 24),
            ],
            Text(s.edExportVideo, style: theme.textTheme.titleMedium),
            const SizedBox(height: 8),
            if (!widget.canRenderVideo)
              Text(s.edNoVideoExport,
                  key: const Key('export-no-video'),
                  style: theme.textTheme.bodySmall)
            else ...[
              row(
                s.edResolution,
                Wrap(spacing: 8, children: [
                  for (final (label, side) in const [
                    ('720p', 720),
                    ('1080p', 1080),
                    ('4K', 2160)
                  ])
                    ChoiceChip(
                      key: Key('export-res-$side'),
                      label: Text(label),
                      selected: _shortSide == side,
                      onSelected: (_) => setState(() => _shortSide = side),
                    ),
                ]),
              ),
              row(
                s.edFrameRate,
                Wrap(spacing: 8, children: [
                  ChoiceChip(
                    label: Text(s.edFrameRateSource),
                    selected: _fps == null,
                    onSelected: (_) => setState(() => _fps = null),
                  ),
                  for (final fps in const [24.0, 25.0, 30.0, 60.0])
                    ChoiceChip(
                      label: Text(fps.round().toString()),
                      selected: _fps == fps,
                      onSelected: (_) => setState(() => _fps = fps),
                    ),
                ]),
              ),
              row(
                s.edCodec,
                Wrap(spacing: 8, children: [
                  ChoiceChip(
                    label: Text(s.edCodecH264),
                    selected: _codec == 'h264',
                    onSelected: (_) => setState(() => _codec = 'h264'),
                  ),
                  ChoiceChip(
                    label: Text(s.edCodecHevc),
                    selected: _codec == 'hevc',
                    onSelected: (_) => setState(() => _codec = 'hevc'),
                  ),
                ]),
              ),
              const SizedBox(height: 12),
              FilledButton.icon(
                key: const Key('export-video'),
                icon: const Icon(Icons.movie_creation_outlined),
                label: Text(s.edExportVideo),
                onPressed: () => Navigator.of(context).pop(
                  ExportChoice.video(
                      shortSide: _shortSide, fps: _fps, codec: _codec),
                ),
              ),
            ],
            ListTile(
              key: const Key('export-clips'),
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.view_module_outlined),
              title: Text(s.edClipsButton),
              subtitle: Text(s.edClipsNote),
              onTap: () => Navigator.of(context).pop(const ExportChoice.clips()),
            ),
            const SizedBox(height: 24),
            Text(s.edExportDrafts, style: theme.textTheme.titleMedium),
            const SizedBox(height: 4),
            Text(s.edDraftNote, style: theme.textTheme.bodySmall),
            const SizedBox(height: 8),
            for (final (id, label) in [
              ('jianying', s.edDraftJianying),
              ('capcut', s.edDraftCapcut),
            ])
              Builder(builder: (context) {
                final info = editor(id);
                final usable = info != null && info.canExport;
                return ListTile(
                  key: Key('export-$id'),
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.drive_folder_upload_outlined),
                  title: Text(label),
                  subtitle: usable
                      ? null
                      : Text(s.edEditorMissing(info?.name ?? label)),
                  enabled: usable,
                  onTap: () =>
                      Navigator.of(context).pop(ExportChoice.draft(id)),
                );
              }),
            ListTile(
              key: const Key('export-otio'),
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.schema_outlined),
              title: Text(s.edDraftOtio),
              subtitle: Text(s.edDraftOtioNote),
              onTap: () =>
                  Navigator.of(context).pop(const ExportChoice.draft('otio')),
            ),
          ],
        ),
      ),
    );
  }
}

extension on _ExportSheetState {
  /// Copying the footage as filmed: offered when the cut allows it, with
  /// what it adds; else why it needs re-encoding.
  Widget _losslessSection(BuildContext context, S s, ThemeData theme) {
    return FutureBuilder<LosslessCheck>(
      future: widget.lossless,
      builder: (context, snapshot) {
        final check = snapshot.data;
        final muted = theme.textTheme.bodySmall
            ?.copyWith(color: theme.colorScheme.onSurfaceVariant);
        final children = <Widget>[
          Text(s.edLosslessTitle, style: theme.textTheme.titleMedium),
          const SizedBox(height: 8),
        ];
        if (check == null) {
          children.add(Row(children: [
            const SizedBox(
              width: 16,
              height: 16,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                snapshot.hasError ? '${snapshot.error}' : s.edLosslessChecking,
                style: muted,
              ),
            ),
          ]));
        } else if (check.possible) {
          children.addAll([
            Text(s.edLosslessNote(check.extraUs / 1e6), style: muted),
            const SizedBox(height: 10),
            FilledButton.tonalIcon(
              key: const Key('export-lossless'),
              icon: const Icon(Icons.bolt_rounded),
              label: Text(s.edLosslessButton),
              onPressed: () =>
                  Navigator.of(context).pop(const ExportChoice.lossless()),
            ),
          ]);
        } else {
          children.addAll([
            Text(s.edLosslessBlocked,
                key: const Key('export-lossless-blocked'), style: muted),
            for (final reason in check.reasons)
              Padding(
                padding: const EdgeInsets.only(left: 8, top: 4),
                child: Text('· $reason', style: muted),
              ),
          ]);
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: children,
        );
      },
    );
  }
}

/// Run an export the person chose, with progress for a video, and say
/// where it went. Returns what was written, or null.
Future<ExportDone?> runExport(
  BuildContext context,
  Engine engine,
  String projectId,
  ExportChoice choice,
) async {
  final s = S.of(context);
  final messenger = ScaffoldMessenger.of(context);
  if (choice.kind == ExportKind.draft) {
    try {
      final done = await engine.editExportDraft(projectId, choice.draft!);
      if (context.mounted) await _showDone(context, engine, done);
      return done;
    } on EngineException catch (e) {
      messenger
          .showSnackBar(SnackBar(content: Text(s.edExportFailed(e.message))));
      return null;
    }
  }
  final String jobId;
  try {
    jobId = switch (choice.kind) {
      ExportKind.lossless => await engine.editExportLossless(projectId),
      ExportKind.clips => await engine.editExportClips(projectId),
      _ => await engine.editExportVideo(
          projectId,
          shortSide: choice.shortSide,
          fps: choice.fps,
          codec: choice.codec,
        ),
    };
  } on EngineException catch (e) {
    messenger
        .showSnackBar(SnackBar(content: Text(s.edExportFailed(e.message))));
    return null;
  }
  if (!context.mounted) return null;
  final status = await showDialog<JobStatus>(
    context: context,
    barrierDismissible: false,
    builder: (_) => _RenderProgress(engine: engine, jobId: jobId),
  );
  if (status == null || !context.mounted) return null;
  if (status.state == 'done' && status.result != null) {
    final done = ExportDone.fromJson(status.result!);
    await _showDone(context, engine, done);
    return done;
  }
  if (status.state == 'failed') {
    messenger.showSnackBar(
        SnackBar(content: Text(s.edExportFailed(status.error ?? ''))));
  }
  return null;
}

Future<void> _showDone(BuildContext context, Engine engine, ExportDone done) {
  final s = S.of(context);
  return showDialog<void>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(s.edExported),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          CopyablePath(label: s.edExportedTo, path: done.path),
          if (done.copied != null)
            Text(s.edClipsDone(done.copied!, done.rendered ?? 0),
                style: Theme.of(context).textTheme.bodySmall),
          if (done.bytes != null)
            Text(formatBytes(done.bytes!),
                style: Theme.of(context).textTheme.bodySmall),
          for (final w in done.warnings)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Text(w, style: Theme.of(context).textTheme.bodySmall),
            ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () =>
              unawaited(engine.reveal(done.path).catchError((_) {})),
          child: Text(s.edShowInFolder),
        ),
        FilledButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(s.edDone)),
      ],
    ),
  );
}

class _RenderProgress extends StatefulWidget {
  const _RenderProgress({required this.engine, required this.jobId});
  final Engine engine;
  final String jobId;

  @override
  State<_RenderProgress> createState() => _RenderProgressState();
}

class _RenderProgressState extends State<_RenderProgress> {
  JobStatus? _status;
  Timer? _poll;
  bool _cancelling = false;

  @override
  void initState() {
    super.initState();
    _poll = Timer.periodic(
        const Duration(milliseconds: 400), (_) => unawaited(_check()));
    unawaited(_check());
  }

  Future<void> _check() async {
    try {
      final status = await widget.engine.jobStatus(widget.jobId);
      if (!mounted) return;
      setState(() => _status = status);
      if (status.finished) {
        _poll?.cancel();
        Navigator.of(context).pop(status);
      }
    } on EngineException {
      // A poll that fails is tried again; the job carries on regardless.
    }
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final status = _status;
    final percent = status?.percent ?? 0;
    return AlertDialog(
      title: Text(s.edExporting),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LinearProgressIndicator(value: percent <= 0 ? null : percent / 100),
          const SizedBox(height: 10),
          Text(
            status == null || status.sourceCount == 0
                ? '${percent.toStringAsFixed(0)}%'
                : '${percent.toStringAsFixed(0)}% · ${s.edFrames(status.sourceIndex, status.sourceCount)}',
          ),
          if (status?.thermal != null) ...[
            const SizedBox(height: 6),
            Text(s.edThermal, style: Theme.of(context).textTheme.bodySmall),
          ],
        ],
      ),
      actions: [
        TextButton(
          key: const Key('export-cancel'),
          onPressed: _cancelling
              ? null
              : () async {
                  setState(() => _cancelling = true);
                  await widget.engine
                      .cancelJob(widget.jobId)
                      .catchError((_) {});
                },
          child: Text(s.cancel),
        ),
      ],
    );
  }
}

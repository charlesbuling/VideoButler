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

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import 'memory_map.dart';
import 'project_page.dart';
import 'thumbnail.dart';
import 'video_grid_page.dart';
import 'widgets.dart';

/// One memory: when and where, who, its videos, and the project the butler
/// made of it -- cut in the background, opened from here, cut again on
/// request. A year can be ordered by time or by kind.
class MemoryPage extends StatefulWidget {
  const MemoryPage({
    super.key,
    required this.engine,
    required this.memoryId,
    required this.title,
  });

  final Engine engine;
  final String memoryId;
  final String title;

  @override
  State<MemoryPage> createState() => _MemoryPageState();
}

class _MemoryPageState extends State<MemoryPage> {
  Memory? _memory;
  String? _error;
  final Set<String> _chosen = {};
  bool _working = false;

  int get _chosenPhotos => [
        for (final v in _memory?.videos ?? const <CatalogVideo>[])
          if (v.isPhoto && _chosen.contains(v.path)) v,
      ].length;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final memory = await widget.engine.memory(widget.memoryId);
      if (!mounted) return;
      setState(() {
        _memory = memory;
        _error = null;
      });
    } on EngineException catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  Future<void> _generate({String? order}) async {
    final s = S.of(context);
    setState(() => _working = true);
    try {
      if (order != null) {
        await widget.engine.reorderMemory(widget.memoryId, order);
      } else {
        await widget.engine.generateMemory(widget.memoryId);
      }
      _toast(s.memoryGenerating);
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    } finally {
      if (mounted) setState(() => _working = false);
    }
  }

  Future<void> _openProject(String id) async {
    try {
      final projects = await widget.engine.projects();
      final project = projects.where((p) => p.id == id).firstOrNull;
      if (project == null || !mounted) return;
      await Navigator.of(context).push(
        MaterialPageRoute<void>(builder: (_) => ProjectPage(engine: widget.engine, project: project)),
      );
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _newProject() async {
    final memory = _memory;
    if (memory == null) return;
    final chosen = [
      for (final v in memory.videos)
        if (_chosen.contains(v.path)) v.path,
    ];
    try {
      final project = await widget.engine.projectOf(memory.title, chosen);
      if (!mounted) return;
      await Navigator.of(context).push(
        MaterialPageRoute<void>(builder: (_) => ProjectPage(engine: widget.engine, project: project)),
      );
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    final memory = _memory;
    return Scaffold(
      appBar: AppBar(title: Text(memory?.title ?? widget.title)),
      floatingActionButtonLocation: FloatingActionButtonLocation.centerFloat,
      floatingActionButton: _chosen.isEmpty
          ? null
          : FloatingActionButton.extended(
              key: const Key('memory-new-project'),
              onPressed: () => unawaited(_newProject()),
              icon: const Icon(Icons.movie_edit),
              label: Text(s.newProjectFrom(
                _chosen.length - _chosenPhotos,
                _chosenPhotos,
              )),
            ),
      body: _error != null
          ? Center(child: Text(_error!))
          : memory == null
              ? const Center(child: CircularProgressIndicator())
              : Align(
                  alignment: Alignment.topCenter,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 980),
                    child: ListView(
                      padding: const EdgeInsets.fromLTRB(20, 20, 20, 20),
                      children: [
                        SectionCard(
                          key: const Key('memory-summary'),
                          title: s.memoryKindLabel(kindId(memory.kind)),
                          subtitle: [
                            s.memoryDates(memory.start, memory.end),
                            if (memory.place != null) memory.place!,
                            s.footageCount(memory.videoCount, memory.photoCount),
                          ].join(' · '),
                          icon: kindIcon(memory.kind),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              if (memory.people.isNotEmpty) ...[
                                Wrap(
                                  spacing: 10,
                                  children: [
                                    for (final p in memory.people)
                                      Column(children: [
                                        ClipOval(
                                          child: SizedBox(
                                            width: 56,
                                            height: 56,
                                            child: ColoredBox(
                                              color: theme.colorScheme.surfaceContainerHighest,
                                              child: localImage(
                                                p.thumbnail,
                                                fallback: const Icon(Icons.person_rounded),
                                              ),
                                            ),
                                          ),
                                        ),
                                        const SizedBox(height: 4),
                                        Text(p.label ?? s.unnamed, style: theme.textTheme.labelSmall),
                                      ]),
                                  ],
                                ),
                                const SizedBox(height: 14),
                              ],
                              Row(
                                children: [
                                  Icon(
                                    memory.projectRun
                                        ? Icons.check_circle_rounded
                                        : memory.status == 'failed'
                                            ? Icons.error_outline_rounded
                                            : Icons.hourglass_top_rounded,
                                    size: 18,
                                    color: memory.status == 'failed'
                                        ? theme.colorScheme.error
                                        : theme.colorScheme.primary,
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    child: Text(
                                      s.memoryStatus(memory.status, memory.projectRun),
                                      key: const Key('memory-status'),
                                    ),
                                  ),
                                ],
                              ),
                              if (memory.error != null) ...[
                                const SizedBox(height: 6),
                                Text(
                                  memory.error!,
                                  style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
                                ),
                              ],
                              const SizedBox(height: 12),
                              Wrap(
                                spacing: 10,
                                runSpacing: 10,
                                children: [
                                  if (memory.projectId != null && memory.status != 'dismissed')
                                    FilledButton.icon(
                                      key: const Key('memory-open-project'),
                                      onPressed: () => unawaited(_openProject(memory.projectId!)),
                                      icon: const Icon(Icons.open_in_new_rounded, size: 18),
                                      label: Text(s.openProject),
                                    ),
                                  FilledButton.tonalIcon(
                                    key: const Key('memory-generate'),
                                    onPressed: _working ? null : () => unawaited(_generate()),
                                    icon: const Icon(Icons.auto_awesome_rounded, size: 18),
                                    label: Text(memory.projectRun ? s.regenerate : s.generateProject),
                                  ),
                                ],
                              ),
                              if (memory.kind == MemoryKind.annual) ...[
                                const SizedBox(height: 16),
                                Text(s.orderLabel, style: theme.textTheme.titleSmall),
                                const SizedBox(height: 8),
                                SegmentedButton<String>(
                                  key: const Key('memory-order'),
                                  segments: [
                                    ButtonSegment(value: 'category', label: Text(s.orderCategory)),
                                    ButtonSegment(value: 'time', label: Text(s.orderTime)),
                                  ],
                                  selected: {memory.order ?? 'category'},
                                  onSelectionChanged: _working
                                      ? null
                                      : (v) => unawaited(_generate(order: v.first)),
                                ),
                              ],
                              const SizedBox(height: 12),
                              Text(
                                s.memoryProjectNote,
                                style: theme.textTheme.bodySmall
                                    ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                              ),
                            ],
                          ),
                        ),
                        const SizedBox(height: 8),
                        VideoGrid(
                          engine: widget.engine,
                          videos: memory.videos,
                          chosen: _chosen,
                          onChanged: () => setState(() {}),
                          shrinkWrap: true,
                        ),
                      ],
                    ),
                  ),
                ),
    );
  }
}

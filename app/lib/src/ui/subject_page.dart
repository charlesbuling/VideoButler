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
import '../models.dart';
import '../theme.dart';
import 'project_page.dart';
import 'thumbnail.dart';
import 'video_grid_page.dart';
import 'widgets.dart';

/// One person from the album, and every project and clip they turn up in.
///
/// A group of faces that look alike, not a recognised identity -- the page
/// repeats the engine's own words on that. From here a project opens
/// directly, and a clip can be shown in Finder.
class SubjectPage extends StatefulWidget {
  const SubjectPage({super.key, required this.engine, required this.subject});

  final Engine engine;
  final Subject subject;

  @override
  State<SubjectPage> createState() => _SubjectPageState();
}

class _SubjectPageState extends State<SubjectPage> {
  late Subject _subject;
  String _grouping = '';
  Map<String, Project> _projects = const {};

  @override
  void initState() {
    super.initState();
    _subject = widget.subject;
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final album = await widget.engine.album();
      final projects = await widget.engine.projects();
      if (!mounted) return;
      setState(() {
        _grouping = album.grouping;
        _projects = {for (final p in projects) p.id: p};
        for (final s in album.subjects) {
          if (s.id == _subject.id) _subject = s;
        }
      });
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

  Future<void> _rename() async {
    final s = S.of(context);
    final controller = TextEditingController(text: _subject.label ?? '');
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.whoIsThis),
        content: TextField(
          key: const Key('subject-name'),
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
            key: const Key('subject-name-save'),
            onPressed: () => Navigator.of(context).pop(controller.text),
            child: Text(s.save),
          ),
        ],
      ),
    );
    if (name == null) return;
    try {
      await widget.engine.renameSubject(_subject.id, name);
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _forget() async {
    final s = S.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.forgetSubjectTitle),
        content: Text(s.forgetSubjectBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(s.cancel),
          ),
          FilledButton(
            key: const Key('subject-forget-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.notAPerson),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await widget.engine.forgetSubject(_subject.id);
      if (mounted) Navigator.of(context).pop();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _merge() async {
    final s = S.of(context);
    final theme = Theme.of(context);
    final album = await widget.engine.album();
    if (!mounted) return;
    final others = album.subjects.where((x) => x.id != _subject.id).toList();
    final into = await showDialog<Subject>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.mergeTitle),
        content: SizedBox(
          width: 520,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(s.mergeBody, style: theme.textTheme.bodySmall),
              const SizedBox(height: 12),
              Flexible(
                child: SingleChildScrollView(
                  child: Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      for (final other in others)
                        InkWell(
                          key: Key('merge-into-${other.id}'),
                          borderRadius: BorderRadius.circular(12),
                          onTap: () => Navigator.of(context).pop(other),
                          child: SizedBox(
                            width: 84,
                            child: Column(children: [
                              ClipOval(
                                child: SizedBox(
                                  width: 64,
                                  height: 64,
                                  child: ColoredBox(
                                    color: theme.colorScheme.surfaceContainerHighest,
                                    child: localImage(
                                      other.thumbnail,
                                      fallback: const Icon(Icons.person_rounded),
                                    ),
                                  ),
                                ),
                              ),
                              Text(
                                other.label ?? s.clips(other.clips),
                                style: theme.textTheme.labelSmall,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ]),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(s.cancel)),
        ],
      ),
    );
    if (into == null) return;
    try {
      await widget.engine.mergeSubject(_subject.id, into.id);
      if (mounted) Navigator.of(context).pop();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _openHosted() async {
    final s = S.of(context);
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => VideoGridPage(
          engine: widget.engine,
          title: _subject.label ?? s.unnamed,
          load: () => widget.engine.personVideos(_subject.id),
        ),
      ),
    );
  }

  Future<void> _openProject(Project project) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => ProjectPage(engine: widget.engine, project: project),
      ),
    );
    // A run there may have changed who is in it.
    await _load();
  }

  Future<void> _reveal(String path) async {
    try {
      await widget.engine.reveal(path);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final subject = _subject;

    // Sightings by project, the project with the most shots first.
    final byProject = <String, List<Appearance>>{};
    for (final a in subject.appearances) {
      byProject.putIfAbsent(a.projectId, () => []).add(a);
    }
    int shotsOf(List<Appearance> list) => list.fold(0, (n, a) => n + a.shots);
    final projectIds = byProject.keys.toList()
      ..sort((a, b) => shotsOf(byProject[b]!).compareTo(shotsOf(byProject[a]!)));

    return Scaffold(
      appBar: AppBar(
        title: Text(subject.label ?? s.unnamed),
        actions: [
          IconButton(
            key: const Key('subject-rename'),
            tooltip: s.rename,
            icon: const Icon(Icons.edit_outlined),
            onPressed: () => unawaited(_rename()),
          ),
          IconButton(
            key: const Key('subject-merge'),
            tooltip: s.mergePerson,
            icon: const Icon(Icons.merge_type_rounded),
            onPressed: () => unawaited(_merge()),
          ),
          IconButton(
            key: const Key('subject-forget'),
            tooltip: s.notAPerson,
            icon: const Icon(Icons.person_off_outlined),
            onPressed: () => unawaited(_forget()),
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
              Row(
                children: [
                  ClipRRect(
                    borderRadius: BorderRadius.circular(72),
                    child: SizedBox(
                      width: 144,
                      height: 144,
                      child: ColoredBox(
                        color: theme.colorScheme.surfaceContainerHighest,
                        child: localImage(
                          subject.thumbnail,
                          fallback: Icon(
                            Icons.person_rounded,
                            size: 64,
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 24),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          subject.label ?? s.unnamed,
                          style: theme.textTheme.headlineSmall,
                        ),
                        const SizedBox(height: 6),
                        Text(
                          s.subjectAppearsIn(
                            projectIds.length,
                            subject.appearances.length,
                            subject.shots,
                          ),
                          key: const Key('subject-summary'),
                          style: theme.textTheme.bodyMedium,
                        ),
                        if (_grouping.isNotEmpty) ...[
                          const SizedBox(height: 10),
                          Text(
                            _grouping,
                            style: theme.textTheme.bodySmall
                                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                          ),
                        ],
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 24),
              for (final id in projectIds) ...[
                _projectCard(theme, s, id, byProject[id]!),
                const SizedBox(height: 16),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _projectCard(ThemeData theme, S s, String id, List<Appearance> sightings) {
    final project = _projects[id];
    final hosted = id == 'hosting';
    final name = hosted ? s.hostedVideos : project?.name ?? sightings.first.projectName;
    final shots = sightings.fold(0, (n, a) => n + a.shots);
    return SectionCard(
      key: Key('subject-project-$id'),
      title: name.isEmpty ? s.aQuickRun : name,
      subtitle: s.clipsAndShots(sightings.length, shots),
      icon: hosted ? Icons.auto_awesome_rounded : Icons.movie_outlined,
      trailing: hosted
          ? FilledButton.tonalIcon(
              key: const Key('subject-open-hosted'),
              onPressed: () => unawaited(_openHosted()),
              icon: const Icon(Icons.grid_view_rounded, size: 18),
              label: Text(s.seeInButler),
            )
          : project == null
              ? null
              : FilledButton.tonalIcon(
                  key: Key('subject-open-$id'),
                  onPressed: () => unawaited(_openProject(project)),
                  icon: const Icon(Icons.open_in_new_rounded, size: 18),
                  label: Text(s.openProject),
                ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final a in sightings)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 2),
              child: Row(
                children: [
                  Icon(Icons.videocam_outlined, size: 18, color: theme.colorScheme.primary),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      a.sourcePath.isEmpty ? s.clipNotRecorded : fileNameOf(a.sourcePath),
                      style: a.sourcePath.isEmpty
                          ? theme.textTheme.bodySmall
                              ?.copyWith(color: theme.colorScheme.onSurfaceVariant)
                          : monoStyle(context, size: 12),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  Text(s.shots(a.shots), style: theme.textTheme.labelSmall),
                  if (a.sourcePath.isNotEmpty)
                    IconButton(
                      tooltip: s.showInFinder,
                      icon: const Icon(Icons.folder_open_outlined, size: 18),
                      onPressed: () => unawaited(_reveal(a.sourcePath)),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

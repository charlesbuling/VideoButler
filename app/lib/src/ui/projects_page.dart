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
import 'album_page.dart';
import 'project_page.dart';
import 'subject_page.dart';
import 'widgets.dart';

/// Where the app opens after the self-test: start a project or pick one up,
/// with the people from earlier projects along the top.
class ProjectsPage extends StatefulWidget {
  const ProjectsPage({super.key, required this.engine, this.onShowAlbum});

  final Engine engine;

  /// Switch the shell to the full album.
  final VoidCallback? onShowAlbum;

  @override
  State<ProjectsPage> createState() => _ProjectsPageState();
}

class _ProjectsPageState extends State<ProjectsPage> {
  List<Project>? _projects;
  Album? _album;
  String? _error;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final results = await Future.wait<Object>([
        widget.engine.projects(),
        widget.engine.album(),
      ]);
      if (!mounted) return;
      setState(() {
        _projects = results[0] as List<Project>;
        _album = results[1] as Album;
        _error = null;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _create() async {
    final s = S.of(context);
    final controller = TextEditingController();
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.newProject),
        content: TextField(
          key: const Key('new-project-name'),
          controller: controller,
          autofocus: true,
          decoration: InputDecoration(
            labelText: s.name,
            hintText: s.projectNameHint,
          ),
          onSubmitted: (value) => Navigator.of(context).pop(value),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(s.cancel),
          ),
          FilledButton(
            key: const Key('new-project-create'),
            onPressed: () => Navigator.of(context).pop(controller.text),
            child: Text(s.create),
          ),
        ],
      ),
    );
    if (name == null || name.trim().isEmpty) return;
    try {
      final project = await widget.engine.createProject(name.trim());
      if (!mounted) return;
      await _open(project);
    } on EngineException catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    }
  }

  Future<void> _openSubject(Subject subject) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => SubjectPage(engine: widget.engine, subject: subject),
      ),
    );
    await _load();
  }

  Future<void> _open(Project project) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => ProjectPage(engine: widget.engine, project: project),
      ),
    );
    // A run adds to the album and updates the project's last run.
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final projects = _projects;
    final subjects = _album?.subjects ?? const <Subject>[];

    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 980),
        child: RefreshIndicator(
          onRefresh: _load,
          child: ListView(
            padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
            children: [
              if (_error != null)
                NoticeBanner(
                  title: s.projectsUnreadable,
                  lines: [_error!],
                  tone: Tone.bad,
                ),
              SectionCard(
                title: s.peopleInFootage,
                subtitle: subjects.isEmpty
                    ? s.peopleEmptyHint
                    : s.peopleFound(subjects.length),
                icon: Icons.people_alt_outlined,
                trailing: widget.onShowAlbum == null
                    ? null
                    : TextButton(
                        onPressed: widget.onShowAlbum,
                        child: Text(s.openAlbum),
                      ),
                child: subjects.isEmpty
                    ? Text(
                        s.nobodyYet,
                        style: theme.textTheme.bodySmall
                            ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                      )
                    : SizedBox(
                        height: 118,
                        child: ListView(
                          scrollDirection: Axis.horizontal,
                          children: [
                            for (final subject in subjects.take(20))
                              Padding(
                                padding: const EdgeInsets.only(right: 12),
                                child: SubjectTile(
                                  subject: subject,
                                  size: 92,
                                  onTap: () => unawaited(_openSubject(subject)),
                                ),
                              ),
                          ],
                        ),
                      ),
              ),
              const SizedBox(height: 20),
              Row(
                children: [
                  Text(s.projects, style: theme.textTheme.titleLarge),
                  const Spacer(),
                  FilledButton.icon(
                    key: const Key('new-project'),
                    onPressed: () => unawaited(_create()),
                    icon: const Icon(Icons.add_rounded),
                    label: Text(s.newProject),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              if (projects == null && _error == null)
                const Padding(
                  padding: EdgeInsets.all(40),
                  child: Center(child: CircularProgressIndicator()),
                )
              else if (projects != null && projects.isEmpty)
                Padding(
                  padding: const EdgeInsets.all(32),
                  child: Column(
                    children: [
                      Icon(Icons.movie_filter_outlined,
                          size: 48, color: theme.colorScheme.outline),
                      const SizedBox(height: 12),
                      Text(
                        s.noProjects,
                        textAlign: TextAlign.center,
                        style: theme.textTheme.bodyMedium
                            ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                      ),
                    ],
                  ),
                )
              else if (projects != null)
                for (final project in projects)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: _ProjectCard(
                      project: project,
                      onTap: () => unawaited(_open(project)),
                    ),
                  ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ProjectCard extends StatelessWidget {
  const _ProjectCard({required this.project, required this.onTap});

  final Project project;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final style = project.style;
    final last = project.lastRun;
    return Card(
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.all(18),
          child: Row(
            children: [
              Icon(
                project.memory == null ? Icons.movie_edit : Icons.auto_awesome_rounded,
                color: theme.colorScheme.primary,
              ),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(project.name, style: theme.textTheme.titleMedium),
                    const SizedBox(height: 6),
                    Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: [
                        if (project.memory != null)
                          StatusChip(
                            key: Key('memory-badge-${project.id}'),
                            label: s.memoryBadge,
                            icon: Icons.auto_awesome_rounded,
                          ),
                        StatusChip(
                          label: s.items(project.inputs.length),
                          icon: Icons.video_library_outlined,
                        ),
                        if (style.mood != null) StatusChip(label: s.moodLabel(style.mood!.id)),
                        if (style.topic != null)
                          StatusChip(label: s.topicLabel(style.topic!.id)),
                        if (style.targetDurationUs != null)
                          StatusChip(
                            label: s.durationChoice(style.targetDurationUs! ~/ 1000000),
                            icon: Icons.timer_outlined,
                          ),
                        if (style.beatSync ?? false)
                          StatusChip(label: s.onTheBeat, icon: Icons.graphic_eq_rounded),
                        if (style.music != null && style.music!.isNotEmpty)
                          StatusChip(label: s.music, icon: Icons.music_note_rounded),
                        if (project.model == ModelChoice.cloud)
                          StatusChip(label: s.cloud, icon: Icons.cloud_outlined),
                        for (final editor in project.editors)
                          StatusChip(
                            label: editor == 'jianying' ? '剪映' : 'CapCut',
                            icon: Icons.output_rounded,
                          ),
                      ],
                    ),
                    // Under the chips rather than beside them: beside, it
                    // took the width the name needed on a phone.
                    if (last != null) ...[
                      const SizedBox(height: 8),
                      Text(
                        '${formatDuration(last.durationUs)} · '
                        '${s.lastRunOn(last.at.split('T').first)}',
                        style: theme.textTheme.labelMedium
                            ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 8),
              const Icon(Icons.chevron_right_rounded),
            ],
          ),
        ),
      ),
    );
  }
}

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
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import 'album_page.dart' show SubjectTile;
import 'file_browser.dart';
import 'memory_map.dart';
import 'subject_page.dart';
import 'video_grid_page.dart';
import 'widgets.dart';

/// The video butler: one tap hands it the footage on this device. It files
/// every video by face and by what it shows, finds the memories in them,
/// and cuts a project for each in the background. Two tabs: the categories,
/// and the memory map.
class ButlerPage extends StatefulWidget {
  const ButlerPage({super.key, required this.engine, this.embedded = false});

  final Engine engine;
  final bool embedded;

  @override
  State<ButlerPage> createState() => _ButlerPageState();
}

class _ButlerPageState extends State<ButlerPage> {
  HostingOverview? _overview;
  String? _error;
  JobStatus? _job;
  Timer? _poll;

  /// Bumped when a pass finishes, so the map reloads what it found.
  int _generation = 0;

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
      final overview = await widget.engine.hosting();
      if (!mounted) return;
      setState(() {
        _overview = overview;
        _error = null;
      });
      if (overview.jobId != null) _follow(overview.jobId!);
    } on EngineException catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  void _follow(String jobId) {
    _poll?.cancel();
    _poll = Timer.periodic(const Duration(milliseconds: 800), (_) async {
      try {
        final status = await widget.engine.jobStatus(jobId);
        if (!mounted) return;
        setState(() => _job = status);
        if (status.finished) {
          _poll?.cancel();
          _poll = null;
          if (status.error != null && status.state == 'failed') _toast(status.error!);
          setState(() {
            _job = null;
            _generation++;
          });
          await _load();
        }
      } on EngineException {
        _poll?.cancel();
        if (mounted) setState(() => _job = null);
      }
    });
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  Future<void> _start() async {
    try {
      final id = await widget.engine.startHosting();
      _follow(id);
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _stop() async {
    final job = _job;
    try {
      if (job != null) await widget.engine.cancelJob(job.jobId);
      await widget.engine.updateHosting(enabled: false);
      await _load();
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _changeFolders() async {
    final s = S.of(context);
    final overview = _overview;
    if (overview == null) return;
    final roots = [...overview.roots];
    final changed = await showDialog<List<String>>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialog) => AlertDialog(
          title: Text(s.hostingFolders),
          content: SizedBox(
            width: 520,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final root in roots)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    leading: const Icon(Icons.folder_outlined),
                    title: Text(root, overflow: TextOverflow.ellipsis),
                    trailing: IconButton(
                      icon: const Icon(Icons.close_rounded),
                      onPressed: () => setDialog(() => roots.remove(root)),
                    ),
                  ),
                // A disk plugged in, whole, in a tap: on a Mac the picker
                // would otherwise have to climb to the system's folders.
                if (overview.volumes.any((v) => !roots.contains(v.$2))) ...[
                  const SizedBox(height: 12),
                  Text(s.disksPluggedIn, style: Theme.of(context).textTheme.labelLarge),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 8,
                    runSpacing: 6,
                    children: [
                      for (final (name, path) in overview.volumes)
                        if (!roots.contains(path))
                          ActionChip(
                            key: Key('hosting-add-disk-$name'),
                            avatar: const Icon(Icons.usb_rounded, size: 16),
                            label: Text(name),
                            tooltip: path,
                            onPressed: () => setDialog(() => roots.add(path)),
                          ),
                    ],
                  ),
                ],
                const SizedBox(height: 8),
                Align(
                  alignment: Alignment.centerLeft,
                  child: TextButton.icon(
                    key: const Key('hosting-add-folder'),
                    onPressed: () async {
                      final picked = await FileBrowserSheet.show(context, engine: widget.engine);
                      if (picked == null) return;
                      setDialog(() {
                        for (final p in picked) {
                          if (!roots.contains(p)) roots.add(p);
                        }
                      });
                    },
                    icon: const Icon(Icons.create_new_folder_outlined),
                    label: Text(s.addFolder),
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(const <String>[]),
              child: Text(s.useDefaultFolders),
            ),
            TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(s.cancel)),
            FilledButton(onPressed: () => Navigator.of(context).pop(roots), child: Text(s.save)),
          ],
        ),
      ),
    );
    if (changed == null) return;
    try {
      final overview = await widget.engine.updateHosting(roots: changed);
      if (mounted) setState(() => _overview = overview);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final overview = _overview;
    final body = DefaultTabController(
      length: 2,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
            child: _header(context, s, overview),
          ),
          TabBar(
            tabs: [
              Tab(key: const Key('tab-categories'), text: s.tabCategories),
              Tab(key: const Key('tab-memory-map'), text: s.tabMemoryMap),
            ],
          ),
          Expanded(
            child: TabBarView(
              physics: const NeverScrollableScrollPhysics(),
              children: [
                _error != null
                    ? Center(child: Text(_error!))
                    : overview == null
                        ? const Center(child: CircularProgressIndicator())
                        : _Categories(
                            engine: widget.engine,
                            overview: overview,
                            onChanged: () => unawaited(_load()),
                          ),
                MemoryMapView(key: ValueKey('map-$_generation'), engine: widget.engine),
              ],
            ),
          ),
        ],
      ),
    );
    if (widget.embedded) return body;
    return Scaffold(appBar: AppBar(title: Text(s.navAlbum)), body: body);
  }

  Widget _header(BuildContext context, S s, HostingOverview? overview) {
    final theme = Theme.of(context);
    final job = _job;
    final running = job != null && !job.finished;
    final when = overview?.scannedAt == null
        ? null
        : _when(DateTime.tryParse(overview!.scannedAt!)?.toLocal());
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 14, 18, 14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Wrap(
              spacing: 12,
              runSpacing: 10,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 560),
                  child: Text(
                    overview == null
                        ? ''
                        : overview.scannedAt == null && !running
                            ? s.hostingIntro
                            : s.hostingStatus(overview.videos, overview.photos, when, overview.memories),
                    key: const Key('hosting-status'),
                    style: theme.textTheme.bodyMedium,
                  ),
                ),
                if (!running)
                  FilledButton.icon(
                    key: const Key('hosting-start'),
                    onPressed: overview == null ? null : () => unawaited(_start()),
                    icon: Icon(overview?.enabled == true
                        ? Icons.refresh_rounded
                        : Icons.auto_awesome_rounded),
                    label: Text(overview?.enabled == true ? s.hostingAgain : s.hostingCta),
                  ),
                if (running || overview?.enabled == true)
                  OutlinedButton.icon(
                    key: const Key('hosting-stop'),
                    onPressed: () => unawaited(_stop()),
                    icon: const Icon(Icons.stop_rounded),
                    label: Text(s.hostingStop),
                  ),
                TextButton.icon(
                  key: const Key('hosting-folders'),
                  onPressed: overview == null ? null : () => unawaited(_changeFolders()),
                  icon: const Icon(Icons.folder_open_outlined, size: 18),
                  label: Text(
                    overview == null || !overview.customRoots
                        ? s.changeFolders
                        : '${s.hostingFolders} · ${overview.roots.length}',
                  ),
                ),
              ],
            ),
            if (overview != null && overview.warnings.isNotEmpty) ...[
              const SizedBox(height: 12),
              NoticeBanner(
                key: const Key('hosting-warnings'),
                title: s.foldersUnreachable,
                lines: overview.warnings,
                icon: Icons.usb_off_rounded,
              ),
            ],
            if (running) ...[
              const SizedBox(height: 12),
              LinearProgressIndicator(
                key: const Key('hosting-progress'),
                value: job.percent / 100,
                borderRadius: BorderRadius.circular(4),
              ),
              const SizedBox(height: 6),
              Text(
                '${s.hostingStep(job.step, job.file, job.sourceIndex, job.sourceCount)} · '
                '${job.percent.toStringAsFixed(0)}%',
                style: theme.textTheme.bodySmall,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ],
        ),
      ),
    );
  }

  String? _when(DateTime? t) {
    if (t == null) return null;
    String two(int v) => v.toString().padLeft(2, '0');
    return '${t.month}/${t.day} ${two(t.hour)}:${two(t.minute)}';
  }
}

/// The categories tab: faces, then the smart categories, then the ones the
/// user removed.
class _Categories extends StatefulWidget {
  const _Categories({required this.engine, required this.overview, required this.onChanged});

  final Engine engine;
  final HostingOverview overview;
  final VoidCallback onChanged;

  @override
  State<_Categories> createState() => _CategoriesState();
}

class _CategoriesState extends State<_Categories> {
  final Map<String, Uint8List> _thumbs = {};

  @override
  void initState() {
    super.initState();
    unawaited(_loadThumbs());
  }

  @override
  void didUpdateWidget(_Categories old) {
    super.didUpdateWidget(old);
    if (old.overview != widget.overview) unawaited(_loadThumbs());
  }

  Future<void> _loadThumbs() async {
    final paths = [
      for (final c in widget.overview.categories)
        for (final p in c.samples)
          if (!_thumbs.containsKey(p)) p,
    ];
    for (var i = 0; i < paths.length; i += 8) {
      try {
        final got = await widget.engine
            .thumbnails(paths.sublist(i, (i + 8).clamp(0, paths.length)), width: 200);
        if (!mounted) return;
        setState(() => _thumbs.addAll(got));
      } on EngineException {
        return;
      }
    }
  }

  Future<void> _openCategory(CategorySummary category) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => VideoGridPage(
          engine: widget.engine,
          title: category.name,
          categoryId: category.id,
          load: () => widget.engine.categoryVideos(category.id),
        ),
      ),
    );
    widget.onChanged();
  }

  Future<void> _openPerson(ButlerPerson person) async {
    final s = S.of(context);
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (context) => VideoGridPage(
          engine: widget.engine,
          title: person.label ?? s.unnamed,
          load: () => widget.engine.personVideos(person.id),
          actions: [
            IconButton(
              key: const Key('person-edit'),
              tooltip: s.rename,
              icon: const Icon(Icons.manage_accounts_outlined),
              onPressed: () async {
                final album = await widget.engine.album();
                final subject = album.subjects.where((x) => x.id == person.id).firstOrNull;
                if (subject == null || !context.mounted) return;
                await Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => SubjectPage(engine: widget.engine, subject: subject),
                  ),
                );
              },
            ),
          ],
        ),
      ),
    );
    widget.onChanged();
  }

  Future<void> _restore(String id) async {
    try {
      await widget.engine.restoreCategory(id);
      widget.onChanged();
    } on EngineException catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    final overview = widget.overview;
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1100),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 16, 16, 60),
          children: [
            if (overview.notes.isNotEmpty && overview.videos > 0)
              NoticeBanner(
                title: s.butlerNotesTitle,
                lines: overview.notes,
                icon: Icons.info_outline_rounded,
              ),
            const SizedBox(height: 16),
            SectionCard(
              key: const Key('butler-faces'),
              title: s.sectionFaces,
              subtitle: s.facesSubtitle,
              icon: Icons.face_retouching_natural_rounded,
              child: overview.people.isEmpty
                  ? Text(s.facesEmpty)
                  : SizedBox(
                      height: 150,
                      child: ListView.separated(
                        scrollDirection: Axis.horizontal,
                        itemCount: overview.people.length,
                        separatorBuilder: (_, __) => const SizedBox(width: 8),
                        itemBuilder: (context, i) {
                          final p = overview.people[i];
                          return SubjectTile(
                            key: Key('butler-person-${p.id}'),
                            subject: Subject(
                              id: p.id,
                              label: p.label,
                              kind: 'person',
                              thumbnail: p.thumbnail,
                              clips: p.videos,
                              shots: 0,
                              appearances: const [],
                            ),
                            caption: s.footageCount(p.videos, p.photos),
                            onTap: () => unawaited(_openPerson(p)),
                          );
                        },
                      ),
                    ),
            ),
            const SizedBox(height: 16),
            SectionCard(
              key: const Key('butler-smart'),
              title: s.sectionSmart,
              subtitle: s.smartSubtitle,
              icon: Icons.auto_awesome_mosaic_rounded,
              child: overview.categories.every((c) => c.count == 0)
                  ? Text(s.smartEmpty)
                  : Wrap(
                      spacing: 14,
                      runSpacing: 14,
                      children: [
                        for (final c in overview.categories)
                          _CategoryCard(
                            category: c,
                            thumbs: _thumbs,
                            onTap: () => unawaited(_openCategory(c)),
                          ),
                      ],
                    ),
            ),
            if (overview.removed.isNotEmpty) ...[
              const SizedBox(height: 16),
              Text(s.removedCategories, style: theme.textTheme.titleSmall),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  for (final (id, name) in overview.removed)
                    InputChip(
                      key: Key('removed-$id'),
                      label: Text(name),
                      avatar: const Icon(Icons.undo_rounded, size: 16),
                      tooltip: s.restore,
                      onPressed: () => unawaited(_restore(id)),
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

IconData _categoryIcon(String id) => switch (id) {
      'study' => Icons.menu_book_rounded,
      'livestream' => Icons.live_tv_rounded,
      'children' => Icons.child_friendly_rounded,
      'electronics' => Icons.devices_other_rounded,
      'transport' => Icons.directions_car_rounded,
      'food' => Icons.restaurant_rounded,
      'gathering' => Icons.celebration_rounded,
      'scenery' => Icons.landscape_rounded,
      'animals' => Icons.pets_rounded,
      _ => Icons.category_rounded,
    };

class _CategoryCard extends StatelessWidget {
  const _CategoryCard({required this.category, required this.thumbs, required this.onTap});

  final CategorySummary category;
  final Map<String, Uint8List> thumbs;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    final samples = category.samples.take(4).toList();
    return SizedBox(
      width: 200,
      child: Material(
        key: Key('category-${category.id}'),
        color: theme.colorScheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(16),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              AspectRatio(
                aspectRatio: 16 / 10,
                child: samples.isEmpty
                    ? ColoredBox(
                        color: theme.colorScheme.surfaceContainerHighest,
                        child: Icon(
                          _categoryIcon(category.id),
                          size: 36,
                          color: theme.colorScheme.outline,
                        ),
                      )
                    : GridView.count(
                        crossAxisCount: samples.length == 1 ? 1 : 2,
                        physics: const NeverScrollableScrollPhysics(),
                        padding: EdgeInsets.zero,
                        mainAxisSpacing: 1,
                        crossAxisSpacing: 1,
                        childAspectRatio: samples.length == 1 ? 16 / 10 : 16 / 10,
                        children: [
                          for (final p in samples)
                            thumbs[p] == null
                                ? ColoredBox(color: theme.colorScheme.surfaceContainerHighest)
                                : Image.memory(thumbs[p]!, fit: BoxFit.cover, gaplessPlayback: true),
                        ],
                      ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 10, 12, 12),
                child: Row(
                  children: [
                    Icon(_categoryIcon(category.id), size: 18, color: theme.colorScheme.primary),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(category.name, style: theme.textTheme.titleSmall),
                          Text(
                            category.unverified
                                ? '${s.footageCount(category.videos, category.photos)} · ${s.unverifiedBadge}'
                                : s.footageCount(category.videos, category.photos),
                            style: theme.textTheme.labelSmall
                                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

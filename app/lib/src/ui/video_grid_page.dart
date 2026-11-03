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
import 'photo_preview.dart';
import 'project_page.dart';
import 'video_preview.dart';

/// Videos and photos as a grid of pictures to choose from: a category's, a
/// person's, a memory's. A tap plays a video or shows a photo large, with
/// choosing it one tap away; what is chosen starts a new project.
class VideoGrid extends StatefulWidget {
  const VideoGrid({
    super.key,
    required this.engine,
    required this.videos,
    required this.chosen,
    required this.onChanged,
    this.shrinkWrap = false,
  });

  final Engine engine;
  final List<CatalogVideo> videos;

  /// Chosen videos, by path. Owned by the page, so it can act on them.
  final Set<String> chosen;
  final VoidCallback onChanged;

  /// Inside another scrolling list rather than filling the page.
  final bool shrinkWrap;

  @override
  State<VideoGrid> createState() => _VideoGridState();
}

class _VideoGridState extends State<VideoGrid> {
  final Map<String, Uint8List> _thumbs = {};
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    unawaited(_loadThumbs());
  }

  @override
  void didUpdateWidget(VideoGrid oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.videos != widget.videos) unawaited(_loadThumbs());
  }

  /// Six at a time from the top, as the footage picker makes them, so the
  /// first row shows while the rest are made.
  Future<void> _loadThumbs() async {
    if (_loading) return;
    _loading = true;
    try {
      final paths = [
        for (final v in widget.videos)
          if (!_thumbs.containsKey(v.path)) v.path,
      ];
      for (var i = 0; i < paths.length; i += 6) {
        final batch = paths.sublist(i, (i + 6).clamp(0, paths.length));
        final got = await widget.engine.thumbnails(batch, width: 240);
        if (!mounted) return;
        setState(() => _thumbs.addAll(got));
      }
    } on EngineException {
      // A picture that cannot be made leaves its tile plain.
    } finally {
      _loading = false;
    }
  }

  void _toggle(String path) {
    if (!widget.chosen.remove(path)) widget.chosen.add(path);
    widget.onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    if (widget.videos.isEmpty) {
      return Padding(
        padding: const EdgeInsets.all(40),
        child: Center(child: Text(s.gridEmpty)),
      );
    }
    return GridView.builder(
      shrinkWrap: widget.shrinkWrap,
      physics: widget.shrinkWrap ? const NeverScrollableScrollPhysics() : null,
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 96),
      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
        maxCrossAxisExtent: 220,
        mainAxisSpacing: 12,
        crossAxisSpacing: 12,
        childAspectRatio: 0.82,
      ),
      itemCount: widget.videos.length,
      itemBuilder: (context, i) {
        final video = widget.videos[i];
        return _VideoTile(
          key: Key('video-${video.path}'),
          video: video,
          thumb: _thumbs[video.path],
          chosen: widget.chosen.contains(video.path),
          onToggle: () => _toggle(video.path),
          onOpen: video.isPhoto
              ? () => unawaited(PhotoPreview.show(
                    context,
                    engine: widget.engine,
                    path: video.path,
                    chosen: widget.chosen.contains(video.path),
                    onToggle: () => _toggle(video.path),
                  ))
              : VideoPreview.available
                  ? () => unawaited(VideoPreview.show(
                        context,
                        path: video.path,
                        chosen: widget.chosen.contains(video.path),
                        onToggle: () => _toggle(video.path),
                      ))
                  : () => _toggle(video.path),
        );
      },
    );
  }
}

String _clock(int us) {
  final seconds = us ~/ 1000000;
  return '${seconds ~/ 60}:${(seconds % 60).toString().padLeft(2, '0')}';
}

String _date(DateTime t) =>
    '${t.year}.${t.month.toString().padLeft(2, '0')}.${t.day.toString().padLeft(2, '0')}';

class _VideoTile extends StatelessWidget {
  const _VideoTile({
    super.key,
    required this.video,
    required this.thumb,
    required this.chosen,
    required this.onToggle,
    required this.onOpen,
  });

  final CatalogVideo video;
  final Uint8List? thumb;
  final bool chosen;
  final VoidCallback onToggle;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final captured = video.captured;
    return Material(
      color: chosen ? theme.colorScheme.primaryContainer : theme.colorScheme.surfaceContainerLow,
      borderRadius: BorderRadius.circular(14),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onOpen,
        onLongPress: onToggle,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: Stack(
                fit: StackFit.expand,
                children: [
                  ColoredBox(
                    color: Colors.black,
                    child: thumb == null
                        ? Icon(
                            video.isPhoto ? Icons.photo_outlined : Icons.movie_outlined,
                            color: theme.colorScheme.outline,
                          )
                        : Image.memory(thumb!, fit: BoxFit.cover, gaplessPlayback: true),
                  ),
                  // A video's running time; a photo has none.
                  if (!video.isPhoto)
                    Positioned(
                      right: 6,
                      bottom: 6,
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          color: Colors.black.withValues(alpha: 0.6),
                          borderRadius: BorderRadius.circular(6),
                        ),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                          child: Text(
                            _clock(video.durationUs),
                            style: const TextStyle(color: Colors.white, fontSize: 11),
                          ),
                        ),
                      ),
                    ),
                  Positioned(
                    left: 4,
                    top: 4,
                    // On a dark disc, so it reads over a white sky as over
                    // a night street.
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: Colors.black.withValues(alpha: 0.32),
                        shape: BoxShape.circle,
                      ),
                      child: Checkbox(
                        key: Key('choose-${video.path}'),
                        value: chosen,
                        onChanged: (_) => onToggle(),
                        shape: const CircleBorder(),
                        side: const BorderSide(color: Colors.white, width: 1.5),
                      ),
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(10, 8, 10, 10),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    captured == null
                        ? video.name
                        : '${_date(captured)}${video.fromCamera ? '' : ' · ${s.fileTimeMark}'}',
                    style: theme.textTheme.labelLarge,
                    overflow: TextOverflow.ellipsis,
                  ),
                  Text(
                    video.place ?? video.name,
                    style: theme.textTheme.labelSmall
                        ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// A page of videos and photos: a category's or a person's.
class VideoGridPage extends StatefulWidget {
  const VideoGridPage({
    super.key,
    required this.engine,
    required this.title,
    required this.load,
    this.categoryId,
    this.actions = const [],
  });

  final Engine engine;
  final String title;
  final Future<List<CatalogVideo>> Function() load;

  /// The category shown, which can be removed from here.
  final String? categoryId;
  final List<Widget> actions;

  @override
  State<VideoGridPage> createState() => _VideoGridPageState();
}

class _VideoGridPageState extends State<VideoGridPage> {
  List<CatalogVideo>? _videos;
  String? _error;
  final Set<String> _chosen = {};

  int get _chosenPhotos => [
        for (final v in _videos ?? const <CatalogVideo>[])
          if (v.isPhoto && _chosen.contains(v.path)) v,
      ].length;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final videos = await widget.load();
      if (!mounted) return;
      setState(() => _videos = videos);
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

  Future<void> _removeCategory() async {
    final s = S.of(context);
    final id = widget.categoryId;
    if (id == null) return;
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.removeCategoryTitle(widget.title)),
        content: Text(s.removeCategoryBody),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(false), child: Text(s.cancel)),
          FilledButton(
            key: const Key('remove-category-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.removeCategory),
          ),
        ],
      ),
    );
    if (yes != true) return;
    try {
      await widget.engine.removeCategory(id);
      if (mounted) Navigator.of(context).pop(true);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _newProject() async {
    final s = S.of(context);
    final videos = _videos ?? const [];
    final chosen = [
      for (final v in videos)
        if (_chosen.contains(v.path)) v.path,
    ];
    final controller = TextEditingController(text: widget.title);
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.newProject),
        content: TextField(
          key: const Key('grid-project-name'),
          controller: controller,
          autofocus: true,
          decoration: InputDecoration(labelText: s.name),
          onSubmitted: (v) => Navigator.of(context).pop(v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(s.cancel)),
          FilledButton(
            key: const Key('grid-project-create'),
            onPressed: () => Navigator.of(context).pop(controller.text),
            child: Text(s.create),
          ),
        ],
      ),
    );
    if (name == null || name.trim().isEmpty) return;
    try {
      final project = await widget.engine.projectOf(name.trim(), chosen);
      if (!mounted) return;
      await Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => ProjectPage(engine: widget.engine, project: project),
        ),
      );
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final videos = _videos;
    final all = videos != null && videos.isNotEmpty && _chosen.length == videos.length;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.title),
        actions: [
          if (videos != null && videos.isNotEmpty)
            TextButton(
              key: const Key('select-all'),
              onPressed: () => setState(() {
                if (all) {
                  _chosen.clear();
                } else {
                  _chosen.addAll(videos.map((v) => v.path));
                }
              }),
              child: Text(all ? s.selectNone : s.selectAll),
            ),
          ...widget.actions,
          if (widget.categoryId != null)
            IconButton(
              key: const Key('remove-category'),
              tooltip: s.removeCategory,
              icon: const Icon(Icons.delete_outline_rounded),
              onPressed: () => unawaited(_removeCategory()),
            ),
          const SizedBox(width: 4),
        ],
      ),
      body: _error != null
          ? Center(child: Text(_error!))
          : videos == null
              ? const Center(child: CircularProgressIndicator())
              : VideoGrid(
                  engine: widget.engine,
                  videos: videos,
                  chosen: _chosen,
                  onChanged: () => setState(() {}),
                ),
      floatingActionButtonLocation: FloatingActionButtonLocation.centerFloat,
      floatingActionButton: _chosen.isEmpty
          ? null
          : FloatingActionButton.extended(
              key: const Key('grid-new-project'),
              onPressed: () => unawaited(_newProject()),
              icon: const Icon(Icons.movie_edit),
              label: Text(s.newProjectFrom(
                _chosen.length - _chosenPhotos,
                _chosenPhotos,
              )),
            ),
    );
  }
}

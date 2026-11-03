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
import 'subject_page.dart';
import 'thumbnail.dart';
import 'widgets.dart';

/// The people who keep turning up in the footage, across every project.
///
/// Grouped by how alike they look, not recognised, and the page says so in
/// the engine's own words. A group has no name until the user gives it one,
/// and the name then follows that person into every later project. Tapping
/// one opens every project and clip they are in.
class AlbumPage extends StatefulWidget {
  const AlbumPage({super.key, required this.engine, this.embedded = false});

  final Engine engine;
  final bool embedded;

  @override
  State<AlbumPage> createState() => _AlbumPageState();
}

class _AlbumPageState extends State<AlbumPage> {
  Album? _album;
  String? _error;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final album = await widget.engine.album();
      if (!mounted) return;
      setState(() {
        _album = album;
        _error = null;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _open(Subject subject) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => SubjectPage(engine: widget.engine, subject: subject),
      ),
    );
    // Named, removed, or re-run from there.
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final album = _album;
    final body = Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 980),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
          children: [
            if (_error != null)
              NoticeBanner(title: s.albumUnreadable, lines: [_error!], tone: Tone.bad),
            if (album == null && _error == null)
              const Padding(
                padding: EdgeInsets.all(40),
                child: Center(child: CircularProgressIndicator()),
              ),
            if (album != null) ...[
              NoticeBanner(
                title: s.peopleInFootage,
                lines: [
                  album.grouping,
                  s.albumNameHint,
                ],
                icon: Icons.face_retouching_natural_rounded,
              ),
              const SizedBox(height: 16),
              if (album.subjects.isEmpty)
                Padding(
                  padding: const EdgeInsets.all(32),
                  child: Center(child: Text(s.albumEmpty)),
                )
              else
                Wrap(
                  spacing: 16,
                  runSpacing: 16,
                  children: [
                    for (final subject in album.subjects)
                      SubjectTile(
                        subject: subject,
                        onTap: () => unawaited(_open(subject)),
                      ),
                  ],
                ),
            ],
          ],
        ),
      ),
    );
    if (widget.embedded) return body;
    return Scaffold(appBar: AppBar(title: Text(s.navAlbum)), body: body);
  }
}

/// A face and a name, for the album and for the strip on the projects page.
class SubjectTile extends StatelessWidget {
  const SubjectTile({
    super.key,
    required this.subject,
    this.onTap,
    this.size = 112,
    this.caption,
  });

  final Subject subject;
  final VoidCallback? onTap;
  final double size;

  /// What is under the name in place of the count of clips.
  final String? caption;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(16),
      child: SizedBox(
        width: size,
        child: Column(
          children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(size / 2),
              child: SizedBox(
                width: size * 0.78,
                height: size * 0.78,
                child: ColoredBox(
                  color: theme.colorScheme.surfaceContainerHighest,
                  child: localImage(
                    subject.thumbnail,
                    fallback: Icon(
                      Icons.person_rounded,
                      size: size * 0.4,
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(height: 6),
            Text(
              subject.label ?? s.unnamed,
              style: theme.textTheme.bodySmall?.copyWith(
                fontWeight: subject.label == null ? FontWeight.normal : FontWeight.w600,
                color: subject.label == null
                    ? theme.colorScheme.onSurfaceVariant
                    : theme.colorScheme.onSurface,
              ),
              overflow: TextOverflow.ellipsis,
            ),
            Text(
              caption ?? s.clips(subject.clips),
              style: theme.textTheme.labelSmall
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ],
        ),
      ),
    );
  }
}

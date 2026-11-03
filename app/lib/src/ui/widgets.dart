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

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../l10n/strings.dart';
import '../theme.dart';

/// A titled card, which is the only container shape this app uses.
class SectionCard extends StatelessWidget {
  const SectionCard({
    super.key,
    required this.title,
    required this.child,
    this.subtitle,
    this.trailing,
    this.icon,
  });

  final String title;
  final String? subtitle;
  final Widget child;
  final Widget? trailing;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(20),
        // On a phone the trailing button goes under the heading: beside it,
        // it squeezed the subtitle into a column a few characters wide.
        child: LayoutBuilder(
          builder: (context, constraints) {
            final narrow = constraints.maxWidth < 480;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    if (icon != null) ...[
                      Icon(icon, size: 18, color: theme.colorScheme.primary),
                      const SizedBox(width: 10),
                    ],
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(title, style: theme.textTheme.titleMedium),
                          if (subtitle != null)
                            Padding(
                              padding: const EdgeInsets.only(top: 2),
                              child: Text(
                                subtitle!,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: theme.colorScheme.onSurfaceVariant,
                                ),
                              ),
                            ),
                        ],
                      ),
                    ),
                    if (trailing != null && !narrow) trailing!,
                  ],
                ),
                if (trailing != null && narrow) ...[
                  const SizedBox(height: 12),
                  Align(alignment: Alignment.centerLeft, child: trailing!),
                ],
                const SizedBox(height: 16),
                child,
              ],
            );
          },
        ),
      ),
    );
  }
}

/// A small status pill. [tone] carries meaning, so it is never decoration.
enum Tone { neutral, good, warning, bad }

class StatusChip extends StatelessWidget {
  const StatusChip({super.key, required this.label, this.tone = Tone.neutral, this.icon});

  final String label;
  final Tone tone;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (background, foreground) = switch (tone) {
      Tone.good => (scheme.primaryContainer, scheme.onPrimaryContainer),
      Tone.warning => (scheme.tertiaryContainer, scheme.onTertiaryContainer),
      Tone.bad => (scheme.errorContainer, scheme.onErrorContainer),
      Tone.neutral => (scheme.surfaceContainerHighest, scheme.onSurfaceVariant),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(20),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (icon != null) ...[
            Icon(icon, size: 13, color: foreground),
            const SizedBox(width: 5),
          ],
          Text(
            label,
            style: Theme.of(context)
                .textTheme
                .labelSmall
                ?.copyWith(color: foreground, fontWeight: FontWeight.w600),
          ),
        ],
      ),
    );
  }
}

/// A labelled number. Measurements live here rather than in sentences, so
/// they are quick to compare and do not interrupt reading.
class Stat extends StatelessWidget {
  const Stat({super.key, required this.label, required this.value, this.tone});

  final String label;
  final String value;
  final Tone? tone;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final color = switch (tone) {
      Tone.good => theme.colorScheme.primary,
      Tone.bad => theme.colorScheme.error,
      Tone.warning => theme.colorScheme.tertiary,
      _ => theme.colorScheme.onSurface,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          value,
          style: theme.textTheme.headlineSmall
              ?.copyWith(color: color, fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 2),
        Text(
          label,
          style: theme.textTheme.bodySmall
              ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
        ),
      ],
    );
  }
}

/// A filesystem path with a copy button.
///
/// Copying matters more than it looks: on the web build the app cannot open a
/// file manager, so the path is how a result reaches the rest of the desktop.
class CopyablePath extends StatelessWidget {
  const CopyablePath({super.key, required this.label, required this.path});

  final String label;
  final String path;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 110,
            child: Text(
              label,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ),
          Expanded(
            child: SelectableText(path, style: monoStyle(context, size: 11.5)),
          ),
          IconButton(
            icon: const Icon(Icons.copy_rounded, size: 15),
            visualDensity: VisualDensity.compact,
            tooltip: s.copyPath,
            onPressed: () async {
              await Clipboard.setData(ClipboardData(text: path));
              // The context can go away across the await, so it is checked
              // again rather than assumed still valid.
              if (!context.mounted) return;
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  content: Text(s.copied(label)),
                  behavior: SnackBarBehavior.floating,
                  width: 260,
                ),
              );
            },
          ),
        ],
      ),
    );
  }
}

/// A 0..1 score as a bar. Reading a column of these is much faster than
/// reading a column of decimals.
class ScoreBar extends StatelessWidget {
  const ScoreBar({super.key, required this.score, this.width = 56, this.dimmed = false});

  final double score;
  final double width;
  final bool dimmed;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final fill = dimmed ? scheme.outline : scheme.primary;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: width,
          height: 6,
          decoration: BoxDecoration(
            color: scheme.surfaceContainerHighest,
            borderRadius: BorderRadius.circular(3),
          ),
          child: FractionallySizedBox(
            alignment: Alignment.centerLeft,
            widthFactor: score.clamp(0.0, 1.0),
            child: Container(
              decoration: BoxDecoration(
                color: fill,
                borderRadius: BorderRadius.circular(3),
              ),
            ),
          ),
        ),
        const SizedBox(width: 8),
        Text(score.toStringAsFixed(2), style: monoStyle(context, size: 11)),
      ],
    );
  }
}

/// A message that needs to be read, not just noticed.
class NoticeBanner extends StatelessWidget {
  const NoticeBanner({
    super.key,
    required this.title,
    required this.lines,
    this.tone = Tone.warning,
    this.icon = Icons.info_outline_rounded,
  });

  final String title;
  final List<String> lines;
  final Tone tone;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (background, foreground) = switch (tone) {
      Tone.bad => (scheme.errorContainer, scheme.onErrorContainer),
      Tone.good => (scheme.primaryContainer, scheme.onPrimaryContainer),
      _ => (scheme.tertiaryContainer, scheme.onTertiaryContainer),
    };
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(14),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 18, color: foreground),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: Theme.of(context)
                      .textTheme
                      .titleSmall
                      ?.copyWith(color: foreground),
                ),
                for (final line in lines)
                  Padding(
                    padding: const EdgeInsets.only(top: 5),
                    child: Text(
                      line,
                      style: Theme.of(context)
                          .textTheme
                          .bodySmall
                          ?.copyWith(color: foreground),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

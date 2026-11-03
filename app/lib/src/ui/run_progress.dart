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

import '../api_models.dart';
import '../l10n/strings.dart';
import '../theme.dart';
import 'widgets.dart';

/// Where a run has got to: every stage, the current one marked, the overall
/// percentage, which clip is being read, and a way to stop.
///
/// A run over a folder takes minutes. A spinner for that long cannot be told
/// apart from a hang; a list that ticks through "Watching the footage · clip
/// 3 of 12 · keyframes" can.
class RunProgressCard extends StatelessWidget {
  const RunProgressCard({
    super.key,
    required this.status,
    required this.onCancel,
    this.cancelling = false,
  });

  final JobStatus status;
  final VoidCallback onCancel;
  final bool cancelling;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final percent = status.percent.clamp(0, 100) / 100.0;
    final currentIndex = status.stages.indexWhere((s) => s.id == status.stage);

    return SectionCard(
      title: status.stage.isEmpty
          ? s.starting
          : s.stageLabel(status.stage, status.stageLabel),
      // Several clips are analysed at once, so the count is of clips
      // finished, with how many are under way.
      subtitle: status.sourceCount > 1 && status.stage == 'analyse'
          // While the heat guard acts, the other workers hold still, and
          // the line under the bar says why.
          ? s.clipsAnalysing(
              status.sourceIndex,
              status.sourceCount,
              status.thermal == null ? status.activeSources : 1,
            )
          : null,
      icon: Icons.hourglass_top_rounded,
      trailing: Text(
        '${status.percent.toStringAsFixed(0)}%',
        key: const Key('run-percent'),
        style: theme.textTheme.titleLarge?.copyWith(
          color: theme.colorScheme.primary,
          fontWeight: FontWeight.w600,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ClipRRect(
            borderRadius: BorderRadius.circular(6),
            child: LinearProgressIndicator(
              value: percent,
              minHeight: 10,
            ),
          ),
          const SizedBox(height: 10),
          Text(
            s.stepLabel(status.step, status.file, status.detail),
            style: monoStyle(context, size: 11.5),
            overflow: TextOverflow.ellipsis,
          ),
          // Said, so a run that slows on a hot device does not read as a
          // hung one.
          if (status.thermal != null) ...[
            const SizedBox(height: 10),
            Row(
              key: const Key('run-thermal'),
              children: [
                Icon(
                  Icons.device_thermostat_rounded,
                  size: 18,
                  color: status.thermal == 'cooling'
                      ? theme.colorScheme.error
                      : theme.colorScheme.tertiary,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    status.thermal == 'cooling' ? s.thermalCooling : s.thermalSlowing,
                    style: theme.textTheme.bodySmall,
                  ),
                ),
              ],
            ),
          ],
          const SizedBox(height: 16),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (var i = 0; i < status.stages.length; i++)
                StatusChip(
                  label: s.stageLabel(status.stages[i].id, status.stages[i].label),
                  tone: currentIndex > i || status.stage == 'done'
                      ? Tone.good
                      : i == currentIndex
                          ? Tone.warning
                          : Tone.neutral,
                  icon: currentIndex > i || status.stage == 'done'
                      ? Icons.check_rounded
                      : i == currentIndex
                          ? Icons.more_horiz_rounded
                          : null,
                ),
            ],
          ),
          const SizedBox(height: 16),
          Align(
            alignment: Alignment.centerRight,
            child: OutlinedButton.icon(
              key: const Key('run-cancel'),
              onPressed: cancelling ? null : onCancel,
              icon: const Icon(Icons.stop_rounded, size: 18),
              label: Text(cancelling ? s.stopping : s.stop),
            ),
          ),
        ],
      ),
    );
  }
}

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
import '../models.dart';
import '../theme.dart';
import 'widgets.dart';

/// What the engine produced, laid out so it can be judged rather than just
/// accepted: the numbers, every shot including the ones left out, the reason
/// behind each chosen clip, and an honest note about what was mocked.
class ResultView extends StatelessWidget {
  const ResultView({
    super.key,
    required this.result,
    this.onOpenEditor,
    this.onReveal,
    this.phoneEditors = const [],
    this.onHandOff,
  });

  final RunResult result;

  /// Phone editors installed here, which take the cut clip by clip.
  final List<EditorInfo> phoneEditors;

  /// Cut the clips and open them in a phone editor.
  final void Function(EditorInfo editor)? onHandOff;

  /// Open an editor by id, when the host can.
  final void Function(String editorId)? onOpenEditor;

  /// Show a file or folder in the desktop's file manager, when the host can.
  final void Function(String path)? onReveal;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (result.warnings.isNotEmpty) ...[
          NoticeBanner(
            title: s.roughStructure,
            lines: result.warnings,
            icon: Icons.science_outlined,
          ),
          const SizedBox(height: 16),
        ],
        if (onHandOff != null && result.handoff.clips.isNotEmpty)
          for (final editor in phoneEditors) ...[
            _handOff(context, editor),
            const SizedBox(height: 16),
          ],
        if (result.editorDrafts.isNotEmpty) ...[
          _editorDrafts(context),
          const SizedBox(height: 16),
        ],
        _summary(context),
        if (result.adapterNotes.isNotEmpty) ...[
          const SizedBox(height: 16),
          _howItWasMade(context),
        ],
        const SizedBox(height: 16),
        _cut(context),
        const SizedBox(height: 16),
        _shots(context),
        if (result.rejections.isNotEmpty) ...[
          const SizedBox(height: 16),
          _rejections(context),
        ],
        const SizedBox(height: 16),
        _outputs(context),
      ],
    );
  }

  /// On a phone the editor takes the cut as clips, so what goes and what
  /// stays behind is said before the button, not found out after.
  Widget _handOff(BuildContext context, EditorInfo editor) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final handoff = result.handoff;
    final music = handoff.music;
    final lines = [
      s.handOffBody(handoff.clips.length, editor.name),
      s.handOffLeftOut(handoff.titleCards, handoff.texts, handoff.transitions),
      if (music != null)
        s.handOffMusic(music.split('/').last, formatDuration(handoff.musicInUs ?? 0)),
    ].where((line) => line.isNotEmpty);
    return SectionCard(
      title: s.handOffTitle(editor.name),
      icon: Icons.send_to_mobile_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final line in lines)
            Padding(
              padding: const EdgeInsets.only(bottom: 6),
              child: Text(
                line,
                style: theme.textTheme.bodySmall
                    ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
              ),
            ),
          const SizedBox(height: 6),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton.icon(
              key: Key('hand-off-${editor.id}'),
              onPressed: () => onHandOff!(editor),
              icon: const Icon(Icons.open_in_new_rounded, size: 18),
              label: Text(s.handOffButton(editor.name)),
            ),
          ),
        ],
      ),
    );
  }

  /// The drafts that went into an editor's own folder: the result the user
  /// actually came for, so it comes first.
  Widget _editorDrafts(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return SectionCard(
      title: s.readyInEditor,
      subtitle: s.readyInEditorSubtitle,
      icon: Icons.task_alt_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final draft in result.editorDrafts)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          [draft.editorName, draft.name]
                              .where((part) => part.isNotEmpty)
                              .join(' · '),
                          style: theme.textTheme.bodyMedium
                              ?.copyWith(fontWeight: FontWeight.w600),
                        ),
                        const SizedBox(height: 2),
                        Text(
                          s.draftFacts(
                            draft.clips,
                            draft.hasMusic,
                            draft.transitionsDropped,
                            draft.mediaLinked,
                            draft.mediaUnreadable,
                          ),
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                      ],
                    ),
                  ),
                  if (onReveal != null)
                    IconButton(
                      tooltip: s.showDraftFolder,
                      icon: const Icon(Icons.folder_open_rounded, size: 18),
                      onPressed: () => onReveal!(draft.path),
                    ),
                  if (onOpenEditor != null && draft.editor.isNotEmpty)
                    FilledButton.tonalIcon(
                      onPressed: () => onOpenEditor!(draft.editor),
                      icon: const Icon(Icons.open_in_new_rounded, size: 18),
                      label: Text(s.openEditor(draft.editorName)),
                    ),
                ],
              ),
            ),
          const SizedBox(height: 4),
          Text(
            s.restartEditorNote,
            style: theme.textTheme.bodySmall
                ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        ],
      ),
    );
  }

  /// Which models ran, in the engine's own words.
  Widget _howItWasMade(BuildContext context) {
    final theme = Theme.of(context);
    return SectionCard(
      title: S.of(context).howItWasMade,
      icon: Icons.memory_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final note in result.adapterNotes)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child: Text(note, style: theme.textTheme.bodySmall),
            ),
        ],
      ),
    );
  }

  Widget _summary(BuildContext context) {
    final s = S.of(context);
    return SectionCard(
      title: s.result,
      subtitle: [
        s.styleName(result.templateId, result.templateName),
        if (result.beat != null)
          s.cutOnBeat(s.beatDescription(result.beat!.origin, result.beat!.bpm)),
      ].join(' · '),
      icon: Icons.auto_awesome_rounded,
      // Says how much was reused, not merely that something was: with a
      // project of clips, "3 of 4" is the number that explains the wait.
      trailing: result.reusedSources > 0
          ? StatusChip(
              label: result.reusedSources == result.totalSources
                  ? s.reusedAnalysis
                  : s.reusedOf(result.reusedSources, result.totalSources),
              icon: Icons.bolt_rounded,
              tone: Tone.good,
            )
          : null,
      child: Wrap(
        spacing: 40,
        runSpacing: 20,
        children: [
          Stat(label: s.durationStat, value: formatDuration(result.durationUs)),
          Stat(label: s.shotsFound, value: '${result.shotsDetected}'),
          Stat(label: s.clipsUsed, value: '${result.clipsUsed}'),
          if (result.totalSources > 1)
            Stat(label: s.videos, value: '${result.totalSources}'),
          if (result.subjects > 0)
            Stat(label: s.peopleFoundStat, value: '${result.subjects}'),
          Stat(
            label: s.grounded,
            value: '${result.verified}',
            tone: Tone.good,
          ),
          Stat(
            label: s.rejected,
            value: '${result.rejected}',
            tone: result.rejected > 0 ? Tone.bad : null,
          ),
        ],
      ),
    );
  }

  Widget _cut(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return SectionCard(
      title: s.theCut,
      subtitle: s.theCutSubtitle,
      icon: Icons.view_timeline_outlined,
      child: Column(
        children: [
          for (final slot in result.slots) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 10),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(
                    width: 64,
                    child: Text(
                      formatDuration(slot.timelineStartUs),
                      style: monoStyle(context, size: 11.5),
                    ),
                  ),
                  Container(
                    width: 3,
                    height: 34,
                    decoration: BoxDecoration(
                      color: slot.isTitleCard
                          ? theme.colorScheme.tertiary
                          : theme.colorScheme.primary,
                      borderRadius: BorderRadius.circular(2),
                    ),
                  ),
                  const SizedBox(width: 14),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            if (result.sources[slot.sourceId] case final file?) ...[
                              Icon(
                                file.isPhoto
                                    ? Icons.photo_outlined
                                    : Icons.movie_creation_outlined,
                                size: 15,
                                color: theme.colorScheme.onSurfaceVariant,
                              ),
                              const SizedBox(width: 6),
                            ],
                            Flexible(
                              child: Text(
                                slot.isTitleCard
                                    ? s.titleCard
                                    // The file it comes from, which is what a
                                    // person recognises; the shot id when the
                                    // engine did not say.
                                    : (result.sources[slot.sourceId]?.name ??
                                        slot.shotId ??
                                        slot.id),
                                overflow: TextOverflow.ellipsis,
                                style: theme.textTheme.bodyMedium?.copyWith(
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            const SizedBox(width: 10),
                            Text(
                              formatDuration(slot.durationUs),
                              style: monoStyle(context, size: 11),
                            ),
                          ],
                        ),
                        if (slot.reason != null)
                          Padding(
                            padding: const EdgeInsets.only(top: 3),
                            child: Text(
                              slot.reason!,
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: theme.colorScheme.onSurfaceVariant,
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            if (slot != result.slots.last) const Divider(height: 1),
          ],
        ],
      ),
    );
  }

  Widget _shots(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    return SectionCard(
      title: s.everyShot,
      subtitle: s.everyShotSubtitle,
      icon: Icons.grid_view_rounded,
      child: Column(
        children: [
          for (final shot in result.shots) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 9),
              child: Row(
                children: [
                  SizedBox(
                    width: 22,
                    child: Icon(
                      shot.used
                          ? Icons.check_circle_rounded
                          : Icons.remove_circle_outline_rounded,
                      size: 16,
                      color: shot.used
                          ? theme.colorScheme.primary
                          : theme.colorScheme.outline,
                    ),
                  ),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          shot.id,
                          style: monoStyle(
                            context,
                            size: 11.5,
                            color: shot.used
                                ? theme.colorScheme.onSurface
                                : theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                        // What was seen in it, when something looked.
                        if (shot.description != null &&
                            shot.description!.isNotEmpty)
                          Text(
                            shot.description!,
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                      ],
                    ),
                  ),
                  SizedBox(
                    width: 130,
                    child: Text(
                      '${formatDuration(shot.startUs)} - ${formatDuration(shot.endUs)}',
                      style: monoStyle(context, size: 11),
                    ),
                  ),
                  ScoreBar(score: shot.score, dimmed: !shot.used),
                ],
              ),
            ),
            if (shot != result.shots.last) const Divider(height: 1),
          ],
        ],
      ),
    );
  }

  Widget _rejections(BuildContext context) {
    final s = S.of(context);
    return SectionCard(
      title: s.rejectedByGrounding,
      subtitle: s.rejectedByGroundingSubtitle,
      icon: Icons.gpp_maybe_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final rejection in result.rejections)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 5),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(
                    width: 130,
                    child: Text(rejection.slotId,
                        style: monoStyle(context, size: 11.5)),
                  ),
                  Expanded(
                    child: Text(
                      rejection.reason,
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }

  Widget _outputs(BuildContext context) {
    final s = S.of(context);
    return SectionCard(
      title: s.filesWritten,
      subtitle: s.filesWrittenSubtitle,
      icon: Icons.save_alt_rounded,
      child: Column(
        children: [
          CopyablePath(label: s.draftLabel, path: result.draftPath),
          CopyablePath(label: s.footageGraph, path: result.footageGraphPath),
          CopyablePath(label: s.editingDsl, path: result.editingDslPath),
        ],
      ),
    );
  }
}

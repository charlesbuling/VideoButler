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

/// The editor's timeline: the piece laid out left to right, the playhead
/// fixed in the middle and the tracks scrolling under it, as phone editors
/// do it.
///
/// Every position drawn here is the engine's (`EditState`), resolved from
/// the DSL; the timeline adds no durations of its own. Changes it makes --
/// a trim, a selection, a scrub -- go back as calls, and the engine answers
/// with the cut as it now is.
library;

import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models.dart';
import 'edit_state.dart';

const _rulerHeight = 22.0;
const _clipHeight = 56.0;
const _laneHeight = 26.0;
const _gap = 6.0;
const _handleWidth = 14.0;

/// Pixels per second the timeline can be zoomed between.
const minZoom = 6.0;
const maxZoom = 400.0;

/// The key a film-strip picture is cached under.
String stripKey(String sourceId, int atUs) => '$sourceId@$atUs';

class EditTimeline extends StatefulWidget {
  const EditTimeline({
    super.key,
    required this.state,
    required this.playheadUs,
    required this.zoom,
    required this.selection,
    required this.strip,
    required this.onScrub,
    required this.onSelect,
    required this.onTrim,
    required this.onDuration,
    required this.onJoin,
    required this.onZoom,
    required this.onNeedStrip,
    required this.onAdd,
    this.following = false,
  });

  final EditState state;
  final int playheadUs;

  /// Pixels per second.
  final double zoom;
  final EditSelection? selection;

  /// Film-strip pictures, by [stripKey].
  final Map<String, Uint8List> strip;

  /// The person moved the playhead by scrolling.
  final ValueChanged<int> onScrub;
  final ValueChanged<EditSelection?> onSelect;

  /// A clip's in or out point dragged to a new moment of its footage.
  final void Function(String slotId, {int? inUs, int? outUs}) onTrim;

  /// A title card's, gap's or photograph's length dragged.
  final void Function(String slotId, int durationUs) onDuration;

  /// The join after a clip tapped.
  final ValueChanged<String> onJoin;
  final ValueChanged<double> onZoom;

  /// Film-strip pictures wanted and not yet held: (source, moment).
  final ValueChanged<List<(String, int)>> onNeedStrip;
  final VoidCallback onAdd;

  /// The piece is playing: the timeline follows the playhead rather than
  /// moving it.
  final bool following;

  @override
  State<EditTimeline> createState() => _EditTimelineState();
}

class _EditTimelineState extends State<EditTimeline> {
  final ScrollController _scroll = ScrollController();
  bool _syncing = false;
  double _viewport = 0;

  /// A trim in progress: which clip, which end, how far.
  String? _trimSlot;
  bool _trimStart = false;
  double _trimDx = 0;

  double get _pad => _viewport / 2;
  double _x(int us) => _pad + us / 1e6 * widget.zoom;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_scrolled);
  }

  @override
  void dispose() {
    _scroll.removeListener(_scrolled);
    _scroll.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(EditTimeline old) {
    super.didUpdateWidget(old);
    if (old.playheadUs != widget.playheadUs || old.zoom != widget.zoom) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _follow());
    }
  }

  /// Put the playhead's moment under the middle line.
  void _follow() {
    if (!mounted || !_scroll.hasClients) return;
    final target = (widget.playheadUs / 1e6 * widget.zoom)
        .clamp(0.0, _scroll.position.maxScrollExtent);
    if ((_scroll.offset - target).abs() < 0.5) return;
    _syncing = true;
    _scroll.jumpTo(target);
    _syncing = false;
  }

  void _scrolled() {
    if (_syncing || widget.following) return;
    final us = (_scroll.offset / widget.zoom * 1e6).round().clamp(0, widget.state.totalUs);
    if (us != widget.playheadUs) widget.onScrub(us);
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return LayoutBuilder(builder: (context, box) {
      if (_viewport != box.maxWidth) {
        _viewport = box.maxWidth;
        WidgetsBinding.instance.addPostFrameCallback((_) => _follow());
      }
      final contentWidth = _x(widget.state.totalUs) + _pad;
      const height = _rulerHeight + _clipHeight + _laneHeight * 2 + _gap * 4;
      return Listener(
        // Ctrl or ⌘ with the wheel zooms, as in desktop editors.
        onPointerSignal: (event) {
          if (event is PointerScrollEvent) {
            final keys = HardwareKeyboard.instance;
            if (keys.isControlPressed || keys.isMetaPressed) {
              final factor = math.pow(1.0015, -event.scrollDelta.dy).toDouble();
              widget.onZoom((widget.zoom * factor).clamp(minZoom, maxZoom));
            }
          }
        },
        child: Stack(
          children: [
            SingleChildScrollView(
              key: const Key('timeline-scroll'),
              controller: _scroll,
              scrollDirection: Axis.horizontal,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => widget.onSelect(null),
                child: SizedBox(
                  width: contentWidth,
                  height: height,
                  child: Stack(
                    clipBehavior: Clip.none,
                    children: [
                      _ruler(theme),
                      ..._clips(theme),
                      ..._joins(theme),
                      ..._texts(theme),
                      ..._sounds(theme),
                      _addButton(theme),
                    ],
                  ),
                ),
              ),
            ),
            // The playhead, fixed in the middle.
            Positioned(
              left: _viewport / 2 - 1,
              top: 0,
              bottom: 0,
              child: IgnorePointer(
                child: Container(width: 2, color: theme.colorScheme.onSurface),
              ),
            ),
          ],
        ),
      );
    });
  }

  Widget _ruler(ThemeData theme) {
    // A label every so many seconds that labels stay apart.
    const steps = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0];
    final step = steps.firstWhere((s) => s * widget.zoom >= 70, orElse: () => 600);
    final total = widget.state.totalUs / 1e6;
    final marks = <Widget>[];
    for (var t = 0.0; t <= total + 1e-6; t += step) {
      marks.add(Positioned(
        left: _x((t * 1e6).round()) - 1,
        top: 0,
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(width: 1, height: 8, color: theme.colorScheme.outline),
            const SizedBox(width: 3),
            Text(
              _clock(t),
              style: theme.textTheme.labelSmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ],
        ),
      ));
    }
    return Positioned(
      left: 0,
      right: 0,
      top: 0,
      height: _rulerHeight,
      child: Stack(clipBehavior: Clip.none, children: marks),
    );
  }

  static String _clock(double seconds) {
    final s = seconds.round();
    final whole = (seconds - seconds.floor()).abs() < 1e-6;
    if (!whole) return '${seconds.toStringAsFixed(1)}s';
    return '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';
  }

  /// Each clip's box runs from its start to the next one's, so boxes meet
  /// where the cut is and a transition's overlap shows as its marker.
  List<Widget> _clips(ThemeData theme) {
    final clips = widget.state.clips;
    final out = <Widget>[];
    final visibleFrom = _scroll.hasClients ? _scroll.offset - 100 : 0.0;
    final visibleTo = visibleFrom + _viewport + 200;
    final wanted = <(String, int)>[];
    for (var i = 0; i < clips.length; i++) {
      final clip = clips[i];
      final endUs = i + 1 < clips.length ? clips[i + 1].startUs : clip.endUs;
      var left = _x(clip.startUs);
      var right = _x(endUs);
      final selected = widget.selection == EditSelection('slot', clip.id);
      if (_trimSlot == clip.id) {
        if (_trimStart) {
          left = math.min(left + _trimDx, right - 8);
        } else {
          right = math.max(right + _trimDx, left + 8);
        }
      }
      final width = math.max(2.0, right - left - 2);
      out.add(Positioned(
        key: Key('clip-${clip.id}'),
        left: left + 1,
        top: _rulerHeight + _gap,
        width: width,
        height: _clipHeight,
        child: GestureDetector(
          onTap: () => widget.onSelect(EditSelection('slot', clip.id)),
          child: _clipBox(theme, clip, width, selected, left, visibleFrom, visibleTo, wanted),
        ),
      ));
      if (selected) {
        out.add(_handle(theme, clip, left: left - _handleWidth + 1, start: true));
        out.add(_handle(theme, clip, left: right - 1, start: false));
      }
    }
    if (wanted.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) widget.onNeedStrip(wanted);
      });
    }
    return out;
  }

  Widget _clipBox(
    ThemeData theme,
    EditClip clip,
    double width,
    bool selected,
    double left,
    double visibleFrom,
    double visibleTo,
    List<(String, int)> wanted,
  ) {
    final scheme = theme.colorScheme;
    final source = clip.sourceId == null ? null : widget.state.sources[clip.sourceId];
    final children = <Widget>[];
    if (clip.kind == SlotKind.titleCard || clip.kind == SlotKind.gap || source == null) {
      children.add(Positioned.fill(
        child: ColoredBox(color: scheme.surfaceContainerHighest),
      ));
      children.add(Center(
        child: Icon(
          clip.kind == SlotKind.titleCard ? Icons.title_rounded : Icons.crop_square_rounded,
          size: 20,
          color: scheme.onSurfaceVariant,
        ),
      ));
    } else {
      final aspect = source.height == 0 ? 16 / 9 : source.width / source.height;
      final thumbWidth = (_clipHeight * aspect).clamp(24.0, 120.0);
      final count = math.max(1, (width / thumbWidth).ceil());
      final tiles = <Widget>[];
      for (var k = 0; k < count; k++) {
        final x = left + k * thumbWidth;
        if (x + thumbWidth < visibleFrom || x > visibleTo) {
          tiles.add(SizedBox(width: thumbWidth, height: _clipHeight));
          continue;
        }
        final atPiece = clip.startUs + ((k + 0.5) * thumbWidth / widget.zoom * 1e6).round();
        // Asked for in quarter seconds, so neighbouring zooms share them.
        var at = source.isImage ? 0 : clip.sourceTimeAt(atPiece);
        at = at ~/ 250000 * 250000;
        final key = stripKey(source.id, at);
        final png = widget.strip[key];
        if (png == null) wanted.add((source.id, at));
        tiles.add(SizedBox(
          width: thumbWidth,
          height: _clipHeight,
          child: png == null
              ? ColoredBox(color: scheme.surfaceContainerHigh)
              : Image.memory(png, fit: BoxFit.cover, gaplessPlayback: true),
        ));
      }
      children.add(Positioned.fill(
        child: ClipRect(
          child: OverflowBox(
            alignment: Alignment.centerLeft,
            maxWidth: double.infinity,
            child: Row(mainAxisSize: MainAxisSize.min, children: tiles),
          ),
        ),
      ));
    }
    // What the clip carries, in a corner.
    final badges = <Widget>[
      if (clip.speed != 1) _badge('${_speedText(clip.speed)}x'),
      if (clip.muted) _badge(null, icon: Icons.volume_off_rounded),
      if (!clip.adjustment.isNeutral) _badge(null, icon: Icons.tune_rounded),
      if (!clip.placement.isFitted) _badge(null, icon: Icons.open_with_rounded),
    ];
    // On a clip too short to hold them all, the box's edge cuts them off,
    // as it does the film strip.
    children.add(Positioned(
      left: 4,
      top: 3,
      right: 4,
      height: 16,
      child: ClipRect(
        child: OverflowBox(
          alignment: Alignment.centerLeft,
          maxWidth: double.infinity,
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            _badge(formatDuration(clip.endUs - clip.startUs).replaceFirst(RegExp(r'^00:'), '')),
            const SizedBox(width: 3),
            ...badges,
          ]),
        ),
      ),
    ));
    if (source != null && !source.exists) {
      children.add(Positioned.fill(
        child: ColoredBox(
          color: scheme.error.withValues(alpha: 0.5),
          child: const Center(child: Icon(Icons.link_off_rounded, color: Colors.white)),
        ),
      ));
    }
    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(
          color: selected ? scheme.onSurface : scheme.outlineVariant,
          width: selected ? 2 : 1,
        ),
      ),
      clipBehavior: Clip.antiAlias,
      child: Stack(children: children),
    );
  }

  static String _speedText(double speed) {
    final text = speed.toStringAsFixed(2);
    return text.replaceFirst(RegExp(r'\.?0+$'), '');
  }

  Widget _badge(String? text, {IconData? icon}) => Container(
        margin: const EdgeInsets.only(right: 3),
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
        decoration: BoxDecoration(
          color: Colors.black.withValues(alpha: 0.55),
          borderRadius: BorderRadius.circular(4),
        ),
        child: icon != null
            ? Icon(icon, size: 11, color: Colors.white)
            : Text(
                text ?? '',
                style: const TextStyle(
                  fontSize: 10,
                  color: Colors.white,
                  fontFeatures: [FontFeature.tabularFigures()],
                ),
              ),
      );

  /// A trim handle on the selected clip: the in point at its start, the
  /// out point -- or the length of a card or photograph -- at its end.
  Widget _handle(ThemeData theme, EditClip clip, {required double left, required bool start}) {
    final canTrim = clip.isClip || !start;
    return Positioned(
      key: Key('trim-${start ? 'in' : 'out'}-${clip.id}'),
      left: left + (_trimSlot == clip.id && _trimStart == start ? _trimDx : 0),
      top: _rulerHeight + _gap,
      width: _handleWidth,
      height: _clipHeight,
      child: MouseRegion(
        cursor: SystemMouseCursors.resizeColumn,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onHorizontalDragStart: canTrim
              ? (_) => setState(() {
                    _trimSlot = clip.id;
                    _trimStart = start;
                    _trimDx = 0;
                  })
              : null,
          onHorizontalDragUpdate: canTrim ? (d) => setState(() => _trimDx += d.delta.dx) : null,
          onHorizontalDragEnd: canTrim ? (_) => _endTrim(clip) : null,
          onHorizontalDragCancel: () => setState(() => _trimSlot = null),
          child: Container(
            decoration: BoxDecoration(
              color: theme.colorScheme.onSurface,
              borderRadius: BorderRadius.horizontal(
                left: start ? const Radius.circular(6) : Radius.zero,
                right: start ? Radius.zero : const Radius.circular(6),
              ),
            ),
            child: Icon(
              start ? Icons.chevron_left_rounded : Icons.chevron_right_rounded,
              size: 14,
              color: theme.colorScheme.surface,
            ),
          ),
        ),
      ),
    );
  }

  void _endTrim(EditClip clip) {
    final dx = _trimDx;
    setState(() => _trimSlot = null);
    final deltaUs = (dx / widget.zoom * 1e6).round();
    if (deltaUs == 0) return;
    final state = widget.state;
    final source = clip.sourceId == null ? null : state.sources[clip.sourceId];
    if (!clip.isClip || source == null || source.isImage) {
      // A card or a photograph: how long it is held.
      final length = (clip.durationUs + deltaUs).clamp(state.minSlotUs, 600000000);
      widget.onDuration(clip.id, length);
      return;
    }
    final footage = (deltaUs * clip.speed).round();
    final minFootage = (state.minSlotUs * clip.speed).ceil();
    if (_trimStart) {
      final inUs = (clip.inUs + footage).clamp(0, clip.outUs - minFootage);
      if (inUs != clip.inUs) widget.onTrim(clip.id, inUs: inUs);
    } else {
      final outUs = (clip.outUs + footage).clamp(clip.inUs + minFootage, source.durationUs);
      if (outUs != clip.outUs) widget.onTrim(clip.id, outUs: outUs);
    }
  }

  /// The joins: a marker where one clip meets the next, which opens its
  /// transition.
  List<Widget> _joins(ThemeData theme) {
    final clips = widget.state.clips;
    final out = <Widget>[];
    for (var i = 0; i + 1 < clips.length; i++) {
      final join = widget.state.joinAfter(clips[i].id);
      final kind = join?.kind ?? TransitionKind.cut;
      out.add(Positioned(
        key: Key('join-${clips[i].id}'),
        left: _x(clips[i + 1].startUs) - 11,
        top: _rulerHeight + _gap + _clipHeight / 2 - 11,
        width: 22,
        height: 22,
        child: GestureDetector(
          onTap: () => widget.onJoin(clips[i].id),
          child: Container(
            decoration: BoxDecoration(
              color: kind == TransitionKind.cut
                  ? theme.colorScheme.surface
                  : theme.colorScheme.primary,
              borderRadius: BorderRadius.circular(5),
              border: Border.all(color: theme.colorScheme.outline),
            ),
            child: Icon(
              kind == TransitionKind.cut ? Icons.horizontal_rule_rounded : Icons.compare_arrows_rounded,
              size: 13,
              color: kind == TransitionKind.cut
                  ? theme.colorScheme.onSurfaceVariant
                  : theme.colorScheme.onPrimary,
            ),
          ),
        ),
      ));
    }
    return out;
  }

  Widget _bar(
    ThemeData theme, {
    required Key key,
    required int startUs,
    required int endUs,
    required double top,
    required String label,
    required IconData icon,
    required Color color,
    required bool selected,
    required VoidCallback onTap,
  }) {
    final left = _x(startUs);
    final width = math.max(6.0, _x(endUs) - left - 1);
    return Positioned(
      key: key,
      left: left,
      top: top,
      width: width,
      height: _laneHeight - 4,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            color: color.withValues(alpha: selected ? 0.95 : 0.7),
            borderRadius: BorderRadius.circular(5),
            border: selected ? Border.all(color: theme.colorScheme.onSurface, width: 2) : null,
          ),
          child: Row(children: [
            Icon(icon, size: 12, color: Colors.white),
            const SizedBox(width: 4),
            Expanded(
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 11, color: Colors.white),
              ),
            ),
          ]),
        ),
      ),
    );
  }

  List<Widget> _texts(ThemeData theme) {
    const top = _rulerHeight + _clipHeight + _gap * 2 + 2;
    return [
      for (final t in widget.state.texts)
        _bar(
          theme,
          key: Key('text-${t.id}'),
          startUs: t.startUs,
          endUs: t.endUs,
          top: top,
          label: t.content,
          icon: Icons.text_fields_rounded,
          color: const Color(0xFFB8860B),
          selected: widget.selection == EditSelection('text', t.id),
          onTap: () => widget.onSelect(EditSelection('text', t.id)),
        ),
    ];
  }

  List<Widget> _sounds(ThemeData theme) {
    const top = _rulerHeight + _clipHeight + _laneHeight + _gap * 3 + 2;
    return [
      for (final m in widget.state.sounds)
        _bar(
          theme,
          key: Key('music-${m.id}'),
          startUs: m.startUs,
          endUs: m.endUs,
          top: top,
          label: m.muted ? '${m.name} (×)' : m.name,
          icon: Icons.music_note_rounded,
          color: const Color(0xFF2E7D6B),
          selected: widget.selection == EditSelection('music', m.id),
          onTap: () => widget.onSelect(EditSelection('music', m.id)),
        ),
    ];
  }

  Widget _addButton(ThemeData theme) => Positioned(
        left: _x(widget.state.totalUs) + 10,
        top: _rulerHeight + _gap + (_clipHeight - 36) / 2,
        width: 36,
        height: 36,
        child: IconButton.filledTonal(
          key: const Key('timeline-add'),
          padding: EdgeInsets.zero,
          onPressed: widget.onAdd,
          icon: const Icon(Icons.add_rounded, size: 20),
        ),
      );
}

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

/// The editor's sheets: one per kind of change, each committing it as one
/// engine operation when a slider is let go or a choice is tapped, so every
/// step can be undone on its own.
library;

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../models.dart';
import 'edit_state.dart';
import 'picture_rules.dart';

/// Apply one `vb-edit` operation; the page sends it and refreshes.
typedef ApplyOp = Future<bool> Function(Map<String, dynamic> op);

Future<void> showEditSheet(BuildContext context, Widget Function(BuildContext) builder) =>
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      constraints: const BoxConstraints(maxWidth: 640),
      builder: (context) => SafeArea(
        child: Padding(
          padding: EdgeInsets.only(bottom: MediaQuery.of(context).viewInsets.bottom),
          child: builder(context),
        ),
      ),
    );

class _Sheet extends StatelessWidget {
  const _Sheet({required this.title, required this.children, this.trailing});
  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(children: [
            Expanded(child: Text(title, style: theme.textTheme.titleMedium)),
            if (trailing != null) trailing!,
          ]),
          const SizedBox(height: 12),
          ...children,
        ],
      ),
    );
  }
}

/// A labelled slider that commits when it is let go.
class _Dial extends StatefulWidget {
  const _Dial({
    super.key,
    required this.label,
    required this.value,
    required this.min,
    required this.max,
    required this.format,
    required this.onCommit,
    this.onSlide,
    this.divisions,
  });

  final String label;
  final double value;
  final double min;
  final double max;
  final String Function(double) format;
  final ValueChanged<double> onCommit;
  final ValueChanged<double>? onSlide;
  final int? divisions;

  @override
  State<_Dial> createState() => _DialState();
}

class _DialState extends State<_Dial> {
  late double _value = widget.value.clamp(widget.min, widget.max);

  @override
  void didUpdateWidget(_Dial old) {
    super.didUpdateWidget(old);
    if (old.value != widget.value) _value = widget.value.clamp(widget.min, widget.max);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(children: [
      SizedBox(width: 72, child: Text(widget.label, style: theme.textTheme.bodyMedium)),
      Expanded(
        child: Slider(
          value: _value,
          min: widget.min,
          max: widget.max,
          divisions: widget.divisions,
          onChanged: (v) {
            setState(() => _value = v);
            widget.onSlide?.call(v);
          },
          onChangeEnd: widget.onCommit,
        ),
      ),
      SizedBox(
        width: 64,
        child: Text(
          widget.format(_value),
          textAlign: TextAlign.end,
          style: theme.textTheme.bodySmall?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]),
        ),
      ),
    ]);
  }
}

String _seconds(double us) => '${(us / 1e6).toStringAsFixed(1)}s';

// --- a clip --------------------------------------------------------------------

/// Speed, on a scale where halving and doubling are the same distance.
Widget speedSheet(BuildContext context, EditClip clip, EditState state, ApplyOp apply) {
  final s = S.of(context);
  final logMin = math.log(state.minSpeed) / math.ln2;
  final logMax = math.log(state.maxSpeed) / math.ln2;
  return StatefulBuilder(builder: (context, setState) {
    var speed = clip.speed;
    return _Sheet(title: s.edSpeed, children: [
      Text(s.edSpeedNote, style: Theme.of(context).textTheme.bodySmall),
      const SizedBox(height: 8),
      _Dial(
        key: const Key('speed-dial'),
        label: s.edSpeed,
        value: math.log(speed) / math.ln2,
        min: logMin,
        max: logMax,
        format: (v) => '${math.pow(2, v).toStringAsFixed(2)}x',
        onCommit: (v) {
          final chosen = (math.pow(2, v) * 100).round() / 100;
          apply({'op': 'speed', 'slotId': clip.id, 'speed': chosen});
        },
      ),
      Wrap(spacing: 8, children: [
        for (final preset in const [0.5, 1.0, 1.5, 2.0, 3.0])
          ChoiceChip(
            label: Text('${preset}x'),
            selected: (clip.speed - preset).abs() < 0.01,
            onSelected: (_) => apply({'op': 'speed', 'slotId': clip.id, 'speed': preset}),
          ),
      ]),
    ]);
  });
}

Widget volumeSheet(BuildContext context, EditClip clip, EditState state, ApplyOp apply) {
  final s = S.of(context);
  return _Sheet(title: s.edVolume, children: [
    _Dial(
      label: s.edVolume,
      value: clip.gainDb,
      min: -30,
      max: 12,
      format: (v) => '${v >= 0 ? '+' : ''}${v.toStringAsFixed(0)} dB',
      onCommit: (v) => apply({'op': 'volume', 'slotId': clip.id, 'gainDb': v.roundToDouble()}),
    ),
    SwitchListTile(
      key: const Key('clip-mute'),
      contentPadding: EdgeInsets.zero,
      title: Text(s.edMute),
      value: clip.muted,
      onChanged: (v) => apply({'op': 'volume', 'slotId': clip.id, 'muted': v}),
    ),
    SwitchListTile(
      contentPadding: EdgeInsets.zero,
      title: Text(s.edMuteAll),
      value: state.allMuted,
      onChanged: (v) => apply({'op': 'muteAll', 'muted': v}),
    ),
  ]);
}

String transitionName(S s, TransitionKind kind) => switch (kind) {
      TransitionKind.cut => s.edTransCut,
      TransitionKind.xfade => s.edTransXfade,
      TransitionKind.fadeToBlack => s.edTransFade,
      TransitionKind.wipeLeft => s.edTransWipeLeft,
      TransitionKind.wipeRight => s.edTransWipeRight,
    };

/// The join after `fromSlotId`, or every join when it is null.
Widget transitionSheet(BuildContext context, EditState state, String? fromSlotId, ApplyOp apply) {
  final s = S.of(context);
  final join = fromSlotId == null ? null : state.joinAfter(fromSlotId);
  var kind = join?.kind ?? TransitionKind.cut;
  var lengthUs = (join?.durationUs ?? 0) > 0 ? join!.durationUs.toDouble() : 500000.0;
  return StatefulBuilder(builder: (context, setState) {
    Map<String, dynamic> op(TransitionKind k, double us, {bool all = false}) => {
          'op': all || fromSlotId == null ? 'transitionAll' : 'transition',
          if (!all && fromSlotId != null) 'fromSlotId': fromSlotId,
          'kind': k.id,
          'durationUs': k == TransitionKind.cut ? 0 : us.round(),
        };
    return _Sheet(
      title: fromSlotId == null ? s.edTransitionAll : s.edTransition,
      children: [
        Wrap(spacing: 8, runSpacing: 8, children: [
          for (final k in TransitionKind.values)
            ChoiceChip(
              key: Key('transition-${k.id}'),
              label: Text(transitionName(s, k)),
              selected: kind == k,
              onSelected: (_) {
                setState(() => kind = k);
                apply(op(k, lengthUs));
              },
            ),
        ]),
        const SizedBox(height: 8),
        if (kind != TransitionKind.cut)
          _Dial(
            label: s.edLength,
            value: lengthUs,
            min: 100000,
            max: 2000000,
            divisions: 19,
            format: _seconds,
            onCommit: (v) {
              setState(() => lengthUs = v);
              apply(op(kind, v));
            },
          ),
        if (fromSlotId != null)
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton.icon(
              key: const Key('transition-all'),
              icon: const Icon(Icons.done_all_rounded, size: 18),
              label: Text(s.edApplyToAll),
              onPressed: () => apply(op(kind, lengthUs, all: true)),
            ),
          ),
      ],
    );
  });
}

Widget adjustSheet(
  BuildContext context,
  EditClip clip,
  ApplyOp apply, {
  required ValueChanged<Adjustment?> onPreview,
}) {
  final s = S.of(context);
  var a = clip.adjustment;
  return StatefulBuilder(builder: (context, setState) {
    void commit(Adjustment next, {bool all = false}) {
      setState(() => a = next);
      onPreview(null);
      apply({
        'op': 'adjust',
        'slotId': clip.id,
        'brightness': next.brightness,
        'contrast': next.contrast,
        'saturation': next.saturation,
        if (all) 'all': true,
      });
    }

    String pct(double v) => '${v >= 0 ? '+' : ''}${(v * 100).round()}';
    return _Sheet(
      title: s.edAdjust,
      trailing: TextButton(
        onPressed: a.isNeutral ? null : () => commit(const Adjustment()),
        child: Text(s.edReset),
      ),
      children: [
        _Dial(
          label: s.edBrightness,
          value: a.brightness,
          min: -0.5,
          max: 0.5,
          format: pct,
          onSlide: (v) => onPreview(Adjustment(brightness: v, contrast: a.contrast, saturation: a.saturation)),
          onCommit: (v) => commit(Adjustment(brightness: v, contrast: a.contrast, saturation: a.saturation)),
        ),
        _Dial(
          label: s.edContrast,
          value: a.contrast,
          min: 0.5,
          max: 1.5,
          format: (v) => pct(v - 1),
          onSlide: (v) => onPreview(Adjustment(brightness: a.brightness, contrast: v, saturation: a.saturation)),
          onCommit: (v) => commit(Adjustment(brightness: a.brightness, contrast: v, saturation: a.saturation)),
        ),
        _Dial(
          label: s.edSaturation,
          value: a.saturation,
          min: 0,
          max: 2,
          format: (v) => pct(v - 1),
          onSlide: (v) => onPreview(Adjustment(brightness: a.brightness, contrast: a.contrast, saturation: v)),
          onCommit: (v) => commit(Adjustment(brightness: a.brightness, contrast: a.contrast, saturation: v)),
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            icon: const Icon(Icons.done_all_rounded, size: 18),
            label: Text(s.edApplyToAll),
            onPressed: () => commit(a, all: true),
          ),
        ),
      ],
    );
  });
}

Widget positionSheet(BuildContext context, EditClip clip, ApplyOp apply) {
  final s = S.of(context);
  var p = clip.placement;
  return StatefulBuilder(builder: (context, setState) {
    void commit(Placement next) {
      setState(() => p = next);
      apply({
        'op': 'transform',
        'slotId': clip.id,
        'scale': next.scale,
        'x': next.x,
        'y': next.y,
        'rotationDeg': next.rotationDeg,
      });
    }

    return _Sheet(
      title: s.edPosition,
      trailing: TextButton(
        onPressed: p.isFitted
            ? null
            : () {
                setState(() => p = Placement.fitted);
                apply({'op': 'resetTransform', 'slotId': clip.id});
              },
        child: Text(s.edReset),
      ),
      children: [
        _Dial(
          label: s.edScale,
          value: math.log(p.scale) / math.ln2,
          min: -1,
          max: 2,
          format: (v) => '${(math.pow(2, v) * 100).round()}%',
          onCommit: (v) => commit(Placement(
            scale: (math.pow(2, v) * 100).round() / 100,
            x: p.x,
            y: p.y,
            rotationDeg: p.rotationDeg,
          )),
        ),
        _Dial(
          label: s.edMoveX,
          value: p.x,
          min: -0.5,
          max: 0.5,
          format: (v) => '${(v * 100).round()}%',
          onCommit: (v) => commit(Placement(scale: p.scale, x: (v * 1000).round() / 1000, y: p.y, rotationDeg: p.rotationDeg)),
        ),
        _Dial(
          label: s.edMoveY,
          value: p.y,
          min: -0.5,
          max: 0.5,
          format: (v) => '${(v * 100).round()}%',
          onCommit: (v) => commit(Placement(scale: p.scale, x: p.x, y: (v * 1000).round() / 1000, rotationDeg: p.rotationDeg)),
        ),
        _Dial(
          label: s.edRotate,
          value: p.rotationDeg,
          min: -180,
          max: 180,
          format: (v) => '${v.round()}°',
          onCommit: (v) => commit(Placement(scale: p.scale, x: p.x, y: p.y, rotationDeg: v.roundToDouble())),
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            icon: const Icon(Icons.rotate_90_degrees_cw_rounded, size: 18),
            label: Text(s.edRotate90),
            onPressed: () {
              var next = p.rotationDeg + 90;
              if (next > 180) next -= 360;
              commit(Placement(scale: p.scale, x: p.x, y: p.y, rotationDeg: next));
            },
          ),
        ),
      ],
    );
  });
}

/// How long a title card, a gap or a photograph is held.
Widget durationSheet(BuildContext context, EditClip clip, EditState state, ApplyOp apply) {
  final s = S.of(context);
  return _Sheet(title: s.edDuration, children: [
    _Dial(
      label: s.edDuration,
      value: clip.durationUs.toDouble(),
      min: math.max(state.minSlotUs, 300000).toDouble(),
      max: math.max(10000000, clip.durationUs).toDouble(),
      format: _seconds,
      onCommit: (v) => apply({'op': 'setDuration', 'slotId': clip.id, 'durationUs': (v / 100000).round() * 100000}),
    ),
  ]);
}

// --- the canvas -----------------------------------------------------------------

/// The ratios the canvas offers, and the footage's own.
const canvasRatios = ['16:9', '9:16', '1:1', '4:3', '3:4', '21:9'];

/// The first video's shape as a ratio the engine takes (both terms at most
/// 64), or the nearest common one.
String nativeRatio(EditState state) {
  for (final c in state.clips) {
    final src = c.sourceId == null ? null : state.sources[c.sourceId];
    if (src == null || src.isImage || src.width == 0 || src.height == 0) continue;
    int gcd(int a, int b) => b == 0 ? a : gcd(b, a % b);
    final g = gcd(src.width, src.height);
    final w = src.width ~/ g, h = src.height ~/ g;
    if (w <= 64 && h <= 64) return '$w:$h';
    final r = src.width / src.height;
    String best = '16:9';
    var err = double.infinity;
    for (final ratio in canvasRatios) {
      final parts = ratio.split(':').map(int.parse).toList();
      final e = (parts[0] / parts[1] - r).abs();
      if (e < err) {
        err = e;
        best = ratio;
      }
    }
    return best;
  }
  return '16:9';
}

const _backdropColors = ['#FFFFFF', '#F2E8DA', '#2B2B2B', '#1F3A5F', '#4A6B3F', '#8C2F39'];

Widget canvasSheet(BuildContext context, EditState state, EditClip? clip, ApplyOp apply) {
  final s = S.of(context);
  final native = nativeRatio(state);
  final current = state.aspectRatio ?? native;
  return _Sheet(title: s.edCanvas, children: [
    Text(s.edRatio, style: Theme.of(context).textTheme.labelLarge),
    const SizedBox(height: 6),
    Wrap(spacing: 8, runSpacing: 8, children: [
      ChoiceChip(
        label: Text('${s.edRatioOriginal} $native'),
        selected: current == native && state.aspectRatio == null,
        onSelected: (_) => apply({'op': 'canvas', 'aspectRatio': native}),
      ),
      for (final r in canvasRatios)
        ChoiceChip(
          key: Key('ratio-$r'),
          label: Text(r),
          selected: state.aspectRatio == r,
          onSelected: (_) => apply({'op': 'canvas', 'aspectRatio': r}),
        ),
    ]),
    const SizedBox(height: 16),
    Text(clip == null ? s.edBackgroundAll : s.edBackground, style: Theme.of(context).textTheme.labelLarge),
    const SizedBox(height: 6),
    _BackdropChoice(
      current: clip?.backdrop ?? Backdrop.black,
      onChosen: (b) => apply({
        'op': 'background',
        if (clip != null) 'slotId': clip.id,
        'background': b.kind == 'black' ? null : {'kind': b.kind, if (b.color != null) 'color': b.color},
      }),
    ),
    if (clip != null)
      Align(
        alignment: Alignment.centerLeft,
        child: TextButton.icon(
          icon: const Icon(Icons.done_all_rounded, size: 18),
          label: Text(s.edApplyToAll),
          onPressed: () {
            final b = clip.backdrop ?? Backdrop.black;
            apply({
              'op': 'background',
              'background': b.kind == 'black' ? null : {'kind': b.kind, if (b.color != null) 'color': b.color},
            });
          },
        ),
      ),
  ]);
}

class _BackdropChoice extends StatelessWidget {
  const _BackdropChoice({required this.current, required this.onChosen});
  final Backdrop current;
  final ValueChanged<Backdrop> onChosen;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    Widget swatch({required Key key, required Widget child, required bool selected, required VoidCallback onTap, Color? color}) =>
        InkWell(
          key: key,
          onTap: onTap,
          borderRadius: BorderRadius.circular(8),
          child: Container(
            width: 52,
            height: 40,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: color ?? theme.colorScheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(
                color: selected ? theme.colorScheme.primary : theme.colorScheme.outlineVariant,
                width: selected ? 2.5 : 1,
              ),
            ),
            child: child,
          ),
        );
    return Wrap(spacing: 8, runSpacing: 8, children: [
      swatch(
        key: const Key('backdrop-black'),
        color: Colors.black,
        selected: current.kind == 'black',
        onTap: () => onChosen(Backdrop.black),
        child: Text(s.edBgBlack, style: const TextStyle(color: Colors.white, fontSize: 11)),
      ),
      swatch(
        key: const Key('backdrop-blur'),
        selected: current.kind == 'blur',
        onTap: () => onChosen(const Backdrop('blur')),
        child: Text(s.edBgBlur, style: theme.textTheme.labelSmall),
      ),
      for (final c in _backdropColors)
        swatch(
          key: Key('backdrop-$c'),
          color: parseColor(c),
          selected: current.kind == 'color' && current.color?.toUpperCase() == c,
          onTap: () => onChosen(Backdrop('color', c)),
          child: const SizedBox.shrink(),
        ),
    ]);
  }
}

// --- text -----------------------------------------------------------------------

const _textColors = ['#FFFFFF', '#000000', '#FFE066', '#FF6B6B', '#4DABF7', '#69DB7C', '#F783AC'];

/// A text's words, look and length. New words are committed with the
/// button; the look commits as it is chosen.
class TextSheet extends StatefulWidget {
  const TextSheet({super.key, required this.text, required this.state, required this.apply});
  final EditText text;
  final EditState state;
  final ApplyOp apply;

  @override
  State<TextSheet> createState() => _TextSheetState();
}

class _TextSheetState extends State<TextSheet> {
  late final TextEditingController _words = TextEditingController(text: widget.text.content);
  late TextLook _look = widget.text.style;

  @override
  void dispose() {
    _words.dispose();
    super.dispose();
  }

  void _style(TextLook next) {
    setState(() => _look = next);
    widget.apply({'op': 'updateText', 'id': widget.text.id, 'style': next.toJson()});
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    return _Sheet(title: s.edText, children: [
      TextField(
        key: const Key('text-words'),
        controller: _words,
        autofocus: widget.text.content.isEmpty,
        minLines: 1,
        maxLines: 4,
        decoration: InputDecoration(hintText: s.edTextHint),
      ),
      const SizedBox(height: 8),
      Align(
        alignment: Alignment.centerRight,
        child: FilledButton(
          key: const Key('text-save'),
          onPressed: () {
            final words = _words.text.trim();
            if (words.isEmpty || words == widget.text.content) {
              Navigator.of(context).pop();
              return;
            }
            widget.apply({'op': 'updateText', 'id': widget.text.id, 'content': words});
            Navigator.of(context).pop();
          },
          child: Text(s.edDone),
        ),
      ),
      const SizedBox(height: 8),
      _Dial(
        label: s.edFontSize,
        value: _look.fontSizePt,
        min: 14,
        max: 120,
        format: (v) => v.round().toString(),
        onCommit: (v) => _style(_look.copyWith(fontSizePt: v.roundToDouble())),
      ),
      Wrap(spacing: 8, runSpacing: 8, children: [
        for (final c in _textColors)
          InkWell(
            key: Key('text-color-$c'),
            onTap: () => _style(_look.copyWith(color: c)),
            customBorder: const CircleBorder(),
            child: Container(
              width: 30,
              height: 30,
              decoration: BoxDecoration(
                color: parseColor(c),
                shape: BoxShape.circle,
                border: Border.all(
                  color: _look.color.toUpperCase() == c ? theme.colorScheme.primary : theme.colorScheme.outlineVariant,
                  width: _look.color.toUpperCase() == c ? 3 : 1,
                ),
              ),
            ),
          ),
      ]),
      const SizedBox(height: 8),
      Wrap(spacing: 8, runSpacing: 8, children: [
        FilterChip(
          label: Text(s.edBold),
          selected: _look.bold,
          onSelected: (v) => _style(_look.copyWith(bold: v)),
        ),
        FilterChip(
          label: Text(s.edOutline),
          selected: _look.outlineWidth > 0,
          onSelected: (v) => _style(_look.copyWith(outlineWidth: v ? 1.5 : 0)),
        ),
        FilterChip(
          key: const Key('text-box'),
          label: Text(s.edTextBox),
          selected: _look.backgroundColor != null,
          onSelected: (v) => _style(v ? _look.copyWith(backgroundColor: '#00000099') : _look.copyWith(clearBackground: true)),
        ),
      ]),
      const SizedBox(height: 12),
      Text(s.edPlace, style: theme.textTheme.labelLarge),
      const SizedBox(height: 6),
      Wrap(spacing: 8, children: [
        for (final (label, alignment) in [
          (s.edPlaceTop, 'topCenter'),
          (s.edPlaceMiddle, 'middleCenter'),
          (s.edPlaceBottom, 'bottomCenter'),
        ])
          ChoiceChip(
            label: Text(label),
            selected: _look.x == null && _look.alignment == alignment,
            onSelected: (_) => _style(_look.copyWith(alignment: alignment, clearPlace: true)),
          ),
      ]),
      Text(s.edDragTextHint, style: theme.textTheme.bodySmall),
      const SizedBox(height: 8),
      _Dial(
        label: s.edDuration,
        value: widget.text.durationUs.toDouble(),
        min: widget.state.minTextUs.toDouble(),
        max: math.max(15000000, widget.text.durationUs).toDouble(),
        format: _seconds,
        onCommit: (v) => widget.apply({
          'op': 'updateText',
          'id': widget.text.id,
          'durationUs': (v / 100000).round() * 100000,
        }),
      ),
    ]);
  }
}

// --- music ----------------------------------------------------------------------

Widget musicSheet(BuildContext context, EditSound sound, ApplyOp apply, {required VoidCallback onReplace}) {
  final s = S.of(context);
  return _Sheet(title: '${s.edMusic} · ${sound.name}', children: [
    _Dial(
      label: s.edVolume,
      value: sound.gainDb,
      min: -30,
      max: 6,
      format: (v) => '${v >= 0 ? '+' : ''}${v.toStringAsFixed(0)} dB',
      onCommit: (v) => apply({'op': 'updateMusic', 'id': sound.id, 'gainDb': v.roundToDouble()}),
    ),
    SwitchListTile(
      contentPadding: EdgeInsets.zero,
      title: Text(s.edMute),
      value: sound.muted,
      onChanged: (v) => apply({'op': 'updateMusic', 'id': sound.id, 'muted': v}),
    ),
    _Dial(
      label: s.edFadeIn,
      value: sound.fadeInUs.toDouble(),
      min: 0,
      max: 5000000,
      divisions: 50,
      format: _seconds,
      onCommit: (v) => apply({'op': 'updateMusic', 'id': sound.id, 'fadeInUs': v.round()}),
    ),
    _Dial(
      label: s.edFadeOut,
      value: sound.fadeOutUs.toDouble(),
      min: 0,
      max: 5000000,
      divisions: 50,
      format: _seconds,
      onCommit: (v) => apply({'op': 'updateMusic', 'id': sound.id, 'fadeOutUs': v.round()}),
    ),
    Row(children: [
      TextButton.icon(
        icon: const Icon(Icons.swap_horiz_rounded, size: 18),
        label: Text(s.edReplaceMusic),
        onPressed: () {
          Navigator.of(context).pop();
          onReplace();
        },
      ),
      const Spacer(),
      TextButton.icon(
        key: const Key('music-delete'),
        icon: const Icon(Icons.delete_outline_rounded, size: 18),
        label: Text(s.edRemoveMusic),
        onPressed: () {
          Navigator.of(context).pop();
          apply({'op': 'deleteMusic', 'id': sound.id});
        },
      ),
    ]),
  ]);
}

// --- footage --------------------------------------------------------------------

/// The project's footage, to add a stretch of after the clip at the
/// playhead.
class FootageSheet extends StatelessWidget {
  const FootageSheet({
    super.key,
    required this.files,
    required this.pictures,
    required this.onChosen,
  });

  final List<FootageFile> files;

  /// A picture of each file, by source id.
  final Map<String, Uint8List> pictures;
  final void Function(FootageFile file, FootageStretch stretch) onChosen;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final theme = Theme.of(context);
    return _Sheet(title: s.edAddClip, children: [
      Text(s.edFootageNote, style: theme.textTheme.bodySmall),
      const SizedBox(height: 8),
      for (final file in files)
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(6),
              child: SizedBox(
                width: 96,
                height: 54,
                child: pictures[file.source.id] == null
                    ? ColoredBox(color: theme.colorScheme.surfaceContainerHigh)
                    : Image.memory(pictures[file.source.id]!, fit: BoxFit.cover),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Text(file.source.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodyMedium),
                if (file.used.isNotEmpty)
                  Text(s.edUsedTimes(file.used.length), style: theme.textTheme.bodySmall),
                const SizedBox(height: 4),
                if (file.stretches.isEmpty)
                  Text(s.edNoStretches, style: theme.textTheme.bodySmall)
                else
                  Wrap(spacing: 6, runSpacing: 6, children: [
                    for (final stretch in file.stretches)
                      ActionChip(
                        key: Key('stretch-${file.source.id}-${stretch.startUs}'),
                        avatar: const Icon(Icons.add_rounded, size: 16),
                        label: Text(
                          '${formatDuration(stretch.startUs)} · ${_seconds((stretch.endUs - stretch.startUs).toDouble())}'
                          '${stretch.label == null ? '' : ' · ${stretch.label}'}',
                          overflow: TextOverflow.ellipsis,
                        ),
                        onPressed: () => onChosen(file, stretch),
                      ),
                  ]),
              ]),
            ),
          ]),
        ),
    ]);
  }
}

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
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../api_models.dart';
import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import '../theme.dart' show monoFallback, monoFamily;
import 'memory_page.dart';

/// The memory map: every trip, child growing up, year and group the butler
/// found, as signals on a dark world the user can pan and zoom, the regions
/// they were filmed in lit in their colour.
///
/// It looks like a monitor -- a near-black sea, dark land with lit coasts, a
/// faint grid, glowing points and readouts in a monospace face -- after
/// World Monitor's maps. Only the look is borrowed: that project is
/// AGPL-3.0, and none of its code is here.
///
/// Drawn from the engine's own outlines (`geo.world`), so the province a
/// trip is named after is the one that lights up, and nothing is fetched:
/// the map works with no network, like the rest of the app.
class MemoryMapView extends StatefulWidget {
  const MemoryMapView({super.key, required this.engine});

  final Engine engine;

  @override
  State<MemoryMapView> createState() => _MemoryMapViewState();
}

/// The world, parsed and turned into paths once per run of the app.
WorldMap? _world;
_WorldPaths? _paths;

MemoryKindId kindId(MemoryKind kind) => switch (kind) {
      MemoryKind.travel => MemoryKindId.travel,
      MemoryKind.growth => MemoryKindId.growth,
      MemoryKind.annual => MemoryKindId.annual,
      MemoryKind.group => MemoryKindId.group,
    };

/// Each kind's signal colour: bright enough to glow on the dark map, and
/// apart enough to tell at a glance.
Color kindColor(MemoryKind kind) => switch (kind) {
      MemoryKind.travel => const Color(0xFFFF6B3D),
      MemoryKind.growth => const Color(0xFF3DDC97),
      MemoryKind.annual => const Color(0xFF4D9DFF),
      MemoryKind.group => const Color(0xFFFFC23D),
    };

/// Memories of more than one kind at one place.
const Color _mixed = Color(0xFFB98CFF);

IconData kindIcon(MemoryKind kind) => switch (kind) {
      MemoryKind.travel => Icons.flight_takeoff_rounded,
      MemoryKind.growth => Icons.child_care_rounded,
      MemoryKind.annual => Icons.calendar_month_rounded,
      MemoryKind.group => Icons.groups_rounded,
    };

/// Which kind lights a region shared by several: the most particular. A
/// year lights every place it was filmed, a trip only where it went.
int _precedence(MemoryKind kind) => switch (kind) {
      MemoryKind.annual => 0,
      MemoryKind.growth => 1,
      MemoryKind.group => 2,
      MemoryKind.travel => 3,
    };

/// The map's own palette. A monitor's, dark whatever the app's theme is:
/// lights read best on a night sea.
abstract final class _Ink {
  static const sea = Color(0xFF081520);
  static const seaEdge = Color(0xFF020509);
  static const gridMinor = Color(0x1C3A8C9C);
  static const gridMajor = Color(0x333A8C9C);
  static const land = Color(0xFF0D1C23);
  static const province = Color(0xFF10222A);
  static const provinceLine = Color(0xFF1A3843);
  static const border = Color(0xFF26535E);
  static const coast = Color(0xFF3C8E9C);
  static const panel = Color(0xE6091218);
  static const panelLine = Color(0xFF1D3239);
  static const text = Color(0xFFE2EDF0);
  static const dim = Color(0xFF7F98A0);
  static const faint = Color(0xFF4C646B);
}

/// The monospace faces have no Chinese; these do, so a readout in Chinese
/// does not lose its words.
const _cjk = <String>['PingFang SC', 'Hiragino Sans GB', 'Noto Sans CJK SC', 'Microsoft YaHei'];

TextStyle _mono(double size, Color color, {FontWeight weight = FontWeight.w500}) => TextStyle(
      fontFamily: monoFamily,
      fontFamilyFallback: const [...monoFallback, ..._cjk],
      fontSize: size,
      fontWeight: weight,
      color: color,
      letterSpacing: 0.4,
      height: 1.2,
      fontFeatures: const [FontFeature.tabularFigures()],
    );

// --- the projection ----------------------------------------------------------

/// Miller's cylindrical projection, cut at 56 S and 80 N: the familiar
/// shape of a world map, without the poles' sprawl, and regions that keep
/// their shape when zoomed into.
const double _south = -56;
const double _north = 80;

double _millerY(double lat) {
  final phi = lat.clamp(-85.0, 85.0) * math.pi / 180;
  return 1.25 * math.log(math.tan(math.pi / 4 + 0.4 * phi));
}

final double _top = _millerY(_north);
final double _bottom = _millerY(_south);

/// Width over height of the map.
final double mapAspect = 2 * math.pi / (_top - _bottom);

/// Where a point falls on a map one unit wide and one unit tall.
Offset project(double lat, double lon) =>
    Offset((lon + 180) / 360, (_top - _millerY(lat)) / (_top - _bottom));

/// The latitude and longitude at a point of the unit map.
(double, double) unproject(Offset unit) {
  final y = _top - unit.dy * (_top - _bottom);
  final phi = (math.atan(math.exp(y / 1.25)) - math.pi / 4) / 0.4;
  return (phi * 180 / math.pi, unit.dx * 360 - 180);
}

/// A unit point in the painter's space, which keeps the map's proportions
/// so one scale fits it: as wide as the map's aspect, one tall. A circle
/// drawn there is a circle.
Offset _square(Offset unit) => Offset(unit.dx * mapAspect, unit.dy);

class _WorldPaths {
  _WorldPaths(WorldMap world) {
    for (final region in world.regions) {
      final path = Path()..fillType = PathFillType.nonZero;
      for (final ring in region.rings) {
        for (var i = 0; i + 1 < ring.length; i += 2) {
          final p = _square(project(ring[i + 1], ring[i]));
          if (i == 0) {
            path.moveTo(p.dx, p.dy);
          } else {
            path.lineTo(p.dx, p.dy);
          }
        }
        path.close();
      }
      (region.province ? provinces : countries)[region.id] = path;
      if (region.parent != null) parentOf[region.id] = region.parent!;
    }
    for (final path in countries.values) {
      coasts.addPath(path, Offset.zero);
    }
  }

  final Map<String, Path> countries = {};
  final Map<String, Path> provinces = {};
  final Map<String, String> parentOf = {};

  /// Every country's outline in one path, to light the coasts in one go.
  final Path coasts = Path();
}

/// Zoom in steps of a half power of two: the map is drawn again only when
/// the zoom crosses one, so panning and zooming move a finished picture, and
/// its lines stay within a few tenths of their width.
int _zoomStep(double scale) => (math.log(scale) / math.ln2 * 2).round();

class _MemoryMapViewState extends State<MemoryMapView> with TickerProviderStateMixin {
  MemoryList? _list;
  String? _error;
  MemoryKind? _filter;
  final TransformationController _transform = TransformationController();
  final ValueNotifier<int> _step = ValueNotifier<int>(0);
  final Map<String, Uint8List> _covers = {};
  late final AnimationController _zoomAnimation =
      AnimationController(vsync: this, duration: const Duration(milliseconds: 320));
  late final AnimationController _pulse =
      AnimationController(vsync: this, duration: const Duration(milliseconds: 2400));
  Matrix4Tween? _zoomTween;
  bool? _layersOpen;

  @override
  void initState() {
    super.initState();
    _zoomAnimation.addListener(() {
      final tween = _zoomTween;
      if (tween != null) {
        _transform.value =
            tween.evaluate(CurvedAnimation(parent: _zoomAnimation, curve: Curves.easeOutCubic));
      }
    });
    _transform.addListener(() {
      _step.value = _zoomStep(_transform.value.getMaxScaleOnAxis());
      _ping();
    });
    unawaited(_load());
  }

  /// Set the signals pulsing for a few beats: when they appear, and when the
  /// map moves. Not for ever -- a map left open would keep the screen busy.
  void _ping() {
    if (!_pulse.isAnimating) unawaited(_pulse.repeat(count: 6));
  }

  @override
  void dispose() {
    _transform.dispose();
    _step.dispose();
    _zoomAnimation.dispose();
    _pulse.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      if (_world == null) {
        _world = await widget.engine.world();
        _paths = _WorldPaths(_world!);
      }
      final list = await widget.engine.memories();
      if (!mounted) return;
      setState(() {
        _list = list;
        _error = null;
      });
      _ping();
      unawaited(_loadCovers(list));
    } on EngineException catch (e) {
      if (mounted) setState(() => _error = e.message);
    }
  }

  Future<void> _loadCovers(MemoryList list) async {
    final paths = [
      for (final m in list.memories)
        if (m.cover != null && !_covers.containsKey(m.cover)) m.cover!,
    ];
    for (var i = 0; i < paths.length; i += 6) {
      try {
        final got = await widget.engine
            .thumbnails(paths.sublist(i, (i + 6).clamp(0, paths.length)), width: 200);
        if (!mounted) return;
        setState(() => _covers.addAll(got));
      } on EngineException {
        return;
      }
    }
  }

  List<Memory> get _shown => [
        for (final m in _list?.memories ?? const <Memory>[])
          if (_filter == null || m.kind == _filter) m,
      ];

  Future<void> _open(Memory memory) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => MemoryPage(engine: widget.engine, memoryId: memory.id, title: memory.title),
      ),
    );
    await _load();
  }

  /// The last frame's layout, for moving the view.
  Size _size = Size.zero;
  Rect _mapRect = Rect.zero;

  /// Whether the view has been framed on the memories yet: on a phone held
  /// upright the whole world is a thin band, so it opens on where they are.
  bool _framed = false;

  Offset _local(GeoPoint p) {
    final unit = project(p.lat, p.lon);
    return Offset(_mapRect.left + unit.dx * _mapRect.width, _mapRect.top + unit.dy * _mapRect.height);
  }

  /// Show [memories]' places: their pins and their videos, framed with a
  /// margin, as close as eight times; the whole world when there are none.
  void _fitTo(List<Memory> memories) {
    final points = [
      for (final m in memories) ...[
        if (m.pin != null) m.pin!,
        ...m.points,
      ],
    ];
    Matrix4 next;
    if (points.isEmpty || _size.isEmpty) {
      next = Matrix4.identity();
    } else {
      var box = Rect.fromPoints(_local(points.first), _local(points.first));
      for (final p in points) {
        box = box.expandToInclude(Rect.fromCenter(center: _local(p), width: 0, height: 0));
      }
      // Room for the tags above their points, the panels and the cards.
      box = box.inflate(48);
      final view = Rect.fromLTRB(0, 90, _size.width, _size.height - 170);
      final scale = math.min(view.width / box.width, view.height / box.height).clamp(1.0, 8.0);
      final dx = view.center.dx - box.center.dx * scale;
      final dy = view.center.dy - box.center.dy * scale;
      next = Matrix4.identity()
        ..translateByDouble(dx, dy, 0, 1)
        ..scaleByDouble(scale, scale, 1, 1);
    }
    _zoomTween = Matrix4Tween(begin: _transform.value, end: next);
    _zoomAnimation.forward(from: 0);
  }

  void _zoom(double factor, Size size) {
    final centre = Offset(size.width / 2, size.height / 2);
    final current = _transform.value;
    final scale = current.getMaxScaleOnAxis();
    final target = (scale * factor).clamp(1.0, 16.0) / scale;
    final next = Matrix4.identity()
      ..translateByDouble(centre.dx, centre.dy, 0, 1)
      ..scaleByDouble(target, target, 1, 1)
      ..translateByDouble(-centre.dx, -centre.dy, 0, 1)
      ..multiply(current);
    _zoomTween = Matrix4Tween(begin: current, end: next);
    _zoomAnimation.forward(from: 0);
  }

  /// The colour each region is lit in.
  Map<String, Color> _lit(List<Memory> shown) {
    final byPrecedence = [...shown]
      ..sort((a, b) => _precedence(a.kind).compareTo(_precedence(b.kind)));
    return {
      for (final m in byPrecedence)
        for (final region in m.regions) region: kindColor(m.kind),
    };
  }

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final list = _list;
    final shown = _shown;
    final lit = _lit(shown);
    return LayoutBuilder(builder: (context, constraints) {
      final size = Size(constraints.maxWidth, constraints.maxHeight);
      final mapWidth = math.min(size.width, size.height * mapAspect);
      final mapRect = Rect.fromCenter(
        center: Offset(size.width / 2, size.height / 2 - 30),
        width: mapWidth,
        height: mapWidth / mapAspect,
      );
      _size = size;
      _mapRect = mapRect;
      if (!_framed && list != null && size.height > size.width) {
        _framed = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _fitTo(_shown);
        });
      }
      final wide = size.width >= 700;
      final layersOpen = _layersOpen ?? wide;
      final places = _placeCount(shown);
      return ClipRect(
        child: Stack(
          children: [
            const Positioned.fill(child: ColoredBox(color: _Ink.seaEdge)),
            Positioned.fill(
              child: InteractiveViewer(
                transformationController: _transform,
                minScale: 1,
                maxScale: 16,
                boundaryMargin: const EdgeInsets.all(80),
                child: SizedBox(
                  width: size.width,
                  height: size.height,
                  child: _paths == null
                      ? const SizedBox.shrink()
                      : RepaintBoundary(
                          child: CustomPaint(
                            painter: _MapPainter(
                              paths: _paths!,
                              rect: mapRect,
                              lit: lit,
                              points: _filter == null
                                  ? const []
                                  : [for (final m in shown) ...m.points],
                              dotColor: _filter == null ? null : kindColor(_filter!),
                              step: _step,
                            ),
                          ),
                        ),
                ),
              ),
            ),
            // The monitor's glass: scan lines and corner marks, still while
            // the map moves under them.
            const Positioned.fill(
              child: IgnorePointer(
                child: RepaintBoundary(child: CustomPaint(painter: _GlassPainter())),
              ),
            ),
            // Signals on top, the same size at every zoom.
            if (list != null)
              AnimatedBuilder(
                animation: _transform,
                builder: (context, _) => Stack(children: _signals(shown, mapRect)),
              ),
            Positioned(
              top: 14,
              left: 16,
              child: _Title(
                stats: list == null ? null : s.mapStats(shown.length, places, lit.length),
              ),
            ),
            Positioned(
              top: 14,
              right: 14,
              child: _Layers(
                open: layersOpen,
                filter: _filter,
                counts: {
                  for (final k in MemoryKind.values)
                    k: (list?.memories ?? const []).where((m) => m.kind == k).length,
                },
                total: list?.memories.length ?? 0,
                onToggle: () => setState(() => _layersOpen = !layersOpen),
                onChanged: (k) {
                  setState(() => _filter = k);
                  _fitTo(k == null ? const [] : _shown);
                },
              ),
            ),
            Positioned(
              right: 14,
              bottom: 170,
              child: Column(
                children: [
                  _MapButton(
                    key: const Key('map-zoom-in'),
                    icon: Icons.add_rounded,
                    tooltip: '+',
                    onTap: () => _zoom(2, size),
                  ),
                  const SizedBox(height: 6),
                  _MapButton(
                    key: const Key('map-zoom-out'),
                    icon: Icons.remove_rounded,
                    tooltip: '−',
                    onTap: () => _zoom(0.5, size),
                  ),
                  const SizedBox(height: 6),
                  _MapButton(
                    key: const Key('map-fit'),
                    icon: Icons.center_focus_strong_rounded,
                    tooltip: s.mapResetView,
                    onTap: () => _fitTo(shown),
                  ),
                ],
              ),
            ),
            Positioned(
              left: 16,
              bottom: 150,
              child: AnimatedBuilder(
                animation: _transform,
                builder: (context, _) => _Readout(
                  text: _readout(size, mapRect),
                  attribution: s.mapAttribution,
                ),
              ),
            ),
            if (_error != null)
              Center(child: _Notice(text: _error!))
            else if (list == null)
              const Center(child: CircularProgressIndicator(color: _Ink.coast))
            else if (list.memories.isEmpty)
              Center(child: _Notice(text: s.mapEmpty)),
            Positioned(
              left: 0,
              right: 0,
              bottom: 0,
              child: _MemoryStrip(
                memories: shown,
                covers: _covers,
                onOpen: (m) => unawaited(_open(m)),
              ),
            ),
          ],
        ),
      );
    });
  }

  /// How many places the memories shown were pinned at: pins within 50 km
  /// of each other are one place, whatever the screen.
  int _placeCount(List<Memory> memories) {
    final places = <GeoPoint>[];
    for (final m in memories) {
      final p = m.pin;
      if (p == null) continue;
      final near = places.any((q) {
        final dx = (p.lon - q.lon) * math.cos((p.lat + q.lat) / 2 * math.pi / 180);
        final dy = p.lat - q.lat;
        return math.sqrt(dx * dx + dy * dy) * 111.2 < 50;
      });
      if (!near) places.add(p);
    }
    return places.length;
  }

  /// Where the middle of the view is, and how close: a monitor's readout.
  String _readout(Size size, Rect mapRect) {
    final zoom = _transform.value.getMaxScaleOnAxis();
    final inverse = Matrix4.tryInvert(_transform.value);
    var text = '×${zoom.toStringAsFixed(1)}';
    if (inverse != null && !mapRect.isEmpty) {
      final local = MatrixUtils.transformPoint(inverse, Offset(size.width / 2, size.height / 2));
      final unit = Offset(
        (local.dx - mapRect.left) / mapRect.width,
        (local.dy - mapRect.top) / mapRect.height,
      );
      if (unit.dx >= 0 && unit.dx <= 1 && unit.dy >= 0 && unit.dy <= 1) {
        final (lat, lon) = unproject(unit);
        final ns = lat >= 0 ? 'N' : 'S';
        final ew = lon >= 0 ? 'E' : 'W';
        text = '${lat.abs().toStringAsFixed(2)}°$ns  ${lon.abs().toStringAsFixed(2)}°$ew  $text';
      }
    }
    return text;
  }

  /// One place per group of memories within a finger's width of each other
  /// on screen, in the order filmed.
  List<(Offset, List<Memory>)> _places(List<Memory> memories, Rect mapRect, Matrix4 matrix) {
    final placed = <(Offset, List<Memory>)>[];
    final pinned = memories.where((m) => m.pin != null).toList()
      ..sort((a, b) => a.start.compareTo(b.start));
    for (final memory in pinned) {
      final unit = project(memory.pin!.lat, memory.pin!.lon);
      final local = Offset(
        mapRect.left + unit.dx * mapRect.width,
        mapRect.top + unit.dy * mapRect.height,
      );
      final screen = MatrixUtils.transformPoint(matrix, local);
      final near = placed.indexWhere((p) => (p.$1 - screen).distance < 26);
      if (near >= 0) {
        placed[near].$2.add(memory);
      } else {
        placed.add((screen, [memory]));
      }
    }
    return placed;
  }

  /// Each place's signal, and a tag naming it, placed clear of the tags
  /// already down and tied to its signal by a line.
  List<Widget> _signals(List<Memory> memories, Rect mapRect) {
    final places = _places(memories, mapRect, _transform.value);
    // Tags keep clear of each other and of every signal.
    final tags = <Rect>[
      for (final (at, _) in places) Rect.fromCircle(center: at, radius: 9),
    ];
    final leaders = <(Offset, Offset, Color)>[];
    final widgets = <Widget>[];
    // From the top of the screen down, so tags stack upwards cleanly.
    places.sort((a, b) => a.$1.dy.compareTo(b.$1.dy));
    for (final (at, group) in places) {
      final kinds = group.map((m) => m.kind).toSet();
      final color = kinds.length == 1 ? kindColor(group.first.kind) : _mixed;
      final more = group.length - 1;
      final label = group.first.label;
      final width = 32 + label.length * 7.2 + (more > 0 ? 10 + '+$more'.length * 7.2 : 0);
      const height = 24.0;
      Rect at0(double dx, double dy) =>
          Rect.fromCenter(center: at + Offset(dx, dy), width: width, height: height);
      final candidates = [
        at0(0, -34),
        at0(width / 2 + 16, -24),
        at0(-width / 2 - 16, -24),
        at0(0, 34),
        at0(0, -62),
        at0(width / 2 + 16, 24),
        at0(-width / 2 - 16, 24),
      ];
      final tag = candidates.firstWhere(
        (r) => !tags.any((t) => t.inflate(4).overlaps(r)),
        orElse: () => candidates.first,
      );
      tags.add(tag);
      final anchor = Offset(
        tag.center.dx.clamp(tag.left + 8, tag.right - 8),
        at.dy < tag.top ? tag.top : (at.dy > tag.bottom ? tag.bottom : tag.center.dy),
      );
      leaders.add((at, anchor, color));
      void tap() => group.length == 1 ? unawaited(_open(group.first)) : _choose(group);
      widgets.add(Positioned(
        left: at.dx - 22,
        top: at.dy - 22,
        width: 44,
        height: 44,
        child: GestureDetector(
          onTap: tap,
          behavior: HitTestBehavior.opaque,
          child: CustomPaint(painter: _SignalPainter(color, _pulse, many: group.length)),
        ),
      ));
      widgets.add(Positioned.fromRect(
        rect: tag,
        child: _Tag(
          key: Key('pin-${group.first.id}'),
          color: color,
          icon: kinds.length == 1 ? kindIcon(group.first.kind) : Icons.layers_rounded,
          label: label,
          more: more,
          onTap: tap,
        ),
      ));
    }
    return [
      Positioned.fill(
        child: IgnorePointer(child: CustomPaint(painter: _LeaderPainter(leaders))),
      ),
      ...widgets,
    ];
  }

  void _choose(List<Memory> group) {
    final s = S.of(context);
    unawaited(showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      backgroundColor: const Color(0xFF0B161C),
      builder: (context) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
              child: Text(s.memoriesHere(group.length), style: _mono(13, _Ink.dim)),
            ),
            for (final m in group)
              ListTile(
                leading: _KindBadge(kind: m.kind),
                title: Text(m.title, style: const TextStyle(color: _Ink.text)),
                subtitle: Text(
                  '${s.memoryDates(m.start, m.end)} · ${s.videoCount(m.videoCount)}',
                  style: _mono(11, _Ink.dim),
                ),
                onTap: () {
                  Navigator.of(context).pop();
                  unawaited(_open(m));
                },
              ),
          ],
        ),
      ),
    ));
  }
}

class _MapPainter extends CustomPainter {
  _MapPainter({
    required this.paths,
    required this.rect,
    required this.lit,
    required this.points,
    required this.dotColor,
    required this.step,
  }) : super(repaint: step);

  final _WorldPaths paths;
  final Rect rect;
  final Map<String, Color> lit;
  final List<GeoPoint> points;
  final Color? dotColor;
  final ValueNotifier<int> step;

  @override
  void paint(Canvas canvas, Size size) {
    final zoom = math.pow(2, step.value / 2).toDouble();
    // Lines a fixed number of screen pixels wide, whatever the zoom: the
    // canvas is scaled to the map's height, then by the zoom.
    double px(double pixels) => pixels / (rect.height * zoom);

    // The sea, lightest under the middle of the map, darkest at the edges.
    final whole = Offset.zero & size;
    canvas.drawRect(
      whole,
      Paint()
        ..shader = RadialGradient(
          center: Alignment(
            (rect.center.dx / size.width) * 2 - 1,
            (rect.center.dy / size.height) * 2 - 1,
          ),
          radius: 0.85,
          colors: const [_Ink.sea, _Ink.seaEdge],
        ).createShader(whole),
    );

    canvas.save();
    canvas.clipRect(rect);
    canvas.translate(rect.left, rect.top);
    canvas.scale(rect.height, rect.height);

    // The grid: every fifteen degrees faintly, every thirty a little more.
    final minor = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = px(0.6)
      ..color = _Ink.gridMinor;
    final major = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = px(0.8)
      ..color = _Ink.gridMajor;
    for (var lon = -165.0; lon < 180; lon += 15) {
      final x = (lon + 180) / 360 * mapAspect;
      canvas.drawLine(Offset(x, 0), Offset(x, 1), lon % 30 == 0 ? major : minor);
    }
    for (var lat = -45.0; lat <= 75; lat += 15) {
      final y = project(lat, 0).dy;
      canvas.drawLine(Offset(0, y), Offset(mapAspect, y), lat % 30 == 0 ? major : minor);
    }
    // The equator, marked as a monitor marks its zero.
    final equator = project(0, 0).dy;
    canvas.drawLine(
      Offset(0, equator),
      Offset(mapAspect, equator),
      Paint()
        ..strokeWidth = px(1)
        ..color = _Ink.gridMajor.withValues(alpha: 0.35),
    );

    // The land, with its coasts lit: a wide faint stroke under a narrow
    // bright one reads as a glow, and costs no blur over every coast.
    final fill = Paint()..style = PaintingStyle.fill;
    fill.color = _Ink.land;
    for (final path in paths.countries.values) {
      canvas.drawPath(path, fill);
    }
    fill.color = _Ink.province;
    for (final path in paths.provinces.values) {
      canvas.drawPath(path, fill);
    }
    final line = Paint()
      ..style = PaintingStyle.stroke
      ..strokeJoin = StrokeJoin.round;
    line
      ..strokeWidth = px(0.45)
      ..color = _Ink.provinceLine;
    for (final path in paths.provinces.values) {
      canvas.drawPath(path, line);
    }
    line
      ..strokeWidth = px(3.2)
      ..color = _Ink.coast.withValues(alpha: 0.10);
    canvas.drawPath(paths.coasts, line);
    line
      ..strokeWidth = px(0.7)
      ..color = _Ink.border;
    canvas.drawPath(paths.coasts, line);

    // Where memories were filmed: lit in their colour, edged and glowing.
    for (final MapEntry(key: id, value: color) in lit.entries) {
      final path = paths.provinces[id] ?? paths.countries[id];
      if (path == null) continue;
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = px(9)
          ..color = color.withValues(alpha: 0.32)
          ..maskFilter = MaskFilter.blur(BlurStyle.normal, px(7)),
      );
      canvas.drawPath(path, Paint()..color = color.withValues(alpha: 0.24));
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = px(1.1)
          ..strokeJoin = StrokeJoin.round
          ..color = color.withValues(alpha: 0.9),
      );
    }

    // Where each video of the memories shown was filmed.
    if (dotColor != null) {
      final halo = Paint()
        ..color = dotColor!.withValues(alpha: 0.35)
        ..maskFilter = MaskFilter.blur(BlurStyle.normal, px(4));
      final dot = Paint()..color = dotColor!;
      final core = Paint()..color = Colors.white.withValues(alpha: 0.85);
      for (final p in points) {
        final at = _square(project(p.lat, p.lon));
        canvas.drawCircle(at, px(6), halo);
        canvas.drawCircle(at, px(2.6), dot);
        canvas.drawCircle(at, px(1), core);
      }
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_MapPainter old) =>
      !_sameColors(old.lit, lit) ||
      old.points != points ||
      old.dotColor != dotColor ||
      old.rect != rect ||
      old.paths != paths;

  static bool _sameColors(Map<String, Color> a, Map<String, Color> b) =>
      a.length == b.length && a.entries.every((e) => b[e.key] == e.value);
}

/// Scan lines across the monitor and marks at its corners, drawn once.
class _GlassPainter extends CustomPainter {
  const _GlassPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final scan = Paint()
      ..color = Colors.black.withValues(alpha: 0.10)
      ..strokeWidth = 1;
    for (var y = 0.0; y < size.height; y += 3) {
      canvas.drawLine(Offset(0, y), Offset(size.width, y), scan);
    }
    final mark = Paint()
      ..color = _Ink.coast.withValues(alpha: 0.55)
      ..strokeWidth = 1.5
      ..style = PaintingStyle.stroke;
    const inset = 8.0;
    const arm = 18.0;
    final r = (Offset.zero & size).deflate(inset);
    for (final (corner, dx, dy) in [
      (r.topLeft, 1.0, 1.0),
      (r.topRight, -1.0, 1.0),
      (r.bottomLeft, 1.0, -1.0),
      (r.bottomRight, -1.0, -1.0),
    ]) {
      canvas.drawPath(
        Path()
          ..moveTo(corner.dx + dx * arm, corner.dy)
          ..lineTo(corner.dx, corner.dy)
          ..lineTo(corner.dx, corner.dy + dy * arm),
        mark,
      );
    }
  }

  @override
  bool shouldRepaint(_GlassPainter old) => false;
}

/// A place's signal: a bright point in a halo, ringed by a pulse that
/// spreads and fades, and a second ring for several memories at once.
class _SignalPainter extends CustomPainter {
  _SignalPainter(this.color, this.pulse, {required this.many}) : super(repaint: pulse);

  final Color color;
  final Animation<double> pulse;
  final int many;

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    for (final phase in const [0.0, 0.5]) {
      final t = (pulse.value + phase) % 1.0;
      canvas.drawCircle(
        c,
        5 + 16 * t,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.4
          ..color = color.withValues(alpha: 0.55 * (1 - t)),
      );
    }
    canvas.drawCircle(
      c,
      9,
      Paint()
        ..color = color.withValues(alpha: 0.45)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 5),
    );
    if (many > 1) {
      canvas.drawCircle(
        c,
        8.5,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.2
          ..color = color.withValues(alpha: 0.9),
      );
    }
    canvas.drawCircle(c, 4.6, Paint()..color = color);
    canvas.drawCircle(c, 1.8, Paint()..color = Colors.white);
  }

  @override
  bool shouldRepaint(_SignalPainter old) => old.color != color || old.many != many;
}

/// The lines tying each tag to its signal.
class _LeaderPainter extends CustomPainter {
  _LeaderPainter(this.lines);

  final List<(Offset, Offset, Color)> lines;

  @override
  void paint(Canvas canvas, Size size) {
    for (final (from, to, color) in lines) {
      final paint = Paint()
        ..strokeWidth = 1
        ..color = color.withValues(alpha: 0.75);
      final d = to - from;
      // Start at the signal's edge, not its middle.
      final start = from + (d.distance > 0 ? d / d.distance * 7 : Offset.zero);
      canvas.drawLine(start, to, paint);
      canvas.drawCircle(to, 1.6, Paint()..color = color);
    }
  }

  @override
  bool shouldRepaint(_LeaderPainter old) => true;
}

/// The tag naming a place: when, in a monitor's face, and how many more.
class _Tag extends StatelessWidget {
  const _Tag({
    super.key,
    required this.color,
    required this.icon,
    required this.label,
    required this.more,
    required this.onTap,
  });

  final Color color;
  final IconData icon;
  final String label;
  final int more;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      behavior: HitTestBehavior.opaque,
      child: Container(
        decoration: BoxDecoration(
          color: _Ink.panel,
          borderRadius: BorderRadius.circular(5),
          border: Border.all(color: color.withValues(alpha: 0.7)),
          boxShadow: [BoxShadow(color: color.withValues(alpha: 0.25), blurRadius: 10)],
        ),
        padding: const EdgeInsets.symmetric(horizontal: 6),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(icon, size: 12, color: color),
            const SizedBox(width: 5),
            Flexible(
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.clip,
                style: _mono(11, _Ink.text, weight: FontWeight.w700),
              ),
            ),
            if (more > 0) ...[
              const SizedBox(width: 5),
              Text('+$more', style: _mono(10.5, color, weight: FontWeight.w700)),
            ],
          ],
        ),
      ),
    );
  }
}

/// A dark panel with a hairline edge, as every readout on the map sits in.
class _Panel extends StatelessWidget {
  const _Panel({required this.child, this.padding = const EdgeInsets.all(12)});

  final Widget child;
  final EdgeInsets padding;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: _Ink.panel,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: _Ink.panelLine),
        boxShadow: const [BoxShadow(color: Color(0x66000000), blurRadius: 18, offset: Offset(0, 6))],
      ),
      child: Padding(padding: padding, child: child),
    );
  }
}

/// The map's name and what it shows.
class _Title extends StatelessWidget {
  const _Title({required this.stats});

  final String? stats;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    return _Panel(
      padding: const EdgeInsets.fromLTRB(14, 10, 16, 11),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  color: const Color(0xFF3DDC97),
                  shape: BoxShape.circle,
                  boxShadow: [
                    BoxShadow(color: const Color(0xFF3DDC97).withValues(alpha: 0.8), blurRadius: 6),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              Text('MEMORY MAP', style: _mono(10.5, _Ink.dim, weight: FontWeight.w600)),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            s.tabMemoryMap,
            style: const TextStyle(
              color: _Ink.text,
              fontSize: 17,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.5,
            ),
          ),
          if (stats != null) ...[
            const SizedBox(height: 3),
            Text(stats!, key: const Key('map-stats'), style: _mono(11.5, _Ink.dim)),
          ],
        ],
      ),
    );
  }
}

/// The layers: every memory, or one kind, each with its colour and count.
/// Open on a wide screen; a single row to open on a phone.
class _Layers extends StatelessWidget {
  const _Layers({
    required this.open,
    required this.filter,
    required this.counts,
    required this.total,
    required this.onToggle,
    required this.onChanged,
  });

  final bool open;
  final MemoryKind? filter;
  final Map<MemoryKind, int> counts;
  final int total;
  final VoidCallback onToggle;
  final ValueChanged<MemoryKind?> onChanged;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    Widget row(MemoryKind? kind) {
      final selected = filter == kind;
      final color = kind == null ? _Ink.coast : kindColor(kind);
      return InkWell(
        key: Key('map-filter-${kind?.id ?? 'all'}'),
        onTap: () => onChanged(kind),
        borderRadius: BorderRadius.circular(6),
        child: Container(
          width: 196,
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          decoration: BoxDecoration(
            color: selected ? color.withValues(alpha: 0.14) : null,
            borderRadius: BorderRadius.circular(6),
            border: Border(
              left: BorderSide(color: selected ? color : Colors.transparent, width: 2),
            ),
          ),
          child: Row(
            children: [
              Icon(kind == null ? Icons.public_rounded : kindIcon(kind), size: 15, color: color),
              const SizedBox(width: 9),
              Expanded(
                child: Text(
                  kind == null ? s.mapAllMemories : s.memoryKindLabel(kindId(kind)),
                  style: TextStyle(
                    color: selected ? _Ink.text : _Ink.dim,
                    fontSize: 13,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
                  ),
                ),
              ),
              Text(
                '${kind == null ? total : counts[kind] ?? 0}',
                style: _mono(12, selected ? color : _Ink.faint, weight: FontWeight.w700),
              ),
            ],
          ),
        ),
      );
    }

    final header = InkWell(
      key: const Key('map-filter'),
      onTap: onToggle,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.layers_rounded,
              size: 15,
              color: filter == null ? _Ink.coast : kindColor(filter!),
            ),
            const SizedBox(width: 8),
            Text(
              open ? s.mapLayers.toUpperCase() : (filter == null ? s.mapAllMemories : s.memoryKindLabel(kindId(filter!))),
              style: open ? _mono(10.5, _Ink.dim, weight: FontWeight.w600) : const TextStyle(color: _Ink.text, fontSize: 13),
            ),
            const SizedBox(width: 6),
            Icon(open ? Icons.expand_less_rounded : Icons.expand_more_rounded, size: 18, color: _Ink.dim),
          ],
        ),
      ),
    );
    return _Panel(
      padding: const EdgeInsets.all(6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          header,
          if (open) ...[
            const SizedBox(height: 4),
            row(null),
            for (final k in MemoryKind.values) row(k),
          ],
        ],
      ),
    );
  }
}

/// Where the view is, under the attribution the outlines need.
class _Readout extends StatelessWidget {
  const _Readout({required this.text, required this.attribution});

  final String text;
  final String attribution;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(text, key: const Key('map-readout'), style: _mono(11, _Ink.coast)),
        const SizedBox(height: 2),
        Text(attribution, style: _mono(10, _Ink.faint)),
      ],
    );
  }
}

class _MapButton extends StatelessWidget {
  const _MapButton({super.key, required this.icon, required this.tooltip, required this.onTap});

  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: Material(
        color: _Ink.panel,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(8),
          side: const BorderSide(color: _Ink.panelLine),
        ),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onTap: onTap,
          child: SizedBox(
            width: 36,
            height: 36,
            child: Icon(icon, size: 19, color: _Ink.text),
          ),
        ),
      ),
    );
  }
}

class _Notice extends StatelessWidget {
  const _Notice({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 380),
      child: _Panel(
        padding: const EdgeInsets.all(22),
        child: Text(
          text,
          textAlign: TextAlign.center,
          style: const TextStyle(color: _Ink.text, fontSize: 14, height: 1.5),
        ),
      ),
    );
  }
}

class _KindBadge extends StatelessWidget {
  const _KindBadge({required this.kind});

  final MemoryKind kind;

  @override
  Widget build(BuildContext context) {
    final color = kindColor(kind);
    return Container(
      width: 34,
      height: 34,
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.16),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: color.withValues(alpha: 0.6)),
      ),
      child: Icon(kindIcon(kind), color: color, size: 18),
    );
  }
}

/// The memories shown, as cards along the bottom of the map.
class _MemoryStrip extends StatelessWidget {
  const _MemoryStrip({required this.memories, required this.covers, required this.onOpen});

  final List<Memory> memories;
  final Map<String, Uint8List> covers;
  final ValueChanged<Memory> onOpen;

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final unpinned = memories.where((m) => m.pin == null).length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (unpinned > 0)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 6),
            child: Text(s.unpinnedMemories(unpinned), style: _mono(10.5, _Ink.dim)),
          ),
        SizedBox(
          height: 118,
          child: ListView.separated(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.fromLTRB(14, 0, 14, 14),
            itemCount: memories.length,
            separatorBuilder: (_, __) => const SizedBox(width: 10),
            itemBuilder: (context, i) {
              final m = memories[i];
              final cover = m.cover == null ? null : covers[m.cover];
              final color = kindColor(m.kind);
              return SizedBox(
                width: 268,
                child: Material(
                  key: Key('memory-card-${m.id}'),
                  color: _Ink.panel,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(10),
                    side: const BorderSide(color: _Ink.panelLine),
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: InkWell(
                    onTap: () => onOpen(m),
                    child: Row(
                      children: [
                        Container(width: 3, color: color),
                        SizedBox(
                          width: 92,
                          height: double.infinity,
                          child: cover == null
                              ? ColoredBox(
                                  color: color.withValues(alpha: 0.10),
                                  child: Icon(kindIcon(m.kind), color: color),
                                )
                              : Stack(
                                  fit: StackFit.expand,
                                  children: [
                                    Image.memory(cover, fit: BoxFit.cover, gaplessPlayback: true),
                                    DecoratedBox(
                                      decoration: BoxDecoration(
                                        gradient: LinearGradient(
                                          begin: Alignment.centerLeft,
                                          end: Alignment.centerRight,
                                          colors: [Colors.transparent, _Ink.panel.withValues(alpha: 0.9)],
                                        ),
                                      ),
                                    ),
                                  ],
                                ),
                        ),
                        Expanded(
                          child: Padding(
                            padding: const EdgeInsets.fromLTRB(10, 9, 10, 9),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                Row(children: [
                                  Icon(kindIcon(m.kind), size: 13, color: color),
                                  const SizedBox(width: 5),
                                  Text(
                                    s.memoryKindLabel(kindId(m.kind)).toUpperCase(),
                                    style: _mono(10, color, weight: FontWeight.w700),
                                  ),
                                ]),
                                const SizedBox(height: 4),
                                Flexible(
                                  child: Text(
                                    m.title,
                                    maxLines: 2,
                                    overflow: TextOverflow.ellipsis,
                                    style: const TextStyle(
                                      color: _Ink.text,
                                      fontSize: 13.5,
                                      fontWeight: FontWeight.w600,
                                      height: 1.3,
                                    ),
                                  ),
                                ),
                                const SizedBox(height: 4),
                                Text(
                                  '${m.label} · ${s.videoCount(m.videoCount)}',
                                  style: _mono(10.5, _Ink.dim),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}

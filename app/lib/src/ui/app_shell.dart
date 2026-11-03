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

import '../engine/engine.dart';
import '../engine/engine_api.dart';
import '../l10n/strings.dart';
import 'butler_page.dart';
import 'home_page.dart';
import 'models_page.dart';
import 'projects_page.dart';
import 'self_test_page.dart';
import 'settings_page.dart';

/// The app as the user moves through it: the self-test first, then projects,
/// the video butler, the model library and settings.
class AppFlow extends StatefulWidget {
  const AppFlow({super.key, required this.engine, this.skipSelfTest = false});

  final Engine engine;

  /// For tests that are about what comes after the self-test.
  final bool skipSelfTest;

  @override
  State<AppFlow> createState() => _AppFlowState();
}

class _AppFlowState extends State<AppFlow> {
  late bool _checked = widget.skipSelfTest;

  @override
  Widget build(BuildContext context) {
    if (!_checked) {
      return SelfTestPage(
        engine: widget.engine,
        onContinue: () => setState(() => _checked = true),
      );
    }
    return AppShell(engine: widget.engine);
  }
}

class AppShell extends StatefulWidget {
  const AppShell({super.key, required this.engine});

  final Engine engine;

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _index = 0;

  @override
  void initState() {
    super.initState();
    unawaited(_keepUp());
  }

  /// Hosting is "keep my footage sorted": each time the app opens, the
  /// butler looks for what is new since the last pass. A pass over footage
  /// it has seen reads only file sizes and dates, so this is cheap.
  Future<void> _keepUp() async {
    try {
      final hosting = await widget.engine.hosting();
      final last = hosting.scannedAt == null ? null : DateTime.tryParse(hosting.scannedAt!);
      final stale = last == null || DateTime.now().difference(last) > const Duration(minutes: 30);
      if (hosting.enabled && hosting.jobId == null && stale) {
        await widget.engine.startHosting();
      }
    } on EngineException {
      // The butler's page says what is wrong when it is opened.
    }
  }

  /// Rebuilt on every switch so each page reloads what it shows: the album
  /// after a run, the models after a download.
  int _generation = 0;

  void _select(int index) => setState(() {
        _index = index;
        _generation++;
      });

  @override
  Widget build(BuildContext context) {
    final s = S.of(context);
    final pages = <(String, IconData, Widget)>[
      (
        s.navProjects,
        Icons.movie_edit,
        ProjectsPage(
          key: ValueKey('projects-$_generation'),
          engine: widget.engine,
          onShowAlbum: () => _select(1),
        ),
      ),
      (
        s.navAlbum,
        Icons.auto_awesome_rounded,
        ButlerPage(key: ValueKey('butler-$_generation'), engine: widget.engine, embedded: true),
      ),
      (
        s.navModels,
        Icons.download_for_offline_outlined,
        ModelsPage(key: ValueKey('models-$_generation'), engine: widget.engine, embedded: true),
      ),
      (
        s.navSettings,
        Icons.settings_outlined,
        SettingsPage(key: ValueKey('settings-$_generation'), engine: widget.engine, embedded: true),
      ),
      // A single run with no project to save, kept for trying a clip out.
      (
        s.navQuickDraft,
        Icons.bolt_rounded,
        HomePage(key: ValueKey('quick-$_generation'), engine: widget.engine, embedded: true),
      ),
    ];

    // A phone gets its pages along the bottom: a rail with labels took half
    // of its width and squeezed every page into the rest.
    if (MediaQuery.sizeOf(context).width < 600) {
      return Scaffold(
        appBar: AppBar(title: Text(pages[_index].$1)),
        body: pages[_index].$3,
        bottomNavigationBar: NavigationBar(
          selectedIndex: _index,
          onDestinationSelected: _select,
          labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
          destinations: [
            for (final (label, icon, _) in pages)
              NavigationDestination(icon: Icon(icon), label: label),
          ],
        ),
      );
    }

    return Scaffold(
      body: Row(
        children: [
          NavigationRail(
            selectedIndex: _index,
            onDestinationSelected: _select,
            labelType: NavigationRailLabelType.all,
            leading: Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Icon(
                Icons.movie_filter_rounded,
                size: 28,
                color: Theme.of(context).colorScheme.primary,
              ),
            ),
            destinations: [
              for (final (label, icon, _) in pages)
                NavigationRailDestination(icon: Icon(icon), label: Text(label)),
            ],
          ),
          const VerticalDivider(width: 1),
          Expanded(
            child: Column(
              children: [
                AppBar(
                  title: Text(pages[_index].$1),
                  automaticallyImplyLeading: false,
                ),
                Expanded(child: pages[_index].$3),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

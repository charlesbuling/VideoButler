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
import 'widgets.dart';

/// What the app checks before anything else.
///
/// Every line here fails in a way that looks like a bug in the app if it is
/// found in the middle of a run instead: no video engine, no writable
/// library, no draft folder for the editor, an editor that is open and will
/// not see a new draft until it restarts. Saying so up front turns each of
/// those into a sentence the user can act on.
class SelfTestPage extends StatefulWidget {
  const SelfTestPage(
      {super.key, required this.engine, required this.onContinue});

  final Engine engine;
  final VoidCallback onContinue;

  @override
  State<SelfTestPage> createState() => _SelfTestPageState();
}

class _SelfTestPageState extends State<SelfTestPage> {
  SelfTest? _result;
  String? _error;
  bool _running = true;

  Future<void> _run(AppLanguage language) async {
    setState(() {
      _running = true;
      _error = null;
    });
    try {
      final result = await widget.engine.selfTest(language: language.id);
      if (!mounted) return;
      setState(() {
        _result = result;
        _running = false;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.message;
        _running = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _running = false;
      });
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // The checks are worded by the engine, so a change of language asks for
    // them again rather than leaving them in the old one.
    final language = LanguageScope.maybeOf(context)?.language ?? AppLanguage.zh;
    if (!_started || (language != _shownIn && !_running)) {
      _started = true;
      _shownIn = language;
      // After this frame, not during it: _run sets state.
      scheduleMicrotask(() => unawaited(_run(language)));
    }
  }

  /// The first check starts here rather than in initState, because it needs
  /// the language, and that comes from the widget tree.
  bool _started = false;
  AppLanguage? _shownIn;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final result = _result;

    return Scaffold(
      // No app bar on this page, so nothing else keeps the heading out from
      // under a phone's status bar and notch.
      body: SafeArea(
        bottom: false,
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 720),
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.all(28),
              children: [
                Row(
                  children: [
                    Icon(Icons.movie_edit,
                        size: 32, color: theme.colorScheme.primary),
                    const SizedBox(width: 12),
                    Text(s.appName, style: theme.textTheme.headlineSmall),
                  ],
                ),
                const SizedBox(height: 6),
                Text(
                  s.selfTestIntro,
                  style: theme.textTheme.bodyMedium
                      ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                ),
                const SizedBox(height: 24),
                if (_running)
                  const Padding(
                    padding: EdgeInsets.all(40),
                    child: Center(child: CircularProgressIndicator()),
                  ),
                if (_error != null)
                  NoticeBanner(
                    title: s.engineUnreachable,
                    lines: [_error!],
                    tone: Tone.bad,
                    icon: Icons.error_outline_rounded,
                  ),
                if (result != null && !_running) ...[
                  SectionCard(
                    title: s.environment,
                    subtitle: switch (result.status) {
                      'pass' => s.envAllGood,
                      'fail' => s.envMissing,
                      _ => s.envWorthKnowing,
                    },
                    icon: Icons.fact_check_outlined,
                    trailing: StatusChip(
                      label: result.canAnalyse ? s.ready : s.cannotRun,
                      tone: result.canAnalyse ? Tone.good : Tone.bad,
                      icon: result.canAnalyse
                          ? Icons.check_rounded
                          : Icons.priority_high_rounded,
                    ),
                    child: Column(
                      children: [
                        for (final check in result.checks)
                          _CheckRow(check: check),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
      // Outside the list, so it is on screen however many checks there are
      // and however small the window is.
      bottomNavigationBar: SafeArea(
        child: Center(
          heightFactor: 1,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 720),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(28, 8, 28, 20),
              child: Row(
                children: [
                  TextButton.icon(
                    onPressed: _running
                        ? null
                        : () => unawaited(_run(_shownIn ?? AppLanguage.zh)),
                    icon: const Icon(Icons.refresh_rounded, size: 18),
                    label: Text(s.checkAgain),
                  ),
                  const Spacer(),
                  FilledButton.icon(
                    key: const Key('selftest-continue'),
                    // Continuing is allowed even when a run cannot happen:
                    // projects, the album and the model library still work,
                    // and a user who is about to install ffmpeg should not be
                    // locked out of the settings.
                    onPressed: _running ? null : widget.onContinue,
                    icon: const Icon(Icons.arrow_forward_rounded),
                    label: Text(s.continueLabel),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _CheckRow extends StatelessWidget {
  const _CheckRow({required this.check});

  final CheckResult check;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final (icon, color) = switch (check.status) {
      'pass' => (Icons.check_circle_rounded, theme.colorScheme.primary),
      'fail' => (Icons.cancel_rounded, theme.colorScheme.error),
      _ => (Icons.info_rounded, theme.colorScheme.tertiary),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 20, color: color),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(check.label, style: theme.textTheme.bodyMedium),
                const SizedBox(height: 2),
                Text(
                  check.detail,
                  style: theme.textTheme.bodySmall
                      ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

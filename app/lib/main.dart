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
import 'package:flutter_localizations/flutter_localizations.dart';

import 'src/engine/engine.dart';
import 'src/engine/engine_api.dart';
import 'src/engine/engine_factory.dart';
import 'src/engine/platform_models.dart';
import 'src/l10n/strings.dart';
import 'src/theme.dart';
import 'src/ui/app_shell.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // On Android the bundled models are copied out of the APK before the
  // self-test looks for them; elsewhere this returns at once.
  await PlatformModels.prepare();
  runApp(VideoButlerApp(engine: createEngine()));
}

class VideoButlerApp extends StatefulWidget {
  const VideoButlerApp({
    super.key,
    required this.engine,
    this.skipSelfTest = false,
    this.language,
  });

  final Engine engine;

  /// For tests about what comes after the self-test.
  final bool skipSelfTest;

  /// A language to start in, for tests. Otherwise the one the user last
  /// chose, and Chinese until they have chosen.
  final AppLanguage? language;

  @override
  State<VideoButlerApp> createState() => _VideoButlerAppState();
}

class _VideoButlerAppState extends State<VideoButlerApp> {
  late AppLanguage _language = widget.language ?? AppLanguage.zh;

  @override
  void initState() {
    super.initState();
    if (widget.language == null) unawaited(_loadLanguage());
  }

  Future<void> _loadLanguage() async {
    try {
      final settings = await widget.engine.settings();
      if (!mounted) return;
      setState(() => _language = AppLanguage.parse(settings.language));
    } on EngineException {
      // The self-test says what is wrong with the engine; the language
      // stays at its default meanwhile.
    }
  }

  /// Change the language, and tell the engine, which writes its own
  /// sentences -- the self-test, warnings, why each clip was chosen -- in it.
  void _setLanguage(AppLanguage language) {
    setState(() => _language = language);
    unawaited(
      widget.engine.saveLanguage(language.id).then((_) {}, onError: (_) {}),
    );
  }

  @override
  Widget build(BuildContext context) {
    return LanguageScope(
      language: _language,
      onChanged: _setLanguage,
      child: MaterialApp(
        title: 'Video Butler',
        debugShowCheckedModeBanner: false,
        theme: buildTheme(Brightness.light),
        darkTheme: buildTheme(Brightness.dark),
        locale: _language.locale,
        supportedLocales: [for (final language in AppLanguage.values) language.locale],
        localizationsDelegates: const [
          GlobalMaterialLocalizations.delegate,
          GlobalWidgetsLocalizations.delegate,
          GlobalCupertinoLocalizations.delegate,
        ],
        home: AppFlow(engine: widget.engine, skipSelfTest: widget.skipSelfTest),
      ),
    );
  }
}

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
import '../models.dart';
import '../theme.dart';
import 'widgets.dart';

/// Where the app keeps things, and the cloud endpoint.
///
/// The endpoint is OpenAI-compatible rather than a named provider, so the same
/// form points at a hosted API or at a model server on this machine. The key
/// never comes back from the engine: the form shows whether one is stored and
/// sends a new one only when the user types it.
class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.engine, this.embedded = false});

  final Engine engine;
  final bool embedded;

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  AppSettings? _settings;
  String? _error;
  bool _saving = false;
  bool _checking = false;
  Map<String, dynamic>? _check;
  CacheInfo? _cache;
  bool _clearing = false;

  final _baseUrl = TextEditingController();
  final _model = TextEditingController();
  final _visionModel = TextEditingController();
  final _apiKey = TextEditingController();

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void dispose() {
    _baseUrl.dispose();
    _model.dispose();
    _visionModel.dispose();
    _apiKey.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    unawaited(_loadCache());
    try {
      final settings = await widget.engine.settings();
      if (!mounted) return;
      setState(() {
        _settings = settings;
        _baseUrl.text = settings.cloud.baseUrl;
        _model.text = settings.cloud.model;
        _visionModel.text = settings.cloud.visionModel;
        _apiKey.clear();
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _check = null;
    });
    try {
      final saved = await widget.engine.saveCloud(
        baseUrl: _baseUrl.text,
        model: _model.text,
        visionModel: _visionModel.text,
        // Only a key the user typed is sent; an empty field keeps the stored
        // one, so reopening this page and saving cannot erase it.
        apiKey: _apiKey.text.trim().isEmpty ? null : _apiKey.text.trim(),
      );
      if (!mounted) return;
      setState(() {
        _settings = saved;
        _apiKey.clear();
        _saving = false;
      });
      _toast(S.of(context).saved);
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() => _saving = false);
      _toast(e.message);
    }
  }

  Future<void> _forgetKey() async {
    try {
      final saved = await widget.engine.saveCloud(
        baseUrl: _baseUrl.text,
        model: _model.text,
        visionModel: _visionModel.text,
        apiKey: '',
      );
      if (!mounted) return;
      setState(() => _settings = saved);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _test() async {
    setState(() {
      _checking = true;
      _check = null;
    });
    try {
      final result = await widget.engine.checkCloud();
      if (!mounted) return;
      setState(() {
        _check = result;
        _checking = false;
      });
    } on EngineException catch (e) {
      if (!mounted) return;
      setState(() {
        _check = {'reachable': false, 'error': e.message};
        _checking = false;
      });
    }
  }

  Future<void> _loadCache() async {
    try {
      final cache = await widget.engine.cacheInfo();
      if (mounted) setState(() => _cache = cache);
    } on EngineException {
      // Not knowing the size is no reason to hide the button.
    }
  }

  Future<void> _clearCache() async {
    final s = S.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(s.clearCacheTitle),
        content: Text(s.clearCacheBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(s.cancel),
          ),
          FilledButton(
            key: const Key('cache-clear-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(s.clearCache),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    setState(() => _clearing = true);
    try {
      final cleared = await widget.engine.clearCache();
      if (!mounted) return;
      _toast(s.cacheCleared(formatBytes(cleared.bytes)));
    } on EngineException catch (e) {
      _toast(e.message);
    }
    if (mounted) setState(() => _clearing = false);
    await _loadCache();
  }

  Future<void> _saveDraftMedia(String mode) async {
    try {
      final saved = await widget.engine.saveDraftMedia(mode);
      if (mounted) setState(() => _settings = saved);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  Future<void> _saveThermalGuard(bool on) async {
    try {
      final saved = await widget.engine.saveThermalGuard(on);
      if (mounted) setState(() => _settings = saved);
    } on EngineException catch (e) {
      _toast(e.message);
    }
  }

  void _toast(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final s = S.of(context);
    final settings = _settings;
    final scope = LanguageScope.maybeOf(context);
    final body = Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(20, 20, 20, 60),
          children: [
            if (_error != null)
              NoticeBanner(title: s.settingsUnreadable, lines: [_error!], tone: Tone.bad),
            SectionCard(
              title: s.languageTitle,
              icon: Icons.translate_rounded,
              child: Align(
                alignment: Alignment.centerLeft,
                child: SegmentedButton<AppLanguage>(
                  key: const Key('language'),
                  segments: const [
                    ButtonSegment(value: AppLanguage.zh, label: Text('中文')),
                    ButtonSegment(value: AppLanguage.en, label: Text('English')),
                  ],
                  selected: {scope?.language ?? AppLanguage.zh},
                  onSelectionChanged: scope == null
                      ? null
                      : (choice) => scope.onChanged(choice.first),
                ),
              ),
            ),
            const SizedBox(height: 16),
            SectionCard(
              title: s.cloudModel,
              subtitle: s.cloudModelSubtitle,
              icon: Icons.cloud_outlined,
              trailing: settings == null
                  ? null
                  : StatusChip(
                      label: settings.cloud.configured ? s.setUp : s.notSetUp,
                      tone: settings.cloud.configured ? Tone.good : Tone.neutral,
                    ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  TextField(
                    key: const Key('cloud-url'),
                    controller: _baseUrl,
                    decoration: InputDecoration(
                      labelText: s.baseUrl,
                      hintText: 'https://api.example.com/v1',
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _model,
                    decoration: InputDecoration(
                      labelText: s.model,
                      hintText: s.modelHint,
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _visionModel,
                    decoration: InputDecoration(
                      labelText: s.visionModel,
                      hintText: s.visionModelHint,
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _apiKey,
                    obscureText: true,
                    decoration: InputDecoration(
                      labelText: s.apiKey,
                      hintText: settings?.cloud.apiKeySet ?? false
                          ? s.apiKeyStored
                          : s.apiKeyNotNeeded,
                    ),
                  ),
                  if (settings?.cloud.apiKeySet ?? false)
                    Align(
                      alignment: Alignment.centerLeft,
                      child: TextButton(
                        onPressed: () => unawaited(_forgetKey()),
                        child: Text(s.forgetKey),
                      ),
                    ),
                  const SizedBox(height: 8),
                  Text(
                    s.keyPrivacy,
                    style: theme.textTheme.bodySmall
                        ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                  ),
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      OutlinedButton.icon(
                        onPressed: _checking || !(settings?.cloud.configured ?? false)
                            ? null
                            : () => unawaited(_test()),
                        icon: _checking
                            ? const SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(strokeWidth: 2),
                              )
                            : const Icon(Icons.network_check_rounded, size: 18),
                        label: Text(s.testConnection),
                      ),
                      const Spacer(),
                      FilledButton(
                        key: const Key('cloud-save'),
                        onPressed: _saving ? null : () => unawaited(_save()),
                        child: Text(s.save),
                      ),
                    ],
                  ),
                  if (_check != null) ...[
                    const SizedBox(height: 12),
                    _checkResult(),
                  ],
                ],
              ),
            ),
            const SizedBox(height: 16),
            if (settings != null) ...[
              SectionCard(
                title: s.draftMediaTitle,
                subtitle: s.draftMediaSubtitle,
                icon: Icons.link_rounded,
                child: RadioGroup<String>(
                  groupValue: settings.draftMedia,
                  onChanged: (value) => unawaited(_saveDraftMedia(value ?? 'link')),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (final (mode, title, note) in [
                        ('link', s.draftMediaLink, s.draftMediaLinkNote),
                        ('reference', s.draftMediaReference, s.draftMediaReferenceNote),
                      ])
                        RadioListTile<String>(
                          key: Key('draft-media-$mode'),
                          contentPadding: EdgeInsets.zero,
                          value: mode,
                          title: Text(title),
                          subtitle: Text(note),
                        ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 16),
              SectionCard(
                title: s.thermalTitle,
                icon: Icons.device_thermostat_rounded,
                child: SwitchListTile(
                  key: const Key('thermal-guard'),
                  contentPadding: EdgeInsets.zero,
                  value: settings.thermalGuard,
                  onChanged: (on) => unawaited(_saveThermalGuard(on)),
                  title: Text(s.thermalSwitch),
                  subtitle: Text(s.thermalNote),
                ),
              ),
              const SizedBox(height: 16),
            ],
            SectionCard(
              title: s.cacheTitle,
              icon: Icons.cached_rounded,
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      _cache == null
                          ? s.readingClips
                          : _cache!.entries == 0
                              ? s.cacheEmpty
                              : s.cacheSubtitle(_cache!.entries, formatBytes(_cache!.bytes)),
                      style: theme.textTheme.bodyMedium,
                    ),
                  ),
                  const SizedBox(width: 12),
                  OutlinedButton.icon(
                    key: const Key('cache-clear'),
                    onPressed: _clearing || (_cache?.entries ?? 1) == 0
                        ? null
                        : () => unawaited(_clearCache()),
                    icon: _clearing
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.delete_sweep_outlined, size: 18),
                    label: Text(s.clearCache),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            if (settings != null)
              SectionCard(
                title: s.library,
                subtitle: s.librarySubtitle,
                icon: Icons.inventory_2_outlined,
                child: Row(
                  children: [
                    Expanded(
                      child: SelectableText(
                        settings.libraryPath,
                        style: monoStyle(context, size: 12),
                      ),
                    ),
                    IconButton(
                      tooltip: s.showInFinder,
                      icon: const Icon(Icons.folder_open_rounded),
                      onPressed: () async {
                        try {
                          await widget.engine.reveal(settings.libraryPath);
                        } on EngineException catch (e) {
                          _toast(e.message);
                        }
                      },
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
    if (widget.embedded) return body;
    return Scaffold(appBar: AppBar(title: Text(s.navSettings)), body: body);
  }

  Widget _checkResult() {
    final s = S.of(context);
    final check = _check!;
    if (check['reachable'] == true) {
      final models = (check['models'] as List<dynamic>? ?? []).cast<Object>();
      final hasChosen = check['hasChosenModel'] == true;
      return NoticeBanner(
        title: s.endpointAnswered,
        lines: [
          if (models.isEmpty)
            s.endpointNoList
          else if (hasChosen)
            s.endpointHasModel(models.length)
          else
            s.endpointLacksModel(models.length, models.take(8).join(', ')),
        ],
        tone: hasChosen || models.isEmpty ? Tone.good : Tone.warning,
        icon: Icons.check_circle_outline_rounded,
      );
    }
    return NoticeBanner(
      title: s.endpointSilent,
      lines: [check['error']?.toString() ?? s.noReason],
      tone: Tone.bad,
      icon: Icons.cloud_off_outlined,
    );
  }
}

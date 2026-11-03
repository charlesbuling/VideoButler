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

/// The web engine: HTTP to `vb serve` on loopback.
///
/// This is how the desktop app works without Xcode. Flutter's web target
/// compiles the same Dart UI with no Apple toolchain at all, and `vb serve`
/// hosts both the built app and the engine API from one origin.
///
/// It calls the same Rust `api` module the FFI engine does, so the two cannot
/// behave differently; only the wire between them differs.
library;

import 'dart:convert';

import 'package:http/http.dart' as http;

import '../models.dart';
import 'engine.dart';

class HttpEngine implements Engine {
  HttpEngine({Uri? base}) : _base = base ?? Uri.base;

  /// Requests are same-origin by default, which is what `vb serve` expects:
  /// it sends no CORS headers, so nothing on another origin can read a
  /// response or reach the filesystem through it.
  final Uri _base;
  final _client = http.Client();

  @override
  String get transportLabel => 'local server (HTTP)';

  /// Build an API URL from the page's own origin.
  ///
  /// The query is replaced rather than inherited: `Uri.replace` keeps the
  /// original when passed null, so a page opened with a query string would
  /// otherwise leak it onto every API call.
  Uri _endpoint(String path, [Map<String, String>? query]) => Uri(
        scheme: _base.scheme,
        host: _base.host,
        port: _base.hasPort ? _base.port : null,
        path: path,
        queryParameters: query,
      );

  dynamic _decode(http.Response response) {
    final Map<String, dynamic> body;
    try {
      body = jsonDecode(response.body) as Map<String, dynamic>;
    } on FormatException {
      throw EngineException(
        'the engine returned something that is not JSON '
        '(HTTP ${response.statusCode}). Is `vb serve` still running?',
      );
    }
    if (body['status'] == 'error') {
      throw EngineException(body['message'] as String? ?? 'unknown error');
    }
    return body['data'];
  }

  Future<dynamic> _get(String path, [Map<String, String>? query]) async {
    try {
      return _decode(await _client.get(_endpoint(path, query)));
    } on http.ClientException catch (e) {
      throw EngineException('could not reach the engine: ${e.message}');
    }
  }

  @override
  Future<EngineVersion> version() async =>
      EngineVersion.fromJson(await _get('/api/version') as Map<String, dynamic>);

  @override
  Future<DoctorReport> doctor() async =>
      DoctorReport.fromJson(await _get('/api/doctor') as Map<String, dynamic>);

  @override
  Future<List<EditTemplate>> templates() async =>
      (await _get('/api/templates') as List<dynamic>)
          .map((t) => EditTemplate.fromJson(t as Map<String, dynamic>))
          .toList(growable: false);

  @override
  Future<DirectoryListing> browse(String? directory, {String kind = 'video'}) async {
    final data = await call('browse', {
      if (directory != null) 'dir': directory,
      'kind': kind,
    });
    return DirectoryListing.fromJson(data as Map<String, dynamic>);
  }

  /// POST to `/api/call/<method>`. POST even for reads, because many of these
  /// change state and the server refuses a GET for any of them: a GET that
  /// changed state would be triggerable by a link on any page.
  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) async {
    try {
      final response = await _client.post(
        _endpoint('/api/call/$method'),
        headers: const {'Content-Type': 'application/json'},
        body: jsonEncode(args),
      );
      return _decode(response);
    } on http.ClientException catch (e) {
      throw EngineException('could not reach the engine: ${e.message}');
    }
  }

  @override
  Future<RunResult> run({
    required List<String> inputs,
    String? projectDir,
    String? templateId,
  }) async {
    try {
      final response = await _client.post(
        _endpoint('/api/run'),
        headers: const {'Content-Type': 'application/json'},
        body: jsonEncode({
          'inputs': inputs,
          if (projectDir != null && projectDir.isNotEmpty)
            'projectDir': projectDir,
          if (templateId != null) 'templateId': templateId,
        }),
      );
      return RunResult.fromJson(_decode(response) as Map<String, dynamic>);
    } on http.ClientException catch (e) {
      throw EngineException('could not reach the engine: ${e.message}');
    }
  }

  @override
  void dispose() => _client.close();
}

Engine createEngine() => HttpEngine();

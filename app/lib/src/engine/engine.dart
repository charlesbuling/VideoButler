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

/// The engine seam.
///
/// The UI talks to this and never to a transport. There are two
/// implementations and they differ only in how bytes move:
///
/// - [dart:ffi] on native platforms, calling the Rust static library directly.
/// - HTTP on web, talking to `vb serve` over loopback.
///
/// Both call the same Rust `api` module, so behaviour cannot drift between
/// them; only the wire does.
library;

import '../models.dart';

abstract class Engine {
  /// How this engine reaches the core, for display.
  String get transportLabel;

  Future<EngineVersion> version();

  Future<DoctorReport> doctor();

  Future<List<EditTemplate>> templates();

  /// List directories and files. The engine owns file browsing so one picker
  /// works on every platform, including web where the UI cannot see the
  /// filesystem at all. [kind] is `video` (the default) or `audio`.
  Future<DirectoryListing> browse(String? directory, {String kind = 'video'});

  /// Analyse a project's videos and write one draft across them, blocking
  /// until it is done. Kept for callers that want no progress; the app uses
  /// `run.start` through [call] so it can show where a run has got to.
  Future<RunResult> run({
    required List<String> inputs,
    String? projectDir,
    String? templateId,
  });

  /// Any engine operation, by name.
  ///
  /// Everything added after the first handful of operations goes through
  /// here -- projects, runs with progress, the model library, settings, the
  /// album -- so the operation set can grow in Rust without each addition
  /// needing a new C function and a new HTTP route. [EngineApi] wraps it in
  /// typed methods.
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]);

  void dispose() {}
}

/// An engine call that failed for a reason worth showing the user.
class EngineException implements Exception {
  const EngineException(this.message);
  final String message;

  @override
  String toString() => message;
}

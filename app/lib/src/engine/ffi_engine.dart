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

/// The native engine: `dart:ffi` straight into the Rust library.
///
/// The C ABI was designed for exactly this. Every entry point takes and
/// returns C strings holding JSON, so there is no code generation, no bridge
/// package, and one small file of marshalling.
///
/// Two rules the Rust side guarantees and this side must honour: panics never
/// cross the boundary, and every returned string is freed here with
/// `vb_free_string`, because that memory came from Rust's allocator.
library;

import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

import '../models.dart';
import 'engine.dart';

typedef _NoArgC = Pointer<Utf8> Function();
typedef _RunC = Pointer<Utf8> Function(
    Pointer<Utf8>, Pointer<Utf8>, Pointer<Utf8>);
typedef _CallC = Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>);
typedef _FreeC = Void Function(Pointer<Utf8>);
typedef _FreeDart = void Function(Pointer<Utf8>);

class FfiEngine implements Engine {
  FfiEngine._(this._library);

  final DynamicLibrary _library;

  static FfiEngine open() => FfiEngine._(_load());

  /// Locate the engine library for the running platform.
  ///
  /// On Android it is a bundled `.so` beside the app. On Apple platforms the
  /// static library is linked into the executable, so the process itself is
  /// the right handle and there is no file to open.
  static DynamicLibrary _load() {
    if (Platform.isAndroid) {
      return DynamicLibrary.open('libvb_ffi.so');
    }
    if (Platform.isMacOS || Platform.isIOS) {
      final process = DynamicLibrary.process();
      if (process.providesSymbol('vb_version')) {
        return process;
      }
      return DynamicLibrary.open('libvb_ffi.dylib');
    }
    if (Platform.isWindows) {
      return DynamicLibrary.open('vb_ffi.dll');
    }
    return DynamicLibrary.open('libvb_ffi.so');
  }

  @override
  String get transportLabel => 'native (dart:ffi)';

  late final _version = _library.lookupFunction<_NoArgC, _NoArgC>('vb_version');
  late final _doctor = _library.lookupFunction<_NoArgC, _NoArgC>('vb_doctor');
  late final _templates =
      _library.lookupFunction<_NoArgC, _NoArgC>('vb_templates');
  late final _run = _library.lookupFunction<_RunC, _RunC>('vb_run');
  late final _call = _library.lookupFunction<_CallC, _CallC>('vb_call');
  late final _free =
      _library.lookupFunction<_FreeC, _FreeDart>('vb_free_string');

  /// Decode a returned string and hand its memory back to Rust.
  ///
  /// The free sits in a `finally` so a decode failure cannot leak it.
  dynamic _decode(Pointer<Utf8> pointer) {
    if (pointer == nullptr) {
      throw const EngineException('the engine returned nothing');
    }
    try {
      final decoded = jsonDecode(pointer.toDartString()) as Map<String, dynamic>;
      if (decoded['status'] == 'error') {
        throw EngineException(decoded['message'] as String? ?? 'unknown error');
      }
      return decoded['data'];
    } finally {
      _free(pointer);
    }
  }

  Map<String, dynamic> _object(Pointer<Utf8> pointer) =>
      (_decode(pointer) as Map<String, dynamic>?) ?? const {};

  List<dynamic> _list(Pointer<Utf8> pointer) =>
      (_decode(pointer) as List<dynamic>?) ?? const [];

  @override
  Future<EngineVersion> version() async =>
      EngineVersion.fromJson(_object(_version()));

  /// On a worker isolate: the report asks the system which editors are
  /// installed and running, and that is not work for the frame the user is
  /// looking at.
  @override
  Future<DoctorReport> doctor() => Isolate.run(() {
        final engine = FfiEngine.open();
        return DoctorReport.fromJson(engine._object(engine._doctor()));
      });

  @override
  Future<List<EditTemplate>> templates() async => _list(_templates())
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

  /// Nothing to release: the library handle lives as long as the process,
  /// and every allocation is freed at the call that made it.
  @override
  void dispose() {}

  /// Every operation goes to a worker isolate. Most are quick, but some are
  /// not -- checking a cloud endpoint waits on the network -- and a call that
  /// freezes the window for twenty seconds is not one the user can tell from
  /// a crash. The engine's state lives in Rust, not in the isolate, so a
  /// job started from one isolate is visible from the next.
  @override
  Future<dynamic> call(String method, [Map<String, dynamic> args = const {}]) {
    final encoded = jsonEncode(args);
    return Isolate.run(() {
      final engine = FfiEngine.open();
      final methodPtr = method.toNativeUtf8();
      final argsPtr = encoded.toNativeUtf8();
      try {
        return engine._decode(engine._call(methodPtr, argsPtr));
      } finally {
        calloc.free(methodPtr);
        calloc.free(argsPtr);
      }
    });
  }

  @override
  Future<RunResult> run({
    required List<String> inputs,
    String? projectDir,
    String? templateId,
  }) {
    // The C ABI takes the list as JSON: one function serves one video and a
    // folder of forty, and the engine already speaks JSON in both directions.
    final encoded = jsonEncode(inputs);
    // A full analysis decodes video and blocks for seconds to minutes.
    // Running it on the UI isolate would freeze the app, which is the whole
    // reason the engine is native rather than written in Dart.
    return Isolate.run(() {
      final engine = FfiEngine.open();
      final inputPtr = encoded.toNativeUtf8();
      final dirPtr = (projectDir ?? '').toNativeUtf8();
      final templatePtr =
          templateId == null ? nullptr : templateId.toNativeUtf8();
      try {
        return RunResult.fromJson(
          engine._object(
            engine._run(inputPtr.cast(), dirPtr.cast(), templatePtr.cast()),
          ),
        );
      } finally {
        calloc.free(inputPtr);
        calloc.free(dirPtr);
        if (templatePtr != nullptr) calloc.free(templatePtr);
      }
    });
  }
}

Engine createEngine() => FfiEngine.open();

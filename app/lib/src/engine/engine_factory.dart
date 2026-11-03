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

/// Pick the engine for the platform at compile time.
///
/// Native builds get the FFI engine; web builds get the HTTP one. The
/// conditional import means the web bundle never contains `dart:ffi` code and
/// the native binary never contains an HTTP client it would not use.
library;

export 'ffi_engine.dart' if (dart.library.js_interop) 'http_engine.dart'
    show createEngine;

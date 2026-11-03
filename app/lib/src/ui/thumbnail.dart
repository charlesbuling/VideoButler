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

/// A still from disk, where the platform can read one.
///
/// Native builds read the file the engine wrote. The web build cannot see
/// the filesystem at all, so it shows a placeholder rather than failing: the
/// album still works there, it just has no faces in it.
library;

export 'thumbnail_io.dart' if (dart.library.js_interop) 'thumbnail_web.dart';

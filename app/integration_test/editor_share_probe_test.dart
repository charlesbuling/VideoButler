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

/// Hands a list of videos to a phone editor, in order, the way the app
/// does after a cut, so what the editor makes of them can be looked at on
/// a real phone.
///
///   flutter test integration_test/editor_share_probe_test.dart -d <phone> \
///     --dart-define=VB_IT_SHARE=/sdcard/Movies/a.mp4,/sdcard/Movies/b.mp4 \
///     [--dart-define=VB_IT_EDITOR=com.lemon.lv]
library;

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('the editor opens with the videos in order', (tester) async {
    const share = String.fromEnvironment('VB_IT_SHARE');
    const editor = String.fromEnvironment('VB_IT_EDITOR', defaultValue: 'com.lemon.lv');
    if (share.isEmpty) {
      markTestSkipped('set VB_IT_SHARE to a comma-separated list of videos');
      return;
    }
    final paths = share.split(',');
    final sent = await const MethodChannel('videobutler/editors')
        .invokeMapMethod<String, Object?>('openInEditor', {'package': editor, 'paths': paths});
    // ignore: avoid_print
    print('handed ${sent?['shared']} of ${paths.length} videos to $editor');
    expect(sent?['shared'], paths.length);
  });
}
